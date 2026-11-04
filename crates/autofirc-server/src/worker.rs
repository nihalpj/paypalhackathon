use anyhow::Context;
use autofirc_ai::classify::Classifier;
use autofirc_paypal::client::PayPalClient;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::storage::PdfStore;

/// Job pushed by the webhook route after signature verification + DB insert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichJob {
    pub event_id: String,
    pub capture_id: String,
    pub order_id: String,
}

/// Merchant identity used on generated GST documents (from env).
#[derive(Debug, Clone)]
pub struct MerchantProfile {
    pub name: String,
    pub address: String,
    pub gstin: String,
    pub lut_reference: String,
    pub financial_year: String,
}

impl MerchantProfile {
    pub fn from_env() -> Self {
        Self {
            name: std::env::var("MERCHANT_NAME").unwrap_or_else(|_| "Demo Exports LLP".into()),
            address: std::env::var("MERCHANT_ADDRESS")
                .unwrap_or_else(|_| "Bengaluru, Karnataka, India".into()),
            gstin: std::env::var("MERCHANT_GSTIN").unwrap_or_else(|_| "29ABCDE1234F1Z5".into()),
            lut_reference: std::env::var("MERCHANT_LUT_REF")
                .unwrap_or_else(|_| "LUT/2026-27/DEMO0001".into()),
            financial_year: std::env::var("MERCHANT_FY").unwrap_or_else(|_| "2026-27".into()),
        }
    }
}

/// Everything the worker pipeline needs.
#[derive(Clone)]
pub struct Pipeline {
    pub paypal: PayPalClient,
    pub pool: PgPool,
    /// None when no LLM key is configured: rows go straight to manual review.
    pub classifier: Option<Classifier>,
    /// None in offline/dev runs: classification still happens, PDFs don't.
    pub store: Option<PdfStore>,
    pub merchant: MerchantProfile,
}

/// Best-effort INR settlement in paise from the capture's seller breakdown.
pub fn extract_inr_paise(enrichment: &serde_json::Value) -> i64 {
    enrichment
        .pointer("/capture/seller_receivable_breakdown/net_amount/value")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| (v * 100.0).round() as i64)
        .unwrap_or(0)
}

