//! Standalone mock PayPal server for demo/development.
//! Run: cargo run -p autofirc-paypal --bin mock-paypal
#[tokio::main]
async fn main() {
    let bind = std::env::var("MOCK_PAYPAL_ADDR").unwrap_or_else(|_| "127.0.0.1:4011".into());
    let app = autofirc_paypal::mock::router();
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind mock paypal");
    println!("mock PayPal listening on http://{bind}");
    axum::serve(listener, app).await.expect("serve mock paypal");
}
