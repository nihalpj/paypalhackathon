//! Standalone mock DGFT server for demo/development.
//! Run: cargo run -p autofirc-dgft --bin mock-dgft
#[tokio::main]
async fn main() {
    let bind = std::env::var("MOCK_DGFT_ADDR").unwrap_or_else(|_| "127.0.0.1:4010".into());
    let app = autofirc_dgft::mock::MockDgft::new().router();
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind mock dgft");
    println!("mock DGFT listening on http://{bind}");
    axum::serve(listener, app).await.expect("serve mock dgft");
}
