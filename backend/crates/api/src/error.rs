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

    /// Missing, malformed, expired or revoked credentials. Deliberately says nothing about which.
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "Please sign in again.")
    }

    pub fn forbidden(code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    pub fn conflict(code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn unavailable(code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    pub fn rate_limited(retry_after: Duration) -> Self {
        let mut e = Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many requests. Please wait and try again.");
        e.retry_after = Some(retry_after);
        e
    }

    /// Every database connection stayed in use for the whole wait: overload, not a fault, so
    /// the client is told to try again.
    pub fn busy() -> Self {
        tracing::warn!("no database connection free in time");
        let mut e = Self::unavailable("busy", "We're very busy right now. Please try again.");
        e.retry_after = Some(Duration::from_secs(1));
        e
    }

    /// Logs the real cause server-side and returns a generic 500 to the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "internal error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong. Please try again.")
    }
}

/// Database errors are never shown to the client: `?` on a query logs it and answers 500, or
/// 503 when the pool had no connection free in time.
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => ApiError::busy(),
            e => ApiError::internal(e),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_connection_pool_is_a_503_to_retry_and_other_database_errors_stay_500() {
        let busy = ApiError::from(sqlx::Error::PoolTimedOut).into_response();
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(busy.headers()[header::RETRY_AFTER], "1");
        assert_eq!(ApiError::from(sqlx::Error::RowNotFound).status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
