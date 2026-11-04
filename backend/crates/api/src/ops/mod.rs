//! Ops API for staff (PRD O-1, consumed by the ops console in Phase 5): staff sign-in,
//! identity-verification review, and driver application review (documents, vehicle, tiers,
//! decision). Every action and every view of an identity image is written to the audit log.
//!
//! Staff authenticate with email + password (argon2id) + TOTP and receive an opaque 8-hour
//! bearer token. Phase 5 moves the console to an `HttpOnly` cookie with CSRF protection.

pub mod auth;
pub mod drivers;
pub mod incidents;
pub mod verifications;

use axum::{
    Router,
    extract::FromRequestParts,
    http::request::Parts,
    routing::{get, post, put},
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{AppState, auth::bearer_token, error::ApiError};

pub fn router(_: &AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/verifications", get(verifications::queue))
        .route("/verifications/{id}", get(verifications::detail))
        .route("/verifications/{id}/decision", post(verifications::decide))
        .route("/incidents", get(incidents::queue))
        .route("/incidents/{id}/close", post(incidents::close))
        .route("/drivers", get(drivers::queue))
        .route("/drivers/{id}", get(drivers::detail))
        .route("/drivers/{id}/tiers", put(drivers::set_tiers))
        .route("/drivers/{id}/decision", post(drivers::decide))
        .route("/documents/{id}/decision", post(drivers::decide_document))
        .route("/vehicles/{id}/decision", post(drivers::decide_vehicle))
}

/// A signed-in staff member.
#[derive(Debug, Clone)]
pub struct Staff {
    pub id: Uuid,
    pub session_id: Uuid,
    pub roles: Vec<String>,
}

impl Staff {
    /// Verification and driver reviews: reviewers and admins.
    pub fn require_reviewer(&self) -> Result<(), ApiError> {
        if self.roles.iter().any(|r| r == "ops_reviewer" || r == "ops_admin") {
            Ok(())
        } else {
            Err(ApiError::forbidden("forbidden", "Your role can't do this."))
        }
    }

    /// The safety desk: safety agents and admins.
    pub fn require_safety(&self) -> Result<(), ApiError> {
        if self.roles.iter().any(|r| r == "ops_safety" || r == "ops_admin") {
            Ok(())
        } else {
            Err(ApiError::forbidden("forbidden", "Your role can't do this."))
        }
    }
}

pub fn session_hash(s: &AppState, token: &str) -> Vec<u8> {
    s.keys.hasher.hash("staff-session", token.as_bytes()).to_vec()
}

impl FromRequestParts<AppState> for Staff {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let token = bearer_token(parts).filter(|t| t.len() == 43).ok_or_else(ApiError::unauthorized)?;
        let row: Option<(Uuid, Uuid, Vec<String>)> = sqlx::query_as(
            "SELECT st.id, ss.id, st.roles::text[] FROM staff_sessions ss JOIN staff_users st ON st.id = ss.staff_id
             WHERE ss.token_hash = $1 AND ss.revoked_at IS NULL AND ss.expires_at > now() AND st.disabled_at IS NULL",
        )
        .bind(session_hash(s, token))
        .fetch_optional(&s.db)
        .await?;
        let (id, session_id, roles) = row.ok_or_else(ApiError::unauthorized)?;
        s.limiter.check("ops", &id.to_string(), s.limits.ops).await?;
        Ok(Staff { id, session_id, roles })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Approve,
    Reject,
}

/// A review decision. A rejection needs a reason, which the applicant sees.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub decision: Verdict,
    #[schema(value_type = Option<String>, example = "The ID photo is blurred. Please retake it in good light.")]
    pub reason: Option<fuber_common::validate::PlainText<300>>,
}

impl Decision {
    pub fn reason(&self) -> Result<Option<&str>, ApiError> {
        match (self.decision, &self.reason) {
            (Verdict::Reject, None) => Err(ApiError::invalid("a rejection needs a reason the applicant can act on")),
            (_, r) => Ok(r.as_ref().map(|r| r.as_str())),
        }
    }
}
