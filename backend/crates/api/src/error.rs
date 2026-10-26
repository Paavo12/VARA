//! The one error shape every endpoint returns: `{"error": {"code": "...", "message": "..."}}`.
//! Codes are stable and machine-readable; messages are safe to show and never contain stack
//! traces, SQL or internal identifiers.

use std::{borrow::Cow, time::Duration};

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: Cow<'static, str>,
    retry_after: Option<Duration>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        ApiError { status, code, message: message.into(), retry_after: None }
    }

    pub fn invalid(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found")
    }

    pub fn rate_limited(retry_after: Duration) -> Self {
        let mut e = Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many requests. Please wait and try again.");
        e.retry_after = Some(retry_after);
        e
    }

    /// Logs the real cause server-side and returns a generic 500 to the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "internal error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong. Please try again.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "code": self.code, "message": self.message } }));
        let mut res = (self.status, body).into_response();
        if let Some(d) = self.retry_after {
            // Round up so clients never retry early.
            let secs = d.as_millis().div_ceil(1000).max(1);
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(secs as u64));
        }
        res
    }
}

/// Documentation shape of every error response.
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ErrorDetail {
    /// Stable machine-readable code, e.g. `invalid_request`, `rate_limited`.
    pub code: String,
    /// Safe to show to the user.
    pub message: String,
}

pub async fn fallback() -> ApiError {
    ApiError::not_found()
}
