use reqwest::Client;
use sdict::{AppState, build_router};
use sentry::integrations::tracing as sentry_tracing;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

const SPANISHDICT_BASE_URL: &str = "https://www.spanishdict.com";

#[tokio::main]
async fn main() {
    let _sentry_guard = sentry::init(sentry::ClientOptions {
        release: sentry::release_name!(),
        environment: std::env::var("SENTRY_ENV").ok().map(Into::into),
        ..Default::default()
    });

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();

    match log_format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .with(sentry_tracing::layer())
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .with(sentry_tracing::layer())
                .init();
        }
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");

    let state = AppState {
        client: Client::new(),
        base_url: SPANISHDICT_BASE_URL.to_string(),
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}
