//! Outbound real-world actions: bank-petition email + merchant alerts.
//! Primary channel is Zapier MCP (merchant-rewirable to 9,000+ apps);
//! when unconfigured, actions are logged so demo flows still complete.
use anyhow::Context;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BankPetition {
    pub event_id: String,
    pub invoice_number: String,
    pub invoice_pdf_url: String,
    pub lut_pdf_url: String,
    pub fira_reference: String, // paypal capture id acting as the remittance advice
    pub bank_officer_email: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerchantAlert {
    pub kind: String, // "fema_amber" | "fema_red" | "reconciled"
    pub event_id: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct ActionDispatcher {
    http: reqwest::Client,
    /// Zapier MCP / catch-hook endpoint. None → log-only mode.
    zapier_url: Option<String>,
}

impl ActionDispatcher {
    pub fn from_env() -> Self {
        Self {
            http: reqwest::Client::new(),
            zapier_url: std::env::var("ZAPIER_MCP_URL")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    async fn post(&self, action: &str, payload: &impl Serialize) -> anyhow::Result<()> {
        let Some(url) = &self.zapier_url else {
            tracing::info!(action, payload = %serde_json::to_string(payload)?, "action (log-only)");
            return Ok(());
        };
        self.http
            .post(url)
            .json(&serde_json::json!({ "action": action, "payload": payload }))
            .send()
            .await
            .context("zapier dispatch")?
            .error_for_status()
            .context("zapier status")?;
        Ok(())
    }

    pub async fn dispatch_bank_petition(&self, petition: &BankPetition) -> anyhow::Result<()> {
        self.post("bank_petition_email", petition).await
    }

    pub async fn alert(&self, alert: &MerchantAlert) -> anyhow::Result<()> {
        self.post("merchant_alert", alert).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn log_only_mode_succeeds_without_network() {
        let dispatcher = ActionDispatcher {
            http: reqwest::Client::new(),
            zapier_url: None,
        };
        dispatcher
            .alert(&MerchantAlert {
                kind: "reconciled".into(),
                event_id: "EV-1".into(),
                detail: "eBRC issued".into(),
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn petition_posts_action_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .and(body_partial_json(
                serde_json::json!({"action": "bank_petition_email"}),
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let dispatcher = ActionDispatcher {
            http: reqwest::Client::new(),
            zapier_url: Some(format!("{}/hook", server.uri())),
        };
        dispatcher
            .dispatch_bank_petition(&BankPetition {
                event_id: "EV-1".into(),
                invoice_number: "INV/2026-27/0001".into(),
                invoice_pdf_url: "https://signed/invoice".into(),
                lut_pdf_url: "https://signed/lut".into(),
                fira_reference: "CAP-1".into(),
                bank_officer_email: "trade.finance@bank.example".into(),
            })
            .await
            .unwrap();
    }
}
