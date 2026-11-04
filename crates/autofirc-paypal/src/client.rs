use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct PayPalClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    token: Arc<Mutex<Option<CachedToken>>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Order {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub purchase_units: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Capture {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub amount: Option<serde_json::Value>,
    #[serde(default)]
    pub seller_receivable_breakdown: Option<serde_json::Value>,
}

impl PayPalClient {
    pub fn new(base_url: &str, client_id: &str, client_secret: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            token: Arc::new(Mutex::new(None)),
        }
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = &*guard {
            if t.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(60) {
                return Ok(t.access_token.clone());
            }
        }
        let resp: TokenResponse = self
            .http
            .post(format!("{}/v1/oauth2/token", self.base_url))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = CachedToken {
            access_token: resp.access_token.clone(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(resp.expires_in),
        };
        *guard = Some(token);
        Ok(resp.access_token)
    }

    pub async fn get_order(&self, order_id: &str) -> anyhow::Result<Order> {
        let token = self.access_token().await?;
        Ok(self
            .http
            .get(format!("{}/v2/checkout/orders/{order_id}", self.base_url))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn get_capture(&self, capture_id: &str) -> anyhow::Result<Capture> {
        let token = self.access_token().await?;
        Ok(self
            .http
            .get(format!(
                "{}/v2/payments/captures/{capture_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn token_is_fetched_once_and_cached() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A21AA-test", "token_type": "Bearer", "expires_in": 3600
            })))
            .expect(1) // second call must reuse the cache
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/checkout/orders/ORDER-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "ORDER-1", "status": "COMPLETED", "purchase_units": []
            })))
            .mount(&server)
            .await;

        let client = PayPalClient::new(&server.uri(), "id", "secret");
        let o1 = client.get_order("ORDER-1").await.unwrap();
        let o2 = client.get_order("ORDER-1").await.unwrap();
        assert_eq!(o1.id, "ORDER-1");
        assert_eq!(o2.status, "COMPLETED");
    }

    #[tokio::test]
    async fn capture_fetches_settlement_breakdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A21AA-test", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/payments/captures/CAP-9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "CAP-9", "status": "COMPLETED",
                "amount": {"currency_code": "USD", "value": "500.00"},
                "seller_receivable_breakdown": {"net_amount": {"currency_code": "INR", "value": "41500.00"}}
            })))
            .mount(&server)
            .await;

        let client = PayPalClient::new(&server.uri(), "id", "secret");
        let cap = client.get_capture("CAP-9").await.unwrap();
        assert_eq!(cap.id, "CAP-9");
        assert!(cap.seller_receivable_breakdown.is_some());
    }
}
