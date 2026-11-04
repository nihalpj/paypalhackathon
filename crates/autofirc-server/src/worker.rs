use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Job pushed by the webhook route after signature verification + DB insert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichJob {
    pub event_id: String,
    pub capture_id: String,
    pub order_id: String,
}

/// Enrich a ledger row with full PayPal order/capture data.
/// Failure leaves the row in `pending_analysis` for retry.
pub async fn enrich(
    job: EnrichJob,
    paypal: &autofirc_paypal::client::PayPalClient,
    pool: &sqlx::PgPool,
) -> anyhow::Result<()> {
    let order = paypal.get_order(&job.order_id).await.context("get order")?;
    let capture = paypal
        .get_capture(&job.capture_id)
        .await
        .context("get capture")?;
    let enrichment = serde_json::json!({ "order": order, "capture": capture });
    crate::db::set_enrichment(pool, &job.event_id, &enrichment, "enriched").await?;
    tracing::info!(event_id = %job.event_id, "enriched ledger row");
    Ok(())
}
