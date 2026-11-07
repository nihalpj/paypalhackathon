//! One-off ops tool: run the reconcile pipeline (bank petition dispatch,
//! IRM seeding, eBRC filing) for the given capture ids directly against the
//! database, bypassing the apalis queue. Use when the queue is unavailable
//! or a row is stuck in `bank_petitioned`.
//!
//! Usage:
//!   cargo run -p autofirc-server --bin reconcile-now -- <capture_id>...

use anyhow::Context;
use autofirc_server::{db, storage::PdfStore, worker};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let ids: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(!ids.is_empty(), "usage: reconcile-now <capture_id>...");

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    let store = PdfStore::from_env().ok();
    let pipeline = worker::ReconcilePipeline::from_env(pool.clone(), store);

    for id in ids {
        // Idempotence guard: skip rows already reconciled.
        let row = db::get_row(&pool, &id)
            .await?
            .with_context(|| format!("row {id} not found"))?;
        let status = row
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if status == "fully_reconciled" {
            println!("{id}: already fully_reconciled, skipping");
            continue;
        }
        println!("{id}: reconciling from status {status}");
        worker::reconcile(worker::ReconcileJob { event_id: id.clone() }, &pipeline).await?;
        let row = db::get_row(&pool, &id).await?.unwrap_or_default();
        println!(
            "{id}: now {} (eBRC: {})",
            row.get("status").and_then(|v| v.as_str()).unwrap_or("?"),
            row.get("ebrc_number").and_then(|v| v.as_str()).unwrap_or("-")
        );
    }
    Ok(())
}
