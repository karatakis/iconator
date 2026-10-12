use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, rejection::QueryRejection};
use axum::response::Json;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::error::{ApiError, internal_server_error_response};

/// Which lookup function a request wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Folder,
}

impl Kind {
    pub fn from_query(raw: Option<&str>) -> Self {
        match raw {
            Some("folder") => Kind::Folder,
            _ => Kind::File,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LookupParams {
    /// Base64-encoded file or folder path (standard or URL-safe alphabet,
    /// padding optional). Base64 keeps values free of characters that are
    /// hostile in URLs (`/`, `#`, `?`, spaces, non-ASCII).
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct IconResponse {
    pub icon_id: u64,
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'a str,
}

/// Look up the icon ID for `path`, converting library panics into
/// `ApiError::NoIcon` instead of crashing the task.
///
/// `iconator::get_icon_for_file` panics (rather than returning `None`) for
/// paths without a file name and for unknown extensionless filenames; a
/// caught panic is semantically a lookup miss, so it maps to 404.
pub fn lookup(kind: Kind, path: &str) -> Result<u64, ApiError> {
    let result = catch_unwind(AssertUnwindSafe(|| match kind {
        Kind::File => iconator::get_icon_for_file(path),
        Kind::Folder => iconator::get_icon_for_folder(path),
    }));

    match result {
        Ok(Some(icon_id)) => Ok(icon_id),
        Ok(None) => {
            log::warn!(target: "iconator::lookup", "no icon found for {kind:?} path {path:?}");
            Err(ApiError::NoIcon)
        }
        Err(panic) => {
            log::warn!(target: "iconator::lookup",
                "lookup panicked for {kind:?} path {path:?}: {panic:?}; treating as no icon found");
            Err(ApiError::NoIcon)
        }
    }
}

fn non_empty_path(params: Result<Query<LookupParams>, QueryRejection>) -> Result<String, ApiError> {
    let query = params.map_err(|_| ApiError::MissingPath)?.0;

    let encoded = query
        .path
        .filter(|path| !path.is_empty())
        .ok_or(ApiError::MissingPath)?;
    let path = decode_path(&encoded)?;

    if path.is_empty() {
        return Err(ApiError::MissingPath);
    }
    Ok(path)
}

/// Decode the base64-encoded `path` value. Accepts the standard and URL-safe
/// base64 alphabets, with or without padding, so clients can use whichever
/// their platform produces. ASCII whitespace is ignored (copy-paste
/// artifacts). Invalid base64 or non-UTF-8 bytes are client errors (400).
fn decode_path(encoded: &str) -> Result<String, ApiError> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let normalized: String = encoded
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '=')
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();

    URL_SAFE_NO_PAD
        .decode(normalized.as_bytes())
        .map_err(|err| {
            log::warn!(target: "iconator::lookup", "invalid base64 path {encoded:?}: {err}");
            ApiError::InvalidBase64
        })
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|err| {
                log::warn!(target: "iconator::lookup",
                    "base64 path {encoded:?} is not valid UTF-8: {err}");
                ApiError::InvalidBase64
            })
        })
}

async fn icon_file(
    params: Result<Query<LookupParams>, QueryRejection>,
) -> Result<Json<IconResponse>, ApiError> {
    let path = non_empty_path(params)?;
    let icon_id = lookup(Kind::File, &path)?;
    Ok(Json(IconResponse { icon_id }))
}

async fn icon_folder(
    params: Result<Query<LookupParams>, QueryRejection>,
) -> Result<Json<IconResponse>, ApiError> {
    let path = non_empty_path(params)?;
    let icon_id = lookup(Kind::Folder, &path)?;
    Ok(Json(IconResponse { icon_id }))
}

async fn health() -> Json<HealthResponse<'static>> {
    Json(HealthResponse { status: "ok" })
}

