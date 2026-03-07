use reqwest::Client;
use sdict::{AppState, build_router};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

const SPANISHDICT_BASE_URL: &str = "https://www.spanishdict.com";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");

    let state = Arc::new(AppState {
        client: Client::new(),
        base_url: SPANISHDICT_BASE_URL.to_string(),
    });

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}