/// Best-effort client display name from the order's payer block.
pub fn extract_client_name(enrichment: &serde_json::Value) -> String {
    enrichment
        .pointer("/capture/payer/name/given_name")
        .or_else(|| enrichment.pointer("/order/payer/name/given_name"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .or_else(|| {
            enrichment
                .pointer("/order/payer/email_address")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Foreign Client".into())
}

/// Best-effort service description from the first purchase unit.
pub fn extract_service_description(enrichment: &serde_json::Value) -> String {
    enrichment
        .pointer("/order/purchase_units/0/description")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "Export of digital services".into())
}

/// Foreign amount string, e.g. "USD 500.00".
pub fn extract_foreign_amount(enrichment: &serde_json::Value) -> String {
    let ccy = enrichment
        .pointer("/capture/amount/currency_code")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let val = enrichment
        .pointer("/capture/amount/value")
        .and_then(|v| v.as_str())
        .unwrap_or("0.00");
    format!("{ccy} {val}")
}

/// Phase 2 pipeline: enrich → classify → documents.
pub async fn process(job: EnrichJob, p: &Pipeline) -> anyhow::Result<()> {
    // 1. Enrich with full PayPal order/capture data.
    let order = p
        .paypal
        .get_order(&job.order_id)
        .await
        .context("get order")?;
    let capture = p
        .paypal
        .get_capture(&job.capture_id)
        .await
        .context("get capture")?;
    let enrichment = serde_json::json!({ "order": order, "capture": capture });
    crate::db::set_enrichment(&p.pool, &job.event_id, &enrichment, "enriched").await?;

    // 2. AI classification under regulatory guardrails.
    let Some(classifier) = &p.classifier else {
        crate::db::set_enrichment(&p.pool, &job.event_id, &enrichment, "manual_review").await?;
        tracing::warn!(event_id = %job.event_id, "no classifier configured; manual review");
        return Ok(());
    };
    let classified = classifier.classify(&enrichment).await.context("classify")?;
    let c = &classified.classification;
    crate::db::set_classification(
        &p.pool,
        &job.event_id,
        &crate::db::ClassificationUpdate {
            purpose_code: &c.purpose_code,
            mode_of_export: c.mode_of_export as i16,
            sac_code: &c.sac_code,
            confidence: c.confidence_score as i16,
            backstop_note: classified.backstop_note.as_deref(),
            status: if classified.needs_review {
                "manual_review"
            } else {
                "enriched"
            },
        },
    )
    .await?;
    if classified.needs_review {
        tracing::info!(event_id = %job.event_id, "routed to manual review");
        return Ok(());
    }

    // 3. Generate GST Rule 46 invoice + LUT PDFs and store them.
    let Some(store) = &p.store else {
        tracing::warn!(event_id = %job.event_id, "no object storage configured; skipping PDFs");
        return Ok(());
    };
    let invoice_number =
        crate::db::next_invoice_number(&p.pool, &p.merchant.financial_year).await?;
    let invoice = autofirc_pdf::InvoiceData {
        supplier_name: p.merchant.name.clone(),
        supplier_address: p.merchant.address.clone(),
        supplier_gstin: p.merchant.gstin.clone(),
        invoice_number: invoice_number.clone(),
        invoice_date: chrono::Utc::now().date_naive(),
        recipient_name: extract_client_name(&enrichment),
        recipient_address: String::new(),
        recipient_country: "—".into(),
        service_description: extract_service_description(&enrichment),
        sac_code: c.sac_code.clone(),
        taxable_value_foreign: extract_foreign_amount(&enrichment),
        taxable_value_inr_paise: extract_inr_paise(&enrichment),
        lut_reference: p.merchant.lut_reference.clone(),
        place_of_supply: "Outside India".into(),
        purpose_code: c.purpose_code.clone(),
        mode_of_export: c.mode_of_export,
    };
    let invoice_pdf = autofirc_pdf::render_invoice(&invoice).context("render invoice")?;
    let lut_pdf = autofirc_pdf::render_lut(&autofirc_pdf::LutData {
        legal_name: p.merchant.name.clone(),
        gstin: p.merchant.gstin.clone(),
        lut_reference: p.merchant.lut_reference.clone(),
        financial_year: p.merchant.financial_year.clone(),
        date: chrono::Utc::now().date_naive(),
    })
    .context("render lut")?;
    let invoice_key = store
        .upload_pdf(&job.event_id, "invoice", &invoice_pdf)
        .await?;
    let lut_key = store.upload_pdf(&job.event_id, "lut", &lut_pdf).await?;
    crate::db::set_documents(
        &p.pool,
        &job.event_id,
        &invoice_number,
        &invoice_key,
        &lut_key,
        "compliance_packet_ready",
    )
    .await?;
    tracing::info!(event_id = %job.event_id, invoice = %invoice_number, "compliance packet ready");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_enrichment() -> serde_json::Value {
        json!({
            "order": {
                "payer": {"name": {"given_name": "Jane Doe"}, "email_address": "jane@globex.com"},
                "purchase_units": [{"description": "Remote software consulting"}]
            },
            "capture": {
                "amount": {"currency_code": "USD", "value": "500.00"},
                "seller_receivable_breakdown": {"net_amount": {"currency_code": "INR", "value": "41500.00"}}
            }
        })
    }

    #[test]
    fn extracts_fields_from_enrichment() {
        let e = sample_enrichment();
        assert_eq!(extract_inr_paise(&e), 4_150_000);
        assert_eq!(extract_client_name(&e), "Jane Doe");
        assert_eq!(
            extract_service_description(&e),
            "Remote software consulting"
        );
        assert_eq!(extract_foreign_amount(&e), "USD 500.00");
    }

    #[test]
    fn missing_fields_have_safe_defaults() {
        let e = json!({});
        assert_eq!(extract_inr_paise(&e), 0);
        assert_eq!(extract_client_name(&e), "Foreign Client");
        assert_eq!(
            extract_service_description(&e),
            "Export of digital services"
        );
        assert_eq!(extract_foreign_amount(&e), " 0.00");
    }
}