/// The production router: same stack `main` serves and integration tests drive.
///
/// Layer order (outermost first): CORS -> Timeout(10s) -> Trace -> CatchPanic.
/// CORS is outermost so every response — including timeout and error bodies —
/// carries the access-control headers a browser client needs. It is
/// permissive (any origin/method/headers) because this is a public, read-only,
/// unauthenticated API. A panic that escapes a handler (none should — the
/// lookup wrappers catch the known library panics) still yields a JSON 500,
/// never a dropped connection.
pub fn build_router() -> Router {
    Router::new()
        .route("/api/icon/file", get(icon_file))
        .route("/api/icon/folder", get(icon_folder))
        .route("/health", get(health))
        .route("/", get(crate::pages::index))
        .nest_service("/icons", ServeDir::new("icons"))
        .layer(CatchPanicLayer::custom(|_panic| {
            internal_server_error_response()
        }))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .layer(CorsLayer::permissive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn library_test_vectors_pass_through() {
        assert_eq!(lookup(Kind::File, "src/main.rs").unwrap(), 525);
        assert_eq!(lookup(Kind::Folder, ".github").unwrap(), 862);
    }

    #[test]
    fn extensionless_unknown_filename_is_a_miss_not_a_panic() {
        // `LICENSE` is not in the filename table and has no extension, which
        // makes the library unwrap panic; it must surface as NoIcon (404).
        assert!(matches!(
            lookup(Kind::File, "LICENSE"),
            Err(ApiError::NoIcon)
        ));
    }

    #[test]
    fn path_without_file_name_is_a_miss_not_a_panic() {
        assert!(matches!(
            lookup(Kind::File, "foo/.."),
            Err(ApiError::NoIcon)
        ));
        assert!(matches!(
            lookup(Kind::Folder, "foo/.."),
            Err(ApiError::NoIcon)
        ));
    }

    #[test]
    fn unknown_folder_is_a_miss() {
        assert!(matches!(
            lookup(Kind::Folder, "definitely-not-a-known-folder"),
            Err(ApiError::NoIcon)
        ));
    }

    #[test]
    fn missing_path_params_map_to_missing_path_error() {
        let params: Result<Query<LookupParams>, QueryRejection> =
            Ok(Query(LookupParams { path: None }));
        assert!(matches!(non_empty_path(params), Err(ApiError::MissingPath)));

        let params: Result<Query<LookupParams>, QueryRejection> = Ok(Query(LookupParams {
            path: Some(String::new()),
        }));
        assert!(matches!(non_empty_path(params), Err(ApiError::MissingPath)));

        // Base64 of an empty string, e.g. sent by a client that always sets
        // the parameter.
        let params: Result<Query<LookupParams>, QueryRejection> = Ok(Query(LookupParams {
            path: Some("====".to_string()),
        }));
        assert!(matches!(non_empty_path(params), Err(ApiError::MissingPath)));
    }

    #[test]
    fn path_decodes_all_common_base64_variants() {
        // URL-safe, no padding / URL-safe, padded / standard, no padding /
        // standard, padded / with stray whitespace.
        for encoded in [
            "c3JjL21haW4ucnM",
            "c3JjL21haW4ucnM=",
            "c3JjL21haW4ucnM",
            "c3JjL21haW4ucnM=",
            "c3Jj\n L21haW4ucnM=",
        ] {
            assert_eq!(decode_path(encoded).unwrap(), "src/main.rs");
        }
    }

    #[test]
    fn path_with_special_characters_decodes() {
        // "weird name #?.rs" — every character that is hostile in a query string.
        assert_eq!(
            decode_path("d2VpcmQgbmFtZSAjPy5ycw==").unwrap(),
            "weird name #?.rs"
        );
    }

    #[test]
    fn invalid_base64_path_is_a_client_error() {
        // A plain (non-base64) path no longer works — it is rejected.
        assert!(matches!(
            decode_path("src/main.rs"),
            Err(ApiError::InvalidBase64)
        ));
        assert!(matches!(
            decode_path("not valid base64!!!"),
            Err(ApiError::InvalidBase64)
        ));
        // Decodes to bytes, but not UTF-8.
        assert!(matches!(decode_path("/w=="), Err(ApiError::InvalidBase64)));
    }

    #[test]
    fn error_bodies_are_the_specified_json() {
        let body = ApiError::NoIcon.into_response();
        assert_eq!(body.status(), axum::http::StatusCode::NOT_FOUND);

        let body = ApiError::MissingPath.into_response();
        assert_eq!(body.status(), axum::http::StatusCode::BAD_REQUEST);

        let body = ApiError::InvalidBase64.into_response();
        assert_eq!(body.status(), axum::http::StatusCode::BAD_REQUEST);

        let body = ApiError::Internal.into_response();
        assert_eq!(body.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
