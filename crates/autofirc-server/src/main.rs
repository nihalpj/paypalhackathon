use anyhow::Context;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use autofirc_server::routes::{self, AppState, EnqueueFn};
use autofirc_server::worker::{self, EnrichJob};
use sqlx::PgPool;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Both `ring` and `aws-lc-rs` are in the graph (rig pulls the latter);
    // pick ring explicitly so rustls never panics on provider ambiguity.
    let _ = rustls::crypto::ring::default_provider().install_default();
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL")?;
    let redis_url = std::env::var("REDIS_URL").context("REDIS_URL")?;
    let webhook_id = std::env::var("PAYPAL_WEBHOOK_ID").unwrap_or_else(|_| "DEV-WEBHOOK".into());
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let pool = PgPool::connect(&database_url).await?;
    autofirc_server::db::migrate(&pool).await?;

    let conn = apalis_redis::connect(redis_url.clone()).await?;
    let storage = RedisStorage::new(conn);

    let paypal = autofirc_paypal::client::PayPalClient::new(
        &std::env::var("PAYPAL_BASE_URL")
            .unwrap_or_else(|_| "https://api-m.sandbox.paypal.com".into()),
        &std::env::var("PAYPAL_CLIENT_ID").unwrap_or_default(),
        &std::env::var("PAYPAL_CLIENT_SECRET").unwrap_or_default(),
    );

    let push_storage = storage.clone();
    let enqueue: EnqueueFn = Arc::new(move |job: EnrichJob| {
        let mut storage = push_storage.clone();
        Box::pin(async move {
            if let Err(e) = Storage::push(&mut storage, job).await {
                tracing::error!(error = %e, "failed to enqueue enrich job");
            }
        })
    });

    let reconcile_storage = RedisStorage::new(apalis_redis::connect(redis_url).await?);
    let push_reconcile = reconcile_storage.clone();
    let enqueue_reconcile: routes::ReconcileEnqueueFn =
        Arc::new(move |job: worker::ReconcileJob| {
            let mut storage = push_reconcile.clone();
            Box::pin(async move {
                if let Err(e) = Storage::push(&mut storage, job).await {
                    tracing::error!(error = %e, "failed to enqueue reconcile job");
                }
            })
        });

    let pdf_store = autofirc_server::storage::PdfStore::from_env().ok();
    let state = AppState {
        pool: pool.clone(),
        paypal_cert_http: reqwest::Client::new(),
        webhook_id,
        enqueue,
        enqueue_reconcile,
        store: pdf_store.clone(),
        pinned_webhook_key: None,
    };

    let classifier = match autofirc_ai::provider::classifier_from_env() {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "no LLM provider configured; rows will go to manual review");
            None
        }
    };
    let pipeline = worker::Pipeline {
        paypal,
        pool: pool.clone(),
        classifier,
        store: pdf_store.clone(),
        merchant: worker::MerchantProfile::from_env(),
    };
    let reconcile_pipeline = worker::ReconcilePipeline::from_env(pool.clone(), pdf_store.clone());

    tokio::spawn(async move {
        Monitor::new()
            .register(WorkerBuilder::new("enrich").backend(storage).build_fn(
                move |job: EnrichJob| {
                    let pipeline = pipeline.clone();
                    async move { worker::process(job, &pipeline).await }
                },
            ))
            .register(
                WorkerBuilder::new("reconcile")
                    .backend(reconcile_storage)
                    .build_fn(move |job: worker::ReconcileJob| {
                        let pipeline = reconcile_pipeline.clone();
                        async move { worker::reconcile(job, &pipeline).await }
                    }),
            )
            .run()
            .await
            .expect("worker monitor");
    });

    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "autofirc-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
