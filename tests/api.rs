// The integration tests drive the production router. The server modules are
// included from src/ so there is exactly one router stack — the same code
// `main` serves. (main.rs itself is not included: a binary entry point cannot
// be linked into a test crate; config.rs is covered by its in-module tests.)
#[path = "../src/api.rs"]
mod api;
#[path = "../src/error.rs"]
mod error;
#[path = "../src/pages.rs"]
mod pages;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tower::ServiceExt;

fn router() -> axum::Router {
    api::build_router()
}

async fn get(uri: &str) -> axum::http::Response<Body> {
    router()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_string(response: axum::http::Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn b64(path: &str) -> String {
    URL_SAFE_NO_PAD.encode(path.as_bytes())
}

#[tokio::test]
async fn file_lookup_returns_library_test_vector() {
    let response = get(&format!("/api/icon/file?path={}", b64("src/main.rs"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(body_string(response).await, r#"{"icon_id":525}"#);
}

#[tokio::test]
async fn folder_lookup_returns_library_test_vector() {
    let response = get(&format!("/api/icon/folder?path={}", b64(".github"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, r#"{"icon_id":862}"#);
}

#[tokio::test]
async fn unknown_extension_is_a_json_404() {
    let response = get(&format!("/api/icon/file?path={}", b64("foo.unknownextqq"))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(body_string(response).await, r#"{"error":"no icon found"}"#);
}

#[tokio::test]
async fn extensionless_unknown_path_is_a_404_not_a_panic() {
    // Panics inside the library; must surface as a clean 404 through the
    // full stack.
    let response = get(&format!("/api/icon/file?path={}", b64("LICENSE"))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_string(response).await, r#"{"error":"no icon found"}"#);

    let response = get(&format!("/api/icon/file?path={}", b64("foo/.."))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_string(response).await, r#"{"error":"no icon found"}"#);

    let response = get(&format!("/api/icon/folder?path={}", b64("foo/.."))).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_string(response).await, r#"{"error":"no icon found"}"#);
}

#[tokio::test]
async fn missing_path_is_a_json_400() {
    let response = get("/api/icon/file").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        body_string(response).await,
        r#"{"error":"missing required query parameter 'path'"}"#
    );

    let response = get("/api/icon/folder?path=").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn health_returns_ok() {
    let response = get("/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, r#"{"status":"ok"}"#);
}

#[tokio::test]
async fn index_page_has_form_and_icon_grid() {
    let response = get("/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let html = body_string(response).await;
    assert!(html.contains(r#"<form method="get" action="/">"#));
    let img_count = html.matches(r#"<img src="/icons/"#).count();
    assert!(
        img_count > 100,
        "expected a full icon grid, got {img_count} imgs"
    );
}

#[tokio::test]
async fn index_page_lookup_shows_matched_icon() {
    let response = get("/?path=src%2Fmain.rs&kind=file").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_string(response).await;
    assert!(
        html.contains(r#"<img src="/icons/525.svg""#),
        "icon not rendered inline"
    );
    assert!(html.contains("525"));
}

#[tokio::test]
async fn index_page_miss_renders_message_not_404() {
    let response = get("/?path=foo.unknownextqq&kind=file").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_string(response).await.contains("No icon found"));
}

#[tokio::test]
async fn svg_asset_is_served_with_svg_content_type() {
    let response = get("/icons/0.svg").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );
    let body = body_string(response).await;
    assert!(body.contains("<svg"));
}

#[tokio::test]
async fn unknown_svg_is_404() {
    let response = get("/icons/999999.svg").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_to_lookup_endpoints_is_method_not_allowed() {
    let response = router()
        .oneshot(
            Request::post(format!("/api/icon/file?path={}", b64("src/main.rs")))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn lookup_accepts_standard_base64_alphabet() {
    // URL-safe is what the helper produces; the standard (non-URL-safe)
    // alphabet with padding must work too.
    let standard = base64::engine::general_purpose::STANDARD.encode("src/main.rs");
    let response = get(&format!("/api/icon/file?path={standard}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, r#"{"icon_id":525}"#);
}

#[tokio::test]
async fn base64_path_carries_url_hostile_characters() {
    // `/`, spaces, `#`, `?` — all hostile in a query string — pass through
    // untouched via base64. The extension `rs` still resolves.
    let response = get(&format!("/api/icon/file?path={}", b64("weird name #?.rs"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, r#"{"icon_id":525}"#);
}

#[tokio::test]
async fn invalid_base64_is_a_json_400() {
    let response = get("/api/icon/file?path=%3F%3Fnotbase64%3F%3F").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        body_string(response).await,
        r#"{"error":"invalid base64 in query parameter 'path'"}"#
    );
}

#[tokio::test]
async fn plain_text_path_is_rejected_as_invalid_base64() {
    // The old plain-path contract is gone: a non-base64 path value is a 400,
    // not a lookup.
    let response = get("/api/icon/file?path=src%2Fmain.rs").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_string(response).await,
        r#"{"error":"invalid base64 in query parameter 'path'"}"#
    );
}

#[tokio::test]
async fn index_page_result_shows_base64_api_url() {
    let response = get("/?path=src%2Fmain.rs&kind=file").await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_string(response).await;
    assert!(
        html.contains("path=c3JjL21haW4ucnM"),
        "base64 API URL not rendered"
    );
}

#[tokio::test]
async fn responses_carry_permissive_cors_headers() {
    // Any origin may call the API (public, unauthenticated, read-only), so
    // every response — success or error — carries the wildcard allow-origin.
    let response = get("/health").await;
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    let response = get(&format!("/api/icon/file?path={}", b64("src/main.rs"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    // Error responses need the headers too, or a browser client cannot even
    // read the error body.
    let response = get("/api/icon/file").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    let response = get("/icons/999999.svg").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
}

#[tokio::test]
async fn preflight_options_is_answered_by_the_cors_layer() {
    // Browsers send OPTIONS before cross-origin GETs with non-simple headers.
    // `CorsLayer::permissive` answers with the wildcard for methods and
    // headers, which allows any client.
    let response = router()
        .oneshot(
            Request::options("/api/icon/file")
                .header(header::ORIGIN, "https://example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
        "*"
    );
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "*"
    );
}
