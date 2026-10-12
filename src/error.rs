use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("missing required query parameter 'path'")]
    MissingPath,
    #[error("invalid base64 in query parameter 'path'")]
    InvalidBase64,
    #[error("no icon found")]
    NoIcon,
    #[error("internal server error")]
    Internal,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    error: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::MissingPath | ApiError::InvalidBase64 => StatusCode::BAD_REQUEST,
            ApiError::NoIcon => StatusCode::NOT_FOUND,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ErrorResponse {
                error: self.to_string().as_str(),
            }),
        )
            .into_response()
    }
}

/// Response used by the panic backstop middleware: any unexpected panic that
/// escapes a handler is turned into a 500 with a JSON body, never an empty
/// or HTML error page.
pub fn internal_server_error_response() -> Response {
    ApiError::Internal.into_response()
}
