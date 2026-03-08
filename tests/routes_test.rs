use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use reqwest::Client;
use sdict::{AppState, build_router};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture file exists")
}

fn app(base_url: &str) -> axum::Router {
    let state = AppState {
        client: Client::new(),
        base_url: base_url.to_string(),
    };
    build_router(state)
}

#[tokio::test]
async fn test_home_page() {
    let response = app("http://localhost")
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("sdict"));
    assert!(html.contains(r#"action="/search""#));
    assert!(html.contains(r#"name="term""#));
}

#[tokio::test]
async fn test_search_redirect() {
    let response = app("http://localhost")
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/search")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("term=hola"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "/translate/hola"
    );
}

#[tokio::test]
async fn test_search_empty_redirects_home() {
    let response = app("http://localhost")
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/search")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("term="))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "/"
    );
}

#[tokio::test]
async fn test_search_encodes_spaces() {
    let response = app("http://localhost")
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/search")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("term=buenos+dias"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(location.starts_with("/translate/buenos"));
}

#[tokio::test]
async fn test_translate_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/translate/comer"))
        .respond_with(ResponseTemplate::new(200).set_body_string(load_fixture("comer.html")))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/examples/comer"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(load_fixture("comer_examples.html")),
        )
        .mount(&mock_server)
        .await;

    let response = app(&mock_server.uri())
        .oneshot(
            Request::builder()
                .uri("/translate/comer")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("comer"));
    assert!(html.contains("to eat"));
    // Should have filter tags
    assert!(html.contains("filter-tag"));
    assert!(html.contains("Examples"));
}

#[tokio::test]
async fn test_translate_with_filter() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/translate/comer"))
        .respond_with(ResponseTemplate::new(200).set_body_string(load_fixture("comer.html")))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/examples/comer"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(load_fixture("comer_examples.html")),
        )
        .mount(&mock_server)
        .await;

    let response = app(&mock_server.uri())
        .oneshot(
            Request::builder()
                .uri("/translate/comer?filter=eat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    // The active filter tag should be highlighted
    assert!(html.contains(r#"data-state="active""#));
}

#[tokio::test]
async fn test_translate_not_found() {
    let mock_server = MockServer::start().await;

    let empty_html = r#"<html><body><script>window.SD_COMPONENT_DATA = {"sdDictionaryResultsProps":{"entry":{"neodict":[]}},"resultCardHeaderProps":{}};</script></body></html>"#;

    Mock::given(method("GET"))
        .and(path("/translate/xyznotaword"))
        .respond_with(ResponseTemplate::new(200).set_body_string(empty_html))
        .mount(&mock_server)
        .await;

    let response = app(&mock_server.uri())
        .oneshot(
            Request::builder()
                .uri("/translate/xyznotaword")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("No results for"));
}

#[tokio::test]
async fn test_translate_fetch_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/translate/broken"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let response = app(&mock_server.uri())
        .oneshot(
            Request::builder()
                .uri("/translate/broken")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Could not look up this term"));
}

#[tokio::test]
async fn test_not_found() {
    let response = app("http://localhost")
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Page not found"));
    assert!(html.contains(r#"action="/search""#));
}
