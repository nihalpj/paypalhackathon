//! High-fidelity mock of the DGFT Open API for development and demo.
//! The real API requires an IEC, IP whitelisting, and FIU registration —
//! this mock implements the same surface: PBKDF2 token handshake, IRM
//! repository, and signed eBRC self-certification.
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::*;

#[derive(Default)]
pub struct MockState {
    irms: Vec<IrmDto>,
    issued: Vec<EbrcRequest>,
    ebrc_seq: u64,
}

#[derive(Clone)]
pub struct MockDgft {
    state: Arc<Mutex<MockState>>,
}

impl Default for MockDgft {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDgft {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState::default())),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/token", post(token))
            .route("/irms", get(list_irms))
            .route("/seed-irm", post(seed_irm))
            .route("/ebrc", post(submit_ebrc))
            .route("/ebrcs", get(list_ebrcs))
            .with_state(self.state.clone())
    }
}

async fn token(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(req): Json<TokenRequest>,
) -> impl IntoResponse {
    // Reject empty credentials exactly like a real handshake would.
    if req.api_key.is_empty() || req.client_id.is_empty() || req.client_secret_hash.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        )
            .into_response();
    }
    let seq = {
        let mut s = state.lock().unwrap();
        s.ebrc_seq += 0; // state touched to keep the lock meaningful
        s.ebrc_seq
    };
    let _ = seq;
    Json(TokenResponse {
        access_token: format!("mock-token-{}", &req.client_id),
        expires_in: 3600,
    })
    .into_response()
}

async fn list_irms(
    State(state): State<Arc<Mutex<MockState>>>,
    Query(_params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    Json(state.lock().unwrap().irms.clone())
}

async fn seed_irm(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(irm): Json<IrmDto>,
) -> impl IntoResponse {
    let mut s = state.lock().unwrap();
    if !s.irms.iter().any(|i| i.irm_number == irm.irm_number) {
        s.irms.push(irm);
    }
    StatusCode::CREATED
}

async fn submit_ebrc(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(req): Json<EbrcRequest>,
) -> impl IntoResponse {
    if req.signature.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing RSA-SHA256 signature"})),
        )
            .into_response();
    }
    if !(1..=4).contains(&req.mode_of_export) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "mode_of_export must be 1-4"})),
        )
            .into_response();
    }
    if req.gstin.is_empty() || req.invoice_number.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "gst invoice details are mandatory since 13.01.2026"}),
            ),
        )
            .into_response();
    }
    let mut s = state.lock().unwrap();
    // IRMs referenced must exist in the repository.
    for n in &req.irm_numbers {
        if !s.irms.iter().any(|i| &i.irm_number == n) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("unknown IRM {n}")})),
            )
                .into_response();
        }
    }
    s.ebrc_seq += 1;
    s.irms.retain(|i| !req.irm_numbers.contains(&i.irm_number));
    s.issued.push(req);
    Json(EbrcResponse {
        ebrc_number: format!("EBRC-2026-{:04}", s.ebrc_seq),
        status: "ISSUED".into(),
    })
    .into_response()
}

async fn list_ebrcs(State(state): State<Arc<Mutex<MockState>>>) -> impl IntoResponse {
    Json(state.lock().unwrap().issued.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::DgftClient;

    #[tokio::test]
    async fn full_mock_round_trip() {
        let mock = MockDgft::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, mock.router()).await.unwrap();
        });
        let base = format!("http://{addr}");

        // Seed an IRM (as a bank would transmit).
        reqwest::Client::new()
            .post(format!("{base}/seed-irm"))
            .json(&IrmDto {
                irm_number: "IRM-9".into(),
                bank_name: "HDFC".into(),
                account_number: "001".into(),
                currency: "USD".into(),
                amount: "500.00".into(),
                remittance_date: "2026-11-01".into(),
                remitter_name: "Globex Inc".into(),
            })
            .send()
            .await
            .unwrap();

        let mut rng = rand::thread_rng();
        let client = DgftClient::new(
            &base,
            "api-key",
            "client-id",
            "client-secret",
            rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap(),
        );
        let token = client.access_token().await.unwrap();
        assert!(token.starts_with("mock-token-"));

        let irms = client.fetch_irms(&token, "0123456789").await.unwrap();
        assert_eq!(irms.len(), 1);

        let resp = client
            .submit_ebrc(
                &token,
                EbrcRequest {
                    iec: "0123456789".into(),
                    irm_numbers: vec!["IRM-9".into()],
                    invoice_number: "INV/2026-27/0001".into(),
                    invoice_date: "2026-11-04".into(),
                    invoice_value_inr: "41500.00".into(),
                    gstin: "29ABCDE1234F1Z5".into(),
                    sac_code: "998314".into(),
                    mode_of_export: 1,
                    purpose_code: "P0802".into(),
                    signature: String::new(),
                },
            )
            .await
            .unwrap();
        assert!(resp.ebrc_number.starts_with("EBRC-2026-"));

        // The IRM is consumed after certification.
        assert!(
            client
                .fetch_irms(&token, "0123456789")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
