pub mod spanishdict;

use askama::Template;
use askama_web::WebTemplate;
use axum::http::HeaderValue;
use axum::{
    Router,
    extract::{Form, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use reqwest::Client;
use serde::Deserialize;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub base_url: String,
}

// --- Template structs ---

struct SearchFormProps {
    value: String,
    autofocus: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "home.html")]
struct HomeTemplate {
    search: SearchFormProps,
}

#[derive(Template, WebTemplate)]
#[template(path = "results.html")]
struct ResultsTemplate {
    search: SearchFormProps,
    term: spanishdict::Term,
    filter_tags: Vec<spanishdict::FilterTag>,
    active_filter: Option<String>,
    filtered_examples: Vec<spanishdict::CorpusExample>,
}

#[derive(Template, WebTemplate)]
#[template(path = "error.html")]
struct ErrorTemplate {
    search: SearchFormProps,
    message: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "404.html")]
struct NotFoundTemplate {
    search: SearchFormProps,
}

// Custom jinja filters
mod filters {
    /// Convert an index to a letter (0 -> 'a', 1 -> 'b', ...)
    /// Useful for rendering nested ordered lists.
    #[askama::filter_fn]
    pub fn index_to_letter(index: &usize, _: &dyn askama::Values) -> askama::Result<char> {
        let clamped = (*index).min(25);
        Ok((b'a' + clamped as u8) as char)
    }
}

// --- Structs for handlers ---

#[derive(Deserialize)]
pub struct SearchForm {
    term: String,
}

#[derive(Deserialize)]
pub struct TranslateQuery {
    pub filter: Option<String>,
}

// --- Handlers ---

async fn home() -> impl IntoResponse {
    HomeTemplate {
        search: SearchFormProps {
            value: String::new(),
            autofocus: true,
        },
    }
}

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        NotFoundTemplate {
            search: SearchFormProps {
                value: String::new(),
                autofocus: true,
            },
        },
    )
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
    State(state): State<AppState>,
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
            let search = SearchFormProps {
                value: term.query.clone(),
                autofocus: false,
            };
            ResultsTemplate {
                search,
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
                search: SearchFormProps {
                    value: t.clone(),
                    autofocus: true,
                },
                message: format!("No results for '{t}'."),
            },
        )
            .into_response(),
        Err(e) => {
            tracing::error!(term = %term, error = %e, "translation failed");
            (
                StatusCode::OK,
                ErrorTemplate {
                    search: SearchFormProps {
                        value: term.clone(),
                        autofocus: true,
                    },
                    message: "Could not look up this term. Please try again.".to_string(),
                },
            )
                .into_response()
        }
    }
}

// --- Router ---

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/search", post(search))
        .route("/translate/{term}", get(translate))
        .fallback(get(not_found))
        .nest_service("/static", ServeDir::new("static"))
        // Security headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'self'; style-src 'unsafe-inline' 'self'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
