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

// ---------- Phase 4: bank petition + DGFT reconciliation ----------

use autofirc_dgft::client::DgftClient;
use autofirc_dgft::types::{EbrcRequest, IrmDto};

/// Job enqueued when the merchant authorizes a bank petition for a row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileJob {
    pub event_id: String,
}

#[derive(Clone)]
pub struct ReconcilePipeline {
    pub pool: PgPool,
    pub store: Option<PdfStore>,
    pub dispatcher: crate::actions::ActionDispatcher,
    pub dgft: Option<DgftClient>,
    pub iec: String,
    pub bank_officer_email: String,
}

impl ReconcilePipeline {
    pub fn from_env(pool: PgPool, store: Option<PdfStore>) -> Self {
        let dgft = std::env::var("DGFT_BASE_URL").ok().map(|base| {
            let key_pem = std::env::var("DGFT_SIGNING_KEY_PEM").unwrap_or_default();
            let signing_key = if key_pem.is_empty() {
                let mut rng = rand::thread_rng();
                rsa::RsaPrivateKey::new(&mut rng, 2048).expect("demo signing key")
            } else {
                use rsa::pkcs8::DecodePrivateKey;
                rsa::RsaPrivateKey::from_pkcs8_pem(&key_pem).expect("valid DGFT_SIGNING_KEY_PEM")
            };
            DgftClient::new(
                &base,
                &std::env::var("DGFT_API_KEY").unwrap_or_else(|_| "demo".into()),
                &std::env::var("DGFT_CLIENT_ID").unwrap_or_else(|_| "demo".into()),
                &std::env::var("DGFT_CLIENT_SECRET").unwrap_or_else(|_| "demo".into()),
                signing_key,
            )
        });
        Self {
            pool,
            store,
            dispatcher: crate::actions::ActionDispatcher::from_env(),
            dgft,
            iec: std::env::var("MERCHANT_IEC").unwrap_or_else(|_| "0123456789".into()),
            bank_officer_email: std::env::var("BANK_OFFICER_EMAIL")
                .unwrap_or_else(|_| "trade.finance@bank.example".into()),
        }
    }
}

fn jstr<'a>(row: &'a serde_json::Value, key: &str) -> &'a str {
    row.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Petition the bank for one row, then run a reconciliation pass over all
/// bank-petitioned rows against the DGFT IRM repository.
pub async fn reconcile(job: ReconcileJob, p: &ReconcilePipeline) -> anyhow::Result<()> {
    let row = crate::db::get_row(&p.pool, &job.event_id)
        .await?
        .with_context(|| format!("row {} not found", job.event_id))?;

    // 1. Dispatch the compliance packet to the bank officer.
    let store = p.store.as_ref().context("object storage required")?;
    let invoice_key = jstr(&row, "invoice_pdf_key").to_owned();
    let lut_key = jstr(&row, "lut_pdf_key").to_owned();
    let invoice_url = store.signed_url(&invoice_key, 900).await?;
    let lut_url = store.signed_url(&lut_key, 900).await?;
    p.dispatcher
        .dispatch_bank_petition(&crate::actions::BankPetition {
            event_id: job.event_id.clone(),
            invoice_number: jstr(&row, "invoice_number").to_owned(),
            invoice_pdf_url: invoice_url,
            lut_pdf_url: lut_url,
            fira_reference: jstr(&row, "capture_id").to_owned(),
            bank_officer_email: p.bank_officer_email.clone(),
        })
        .await?;
    crate::db::set_status(&p.pool, &job.event_id, "bank_petitioned").await?;

    // 2. Simulate the bank transmitting an IRM to DGFT (in production the
    //    bank does this; against the mock we seed it ourselves).
    if let Some(dgft) = &p.dgft {
        let token = dgft.access_token().await?;
        let foreign =
            extract_foreign_amount(row.get("enrichment").unwrap_or(&serde_json::Value::Null));
        let (ccy, amt) = foreign.split_once(' ').unwrap_or(("USD", "0.00"));
        let remitter =
            extract_client_name(row.get("enrichment").unwrap_or(&serde_json::Value::Null));
        seed_irm(
            dgft.base_url(),
            IrmDto {
                irm_number: format!("IRM-{}", jstr(&row, "capture_id")),
                bank_name: "Demo AD Bank".into(),
                account_number: "001".into(),
                currency: ccy.to_owned(),
                amount: amt.to_owned(),
                remittance_date: chrono::Utc::now().date_naive().to_string(),
                remitter_name: remitter,
            },
        )
        .await?;

        // 3. Reconciliation pass: match pending IRMs to bank-petitioned rows.
        let irms = dgft.fetch_irms(&token, &p.iec).await?;
        let pending = crate::db::list_by_status(&p.pool, "bank_petitioned").await?;
        for row in pending {
            let capture = jstr(&row, "capture_id");
            let irm_number = format!("IRM-{capture}");
            if !irms.iter().any(|i| i.irm_number == irm_number) {
                continue;
            }
            let resp = dgft
                .submit_ebrc(
                    &token,
                    EbrcRequest {
                        iec: p.iec.clone(),
                        irm_numbers: vec![irm_number],
                        invoice_number: jstr(&row, "invoice_number").to_owned(),
                        invoice_date: jstr(&row, "invoice_date").to_owned(),
                        invoice_value_inr: format!(
                            "{:.2}",
                            row.get("amount_inr_paise")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0) as f64
                                / 100.0
                        ),
                        gstin: p.iec_merchant_gstin(),
                        sac_code: jstr(&row, "sac_code").to_owned(),
                        mode_of_export: row
                            .get("mode_of_export")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(1) as u8,
                        purpose_code: jstr(&row, "purpose_code").to_owned(),
                        signature: String::new(),
                    },
                )
                .await?;
            crate::db::set_ebrc(&p.pool, jstr(&row, "id"), &resp.ebrc_number).await?;
            p.dispatcher
                .alert(&crate::actions::MerchantAlert {
                    kind: "reconciled".into(),
                    event_id: jstr(&row, "id").to_owned(),
                    detail: format!("eBRC {} issued", resp.ebrc_number),
                })
                .await?;
            tracing::info!(event_id = %jstr(&row, "id"), ebrc = %resp.ebrc_number, "fully reconciled");
        }
    }
    Ok(())
}

impl ReconcilePipeline {
    fn iec_merchant_gstin(&self) -> String {
        std::env::var("MERCHANT_GSTIN").unwrap_or_else(|_| "29ABCDE1234F1Z5".into())
    }
}

/// Seed an IRM at the (mock) DGFT, simulating the bank's transmission.
async fn seed_irm(dgft_base_url: &str, irm: IrmDto) -> anyhow::Result<()> {
    reqwest::Client::new()
        .post(format!("{}/seed-irm", dgft_base_url.trim_end_matches('/')))
        .json(&irm)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
