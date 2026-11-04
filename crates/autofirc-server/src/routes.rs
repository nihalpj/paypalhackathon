use autofirc_paypal::webhook::{self, TransmissionMeta};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use sqlx::PgPool;
use std::sync::Arc;

use crate::worker::EnrichJob;

pub type BoxFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
pub type EnqueueFn = Arc<dyn Fn(EnrichJob) -> BoxFuture + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub paypal_cert_http: reqwest::Client,
    pub webhook_id: String,
    /// Queue sink: verified worker job payloads.
    pub enqueue: EnqueueFn,
    /// Test hook: when set, skips cert fetch and uses this key directly.
    pub pinned_webhook_key: Option<rsa::RsaPublicKey>,
}

#[derive(Debug, serde::Deserialize)]
struct WebhookEvent {
    id: String,
    #[serde(default)]
    resource: Option<WebhookResource>,
}

#[derive(Debug, serde::Deserialize)]
struct WebhookResource {
    id: Option<String>,
    supplementary_data: Option<serde_json::Value>,
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

pub async fn paypal_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let (Some(tid), Some(tt), Some(cert), Some(sig)) = (
        header(&headers, "paypal-transmission-id"),
        header(&headers, "paypal-transmission-time"),
        header(&headers, "paypal-cert-url"),
        header(&headers, "paypal-transmission-sig"),
    ) else {
        return StatusCode::BAD_REQUEST;
    };
    let meta = TransmissionMeta {
        transmission_id: tid.to_owned(),
        transmission_time: tt.to_owned(),
        cert_url: cert.to_owned(),
        transmission_sig: sig.to_owned(),
    };
    let key = match &state.pinned_webhook_key {
        Some(k) => k.clone(),
        None => match webhook::fetch_public_key(&state.paypal_cert_http, &meta.cert_url).await {
            Ok(k) => k,
            Err(_) => return StatusCode::UNAUTHORIZED,
        },
    };
    if webhook::verify_signature(&meta, &state.webhook_id, &body, &key).is_err() {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event) = serde_json::from_slice::<WebhookEvent>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    let capture_id = event
        .resource
        .as_ref()
        .and_then(|r| r.id.clone())
        .unwrap_or_default();
    let order_id = event
        .resource
        .and_then(|r| r.supplementary_data)
        .and_then(|sd| {
            sd.get("related_ids")?
                .get("order_id")?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    match crate::db::insert_webhook_event(&state.pool, &event.id, &capture_id, &order_id).await {
        Ok(true) => {
            (state.enqueue)(EnrichJob {
                event_id: event.id,
                capture_id,
                order_id,
            })
            .await;
            StatusCode::OK
        }
        Ok(false) => StatusCode::OK, // duplicate: silently ack, no reprocessing
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn list_ledger(State(state): State<AppState>) -> impl IntoResponse {
    match crate::db::list_ledger(&state.pool).await {
        Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/webhooks/paypal", post(paypal_webhook))
        .route("/api/ledger", get(list_ledger))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD as B64};
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};
    use sha2::Sha256;
    use std::sync::Mutex as StdMutex;

    async fn test_state() -> (AppState, rsa::RsaPrivateKey, Arc<StdMutex<Vec<String>>>) {
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        // A pool that is never reached in the unauthorized-path tests.
        let pool = sqlx::PgPool::connect_lazy("postgres://unused:unused@localhost/unused").unwrap();
        let enqueued = Arc::new(StdMutex::new(Vec::new()));
        let enq = enqueued.clone();
        let state = AppState {
            pool,
            paypal_cert_http: reqwest::Client::new(),
            webhook_id: "WH-TEST".into(),
            enqueue: Arc::new(move |job: EnrichJob| {
                let enq = enq.clone();
                Box::pin(async move {
                    enq.lock().unwrap().push(job.event_id);
                })
            }),
            pinned_webhook_key: Some(public),
        };
        (state, private, enqueued)
    }

    #[tokio::test]
    async fn rejects_bad_signature_with_401() {
        let (state, _priv, enqueued) = test_state().await;
        let app = router(state);
        let req = axum::http::Request::post("/api/webhooks/paypal")
            .header("paypal-transmission-id", "t1")
            .header("paypal-transmission-time", "2026-11-04T00:00:00Z")
            .header("paypal-cert-url", "https://api.paypal.com/c.pem")
            .header("paypal-transmission-sig", B64.encode(b"garbage"))
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(enqueued.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_missing_headers_with_400() {
        let (state, _priv, _) = test_state().await;
        let app = router(state);
        let req = axum::http::Request::post("/api/webhooks/paypal")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn signature_fixture_signs_correctly() {
        // Sanity: the test harness signs exactly what the route verifies.
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        let meta = TransmissionMeta {
            transmission_id: "t1".into(),
            transmission_time: "2026-11-04T00:00:00Z".into(),
            cert_url: "https://api.paypal.com/c.pem".into(),
            transmission_sig: String::new(),
        };
        let body = br#"{"id":"EV-1","resource":{"id":"CAP-1"}}"#;
        let msg = webhook::signed_message(&meta, "WH-TEST", body);
        let sig = SigningKey::<Sha256>::new(private).sign_with_rng(&mut rng, msg.as_bytes());
        let meta = TransmissionMeta {
            transmission_sig: B64.encode(sig.to_bytes()),
            ..meta
        };
        assert!(webhook::verify_signature(&meta, "WH-TEST", body, &public).is_ok());
    }
}
