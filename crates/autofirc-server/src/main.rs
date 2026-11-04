use anyhow::Context;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use autofirc_server::routes::{self, AppState, EnqueueFn};
use autofirc_server::worker::{self, EnrichJob};
use sqlx::PgPool;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL")?;
    let redis_url = std::env::var("REDIS_URL").context("REDIS_URL")?;
    let webhook_id = std::env::var("PAYPAL_WEBHOOK_ID").unwrap_or_else(|_| "DEV-WEBHOOK".into());
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let pool = PgPool::connect(&database_url).await?;
    autofirc_server::db::migrate(&pool).await?;

    let conn = apalis_redis::connect(redis_url).await?;
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

    let state = AppState {
        pool: pool.clone(),
        paypal_cert_http: reqwest::Client::new(),
        webhook_id,
        enqueue,
        pinned_webhook_key: None,
    };

    let worker_pool = pool.clone();
    tokio::spawn(async move {
        let paypal = paypal;
        Monitor::new()
            .register(WorkerBuilder::new("enrich").backend(storage).build_fn(
                move |job: EnrichJob| {
                    let paypal = paypal.clone();
                    let pool = worker_pool.clone();
                    async move { worker::enrich(job, &paypal, &pool).await }
                },
            ))
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
