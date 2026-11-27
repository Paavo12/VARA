//! Ops API for staff (PRD O-1, consumed by the ops console in Phase 5): staff sign-in,
//! identity-verification review, and driver application review (documents, vehicle, tiers,
//! decision). Every action and every view of an identity image is written to the audit log.
//!
//! Staff authenticate with email + password (argon2id) + TOTP. The opaque 8-hour session token
//! goes in an `HttpOnly`, `SameSite=Strict` cookie scoped to `/v1/ops`, so page scripts never see
//! it and a reload stays signed in. Every request that changes something must also send the
//! session's CSRF token in `X-CSRF-Token` (from sign-in or `GET /v1/ops/auth/session`). Scripts
//! may send the token as `Authorization: Bearer` instead; a browser never attaches that on its
//! own, so it needs no CSRF token. The console must be served from the same site as the API
//! (e.g. `ops.vara.example` and `api.vara.example`), or the browser won't send the cookie.

pub mod audit_log;
pub mod auth;
pub mod drivers;
pub mod incidents;
pub mod metrics;
pub mod pricing;
pub mod settings;
pub mod settlements;
pub mod support;
pub mod verifications;

use axum::{
    Router,
    extract::FromRequestParts,
    http::{HeaderValue, Method, header, request::Parts},
    routing::{get, post, put},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fuber_common::{Environment, crypto::ct_eq};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{AppState, auth::bearer_token, error::ApiError};

pub fn router(_: &AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/session", get(auth::session))
        .route("/verifications", get(verifications::queue))
        .route("/verifications/{id}", get(verifications::detail))
        .route("/verifications/{id}/decision", post(verifications::decide))
        .route("/incidents", get(incidents::queue))
        .route("/incidents/{id}/messages", get(incidents::messages))
        .route("/incidents/{id}/close", post(incidents::close))
        .route("/incidents/retention", get(incidents::retention_queue))
        .route("/incidents/{id}/retention", post(incidents::decide_retention))
        .route("/drivers", get(drivers::queue))
        .route("/drivers/{id}", get(drivers::detail))
        .route("/drivers/{id}/tiers", put(drivers::set_tiers))
        .route("/drivers/{id}/decision", post(drivers::decide))
        .route("/documents/{id}/decision", post(drivers::decide_document))
        .route("/vehicles/{id}/decision", post(drivers::decide_vehicle))
        .route("/settlements", get(settlements::queue))
        .route("/settlements/{id}/settle", post(settlements::settle))
        .route("/users", get(support::search))
        .route("/users/{id}", get(support::detail))
        .route("/users/{id}/suspend", post(support::suspend))
        .route("/users/{id}/reinstate", post(support::reinstate))
        .route("/users/{id}/priority", post(support::adjust_priority))
        .route("/users/{id}/map-expert", post(support::grant_map_expert))
        .route("/users/{id}/map-expert/end", post(support::end_map_expert))
        .route("/reports", get(support::reports))
        .route("/reports/{id}/status", post(support::set_report_status))
        .route("/settings", get(settings::list))
        .route("/settings/{key}", put(settings::set))
        .route("/metrics", get(metrics::metrics))
        .route("/pricing", get(pricing::list).post(pricing::add))
        .route("/pricing/launch-promo", get(pricing::launch_promo).put(pricing::set_launch_promo))
        .route("/surge-zones", get(pricing::zones))
        .route("/surge-zones/{id}", put(pricing::update_zone))
        .route("/audit", get(audit_log::list))
}

/// A signed-in staff member.
#[derive(Debug, Clone)]
pub struct Staff {
    pub id: Uuid,
    pub session_id: Uuid,
    pub roles: Vec<String>,
    /// The session's CSRF token, for the console to send back.
    pub csrf_token: String,
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

    /// Support: users, suspensions, priority points and reported issues. Support agents and admins.
    pub fn require_support(&self) -> Result<(), ApiError> {
        if self.roles.iter().any(|r| r == "ops_support" || r == "ops_admin") {
            Ok(())
        } else {
            Err(ApiError::forbidden("forbidden", "Your role can't do this."))
        }
    }

    /// Money: weekly settlements, pricing and Map Expert status. Admins only.
    pub fn require_admin(&self) -> Result<(), ApiError> {
        if self.roles.iter().any(|r| r == "ops_admin") { Ok(()) } else { Err(ApiError::forbidden("forbidden", "Your role can't do this.")) }
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
        let (token, from_cookie) = match bearer_token(parts) {
            Some(t) => (t, false),
            None => (cookie_token(parts).ok_or_else(ApiError::unauthorized)?, true),
        };
        if token.len() != 43 {
            return Err(ApiError::unauthorized());
        }
        let csrf_token = csrf_token(s, token);
        // A cookie rides along on any request the browser makes, so a change must prove it came
        // from the console: only the console can read the CSRF token.
        if from_cookie && !matches!(parts.method, Method::GET | Method::HEAD) {
            let sent = parts.headers.get(CSRF_HEADER).map(|v| v.as_bytes()).unwrap_or_default();
            if !ct_eq(sent, csrf_token.as_bytes()) {
                return Err(ApiError::forbidden("csrf_failed", "Your session check failed. Reload the page and try again."));
            }
        }
        let row: Option<(Uuid, Uuid, Vec<String>)> = sqlx::query_as(
            "SELECT st.id, ss.id, st.roles::text[] FROM staff_sessions ss JOIN staff_users st ON st.id = ss.staff_id
             WHERE ss.token_hash = $1 AND ss.revoked_at IS NULL AND ss.expires_at > now() AND st.disabled_at IS NULL",
        )
        .bind(session_hash(s, token))
        .fetch_optional(&s.db)
        .await?;
        let (id, session_id, roles) = row.ok_or_else(ApiError::unauthorized)?;
        s.limiter.check("ops", &id.to_string(), s.limits.ops).await?;
        Ok(Staff { id, session_id, roles, csrf_token })
    }
}

/// The staff session cookie. `HttpOnly`: page scripts never see the token.
pub const COOKIE: &str = "vara_ops";
pub const CSRF_HEADER: &str = "x-csrf-token";

/// Derived from the session token, so it needs no storage and dies with the session.
pub fn csrf_token(s: &AppState, token: &str) -> String {
    URL_SAFE_NO_PAD.encode(s.keys.hasher.hash("staff-csrf", token.as_bytes()))
}

/// A short-lived link for a reviewer to view a stored file.
pub async fn presign_get(s: &AppState, key: Option<String>, expires_at: i64) -> Result<Option<String>, ApiError> {
    match key {
        Some(k) => s.store.presign_get(&k, expires_at).await.map(Some).map_err(ApiError::internal),
        None => Ok(None),
    }
}

fn cookie_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

/// `Set-Cookie` for a new session, or (with `None`) one that clears it.
pub fn session_cookie(s: &AppState, token: Option<&str>) -> HeaderValue {
    let (value, max_age) = match token {
        Some(t) => (t, auth::STAFF_SESSION_TTL.whole_seconds()),
        None => ("", 0),
    };
    // Plain-HTTP local runs can't use Secure cookies on every browser.
    let secure = if s.config.env == Environment::Local { "" } else { "; Secure" };
    HeaderValue::from_str(&format!("{COOKIE}={value}; Path=/v1/ops; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"))
        .expect("token is base64url")
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
