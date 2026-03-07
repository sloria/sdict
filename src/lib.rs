pub mod spanishdict;

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    Router,
    extract::{Form, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

pub(crate) fn translation_letter(index: &usize) -> char {
    let clamped = (*index).min(25);
    (b'a' + clamped as u8) as char
}

pub struct AppState {
    pub client: Client,
    pub base_url: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "home.html")]
struct HomeTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "results.html")]
struct ResultsTemplate {
    term: spanishdict::Term,
    filter_tags: Vec<spanishdict::FilterTag>,
    active_filter: Option<String>,
    filtered_examples: Vec<spanishdict::CorpusExample>,
}

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
struct ErrorTemplate {
    query: String,
    message: String,
}

#[derive(Deserialize)]
pub struct SearchForm {
    term: String,
}

#[derive(Deserialize)]
pub struct TranslateQuery {
    pub filter: Option<String>,
}

async fn home() -> impl IntoResponse {
    HomeTemplate
}

async fn search(Form(form): Form<SearchForm>) -> impl IntoResponse {
    let term = form.term.trim().to_string();
    if term.is_empty() {
        return Redirect::to("/").into_response();
    }
    let encoded = urlencoding::encode(&term);
    Redirect::to(&format!("/translate/{encoded}")).into_response()
}

async fn translate(
    State(state): State<Arc<AppState>>,
    Path(term): Path<String>,
    Query(query): Query<TranslateQuery>,
) -> impl IntoResponse {
    match spanishdict::translate(&state.client, &state.base_url, &term).await {
        Ok(term) => {
            let filter_tags = spanishdict::extract_filter_tags(&term.examples);
            let filtered_examples = match &query.filter {
                Some(f) => spanishdict::filter_examples(&term.examples, f),
                None => term.examples.clone(),
            };
            ResultsTemplate {
                term,
                filter_tags,
                active_filter: query.filter,
                filtered_examples,
            }
            .into_response()
        }
        Err(spanishdict::SdictError::NotFound(t)) => (
            StatusCode::OK,
            ErrorTemplate {
                query: t.clone(),
                message: format!("No results for '{t}'."),
            },
        )
            .into_response(),
        Err(e) => {
            tracing::error!(term = %term, error = %e, "translation failed");
            (
                StatusCode::OK,
                ErrorTemplate {
                    query: term.clone(),
                    message: format!("Could not look up this term. Please try again. ({e})"),
                },
            )
                .into_response()
        }
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/search", post(search))
        .route("/translate/{term}", get(translate))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
