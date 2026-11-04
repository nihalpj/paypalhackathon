//! High-fidelity mock of the PayPal REST API for development and demo.
//! Mirrors the exact surface the worker pipeline consumes: OAuth2 client
//! credentials, order lookup, and capture lookup with the seller receivable
//! breakdown that carries the INR settlement.
//!
//! Canned scenarios are keyed by the trailing digit of the order/capture id
//! so the simulator can drive the full demo matrix without any seeding.
use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

/// A scenario = order + capture pair the mock serves for matching ids.
fn scenario(id: &str) -> (Value, Value) {
    let order_id = format!("ORDER-{id}");
    let capture_id = format!("CAP-{id}");
    let mk = |client: &str,
              email: &str,
              desc: &str,
              ccy: &str,
              foreign: &str,
              inr: &str,
              goods: bool| {
        let mut unit = json!({
            "reference_id": "default",
            "description": desc,
            "amount": {"currency_code": ccy, "value": foreign},
        });
        if goods {
            unit["shipping"] = json!({"name": {"full_name": client}});
            unit["shipping_address"] =
                json!({"address_line_1": "1 Export Plaza", "country_code": "US"});
        }
        let order = json!({
            "id": order_id,
            "status": "COMPLETED",
            "intent": "CAPTURE",
            "payer": {"name": {"given_name": client}, "email_address": email},
            "purchase_units": [unit],
        });
        let capture = json!({
            "id": capture_id,
            "status": "COMPLETED",
            "amount": {"currency_code": ccy, "value": foreign},
            "payer": {"name": {"given_name": client}, "email_address": email},
            "seller_receivable_breakdown": {
                "gross_amount": {"currency_code": ccy, "value": foreign},
                "paypal_fee": {"currency_code": ccy, "value": "0.00"},
                "net_amount": {"currency_code": "INR", "value": inr},
            },
            "create_time": "2026-11-04T09:00:00Z",
        });
        (order, capture)
    };
    match id.chars().last() {
        // High-confidence software services export (→ P0802, auto-packet).
        Some('1') => mk(
            "Globex Inc",
            "ap@globex.com",
            "Remote software consulting, October 2026 sprint",
            "USD",
            "500.00",
            "41500.00",
            false,
        ),
        // Deliberately ambiguous scope → low confidence → manual review.
        Some('2') => mk(
            "Acme Ltd",
            "finance@acme.co.uk",
            "General business services retainer (scope TBD)",
            "GBP",
            "1200.00",
            "126000.00",
            false,
        ),
        // Physical goods with shipping signals (→ P0103, passes backstop).
        Some('3') => mk(
            "Shenzhen Parts Co",
            "orders@szparts.cn",
            "Advance payment: prototype hardware batch 7",
            "USD",
            "2400.00",
            "199200.00",
            true,
        ),
        // Design services in EUR (→ services classification).
        Some('4') => mk(
            "Nordwind Studio",
            "hello@nordwind.eu",
            "Brand identity and UX design services",
            "EUR",
            "750.00",
            "67500.00",
            false,
        ),
        // Default: small services audit.
        _ => mk(
            "Initech LLC",
            "bill@initech.com",
            "Code review and architecture audit",
            "USD",
            "250.00",
            "20750.00",
            false,
        ),
    }
}

fn id_from_path(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_owned()
}

async fn oauth_token() -> impl IntoResponse {
    Json(json!({
        "scope": "https://uri.paypal.com/services/payments/payment",
        "access_token": "MOCK-A21AA-token",
        "token_type": "Bearer",
        "app_id": "MOCK-APP",
        "expires_in": 32400,
        "nonce": "mock-nonce",
    }))
}

async fn get_order(Path(id): Path<String>) -> impl IntoResponse {
    let (order, _) = scenario(&id_from_path(&id, "ORDER-"));
    Json(order)
}

async fn get_capture(Path(id): Path<String>) -> impl IntoResponse {
    let (_, capture) = scenario(&id_from_path(&id, "CAP-"));
    (StatusCode::OK, Json(capture))
}

pub fn router() -> Router {
    Router::new()
        .route("/v1/oauth2/token", post(oauth_token))
        .route("/v2/checkout/orders/{id}", get(get_order))
        .route("/v2/payments/captures/{id}", get(get_capture))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goods_scenario_carries_shipping_signals() {
        let (order, _) = scenario("demo-3");
        let hay = order.to_string();
        assert!(hay.contains("shipping_address"));
    }

    #[test]
    fn services_scenario_has_inr_breakdown() {
        let (_, capture) = scenario("demo-1");
        assert_eq!(
            capture
                .pointer("/seller_receivable_breakdown/net_amount/value")
                .and_then(|v| v.as_str()),
            Some("41500.00")
        );
    }
}
