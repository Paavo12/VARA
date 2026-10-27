//! Rider and driver authentication (TRD §7.4).
//!
//! - Access token: Ed25519 JWT, 15 minutes, claims `sub`, `role`, `sid`. Every request also
//!   checks that the session row is live, so sign-out, suspension and account deletion take
//!   effect immediately rather than when the token expires.
//! - Refresh token: 256-bit random, stored as an HMAC, 30 days, rotated on every use. Each
//!   rotation revokes the old row (`rotated`) and inserts a new one in the same family; if a
//!   rotated token is ever presented again, the whole family is revoked (token theft).
//! - Drivers may be signed in on one device at a time (demo rule): a new driver sign-in
//!   revokes the previous driver session.

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use fuber_common::{
    Config,
    crypto::{HasExpiry, Hasher, Sealer, TokenSigner, random_token},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{AppState, error::ApiError};

pub const ACCESS_TTL: Duration = Duration::minutes(15);
pub const REFRESH_TTL: Duration = Duration::days(30);
const ISSUER: &str = "fuber-api";

/// Server-side keys, derived once at start-up from the configured secrets.
pub struct Keys {
    pub hasher: Hasher,
    pub sealer: Sealer,
    pub tokens: TokenSigner,
}

impl Keys {
    pub fn from_config(c: &Config) -> Arc<Keys> {
        let previous: Vec<_> = c.jwt_previous.iter().cloned().collect();
        Arc::new(Keys {
            hasher: Hasher::new(&c.hash_secret),
            sealer: Sealer::new(&c.data_key),
            tokens: TokenSigner::new(&c.jwt_kid, &c.jwt_secret, &previous),
        })
    }
}

/// Which app a session belongs to. One person can hold both (PRD A-9), with separate sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum App {
    Rider,
    Driver,
}

impl App {
    pub fn as_str(self) -> &'static str {
        match self {
            App::Rider => "rider",
            App::Driver => "driver",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: Uuid,
    pub role: App,
    pub sid: Uuid,
    pub iat: i64,
    pub exp: i64,
}

impl HasExpiry for Claims {
    fn exp(&self) -> i64 {
        self.exp
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TokenPair {
    /// Send as `Authorization: Bearer <access_token>`. Valid for `expires_in` seconds.
    pub access_token: String,
    /// Single use: exchange at `POST /v1/auth/refresh` for a new pair. Store it only in the
    /// device keychain/keystore.
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
}

pub fn refresh_hash(keys: &Keys, token: &str) -> Vec<u8> {
    keys.hasher.hash("refresh", token.as_bytes()).to_vec()
}

pub struct NewSession<'a> {
    pub user_id: Uuid,
    pub app: App,
    /// `None` starts a new family (a fresh sign-in).
    pub family_id: Option<Uuid>,
    pub device_name: Option<&'a str>,
    pub app_version: Option<&'a str>,
}

/// Inserts a session row and returns its tokens. Runs inside the caller's transaction.
pub async fn issue_session(tx: &mut Transaction<'_, Postgres>, keys: &Keys, s: NewSession<'_>) -> Result<TokenPair, ApiError> {
    if s.app == App::Driver && s.family_id.is_none() {
        sqlx::query(
            "UPDATE sessions SET revoked_at = now(), revoked_reason = 'signed_in_elsewhere'
             WHERE user_id = $1 AND app = 'driver' AND revoked_at IS NULL",
        )
        .bind(s.user_id)
        .execute(&mut **tx)
        .await?;
    }
    let refresh = random_token();
    let now = OffsetDateTime::now_utc();
    let sid: Uuid = sqlx::query_scalar(
        "INSERT INTO sessions (user_id, app, family_id, refresh_hash, device_name, app_version, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(s.user_id)
    .bind(s.app.as_str())
    .bind(s.family_id.unwrap_or_else(Uuid::new_v4))
    .bind(refresh_hash(keys, &refresh))
    .bind(s.device_name)
    .bind(s.app_version)
    .bind(now + REFRESH_TTL)
    .fetch_one(&mut **tx)
    .await?;
    let iat = now.unix_timestamp();
    let claims = Claims { iss: ISSUER.into(), sub: s.user_id, role: s.app, sid, iat, exp: iat + ACCESS_TTL.whole_seconds() };
    Ok(TokenPair {
        access_token: keys.tokens.sign(&claims),
        refresh_token: refresh,
        token_type: "Bearer",
        expires_in: ACCESS_TTL.whole_seconds(),
    })
}

/// A signed-in rider or driver with a live session.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub app: App,
    pub session_id: Uuid,
}

fn bearer(parts: &Parts) -> Option<&str> {
    let v = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let token = bearer(parts).ok_or_else(ApiError::unauthorized)?;
        let claims: Claims =
            s.keys.tokens.verify(token, OffsetDateTime::now_utc().unix_timestamp()).map_err(|_| ApiError::unauthorized())?;
        if claims.iss != ISSUER {
            return Err(ApiError::unauthorized());
        }
        let status: Option<String> = sqlx::query_scalar(
            "SELECT u.status::text FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.id = $1 AND s.user_id = $2 AND s.app = $3 AND s.revoked_at IS NULL AND s.expires_at > now()",
        )
        .bind(claims.sid)
        .bind(claims.sub)
        .bind(claims.role.as_str())
        .fetch_optional(&s.db)
        .await?;
        match status.as_deref() {
            None | Some("deleted") => return Err(ApiError::unauthorized()),
            Some("suspended") => {
                return Err(ApiError::forbidden("account_suspended", "Your account is suspended. Please contact FUber support."));
            }
            Some(_) => {}
        }
        s.limiter.check("user", &claims.sub.to_string(), s.limits.user).await?;
        Ok(AuthUser { user_id: claims.sub, app: claims.role, session_id: claims.sid })
    }
}

/// A signed-in user of the rider app.
pub struct RiderUser(pub AuthUser);

impl FromRequestParts<AppState> for RiderUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let u = AuthUser::from_request_parts(parts, s).await?;
        if u.app != App::Rider {
            return Err(ApiError::forbidden("wrong_app", "This is only available in the FUber rider app."));
        }
        Ok(RiderUser(u))
    }
}

/// A signed-in user of the driver app.
pub struct DriverUser(pub AuthUser);

impl FromRequestParts<AppState> for DriverUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, ApiError> {
        let u = AuthUser::from_request_parts(parts, s).await?;
        if u.app != App::Driver {
            return Err(ApiError::forbidden("wrong_app", "This is only available in the FUber driver app."));
        }
        Ok(DriverUser(u))
    }
}

pub(crate) fn bearer_token(parts: &Parts) -> Option<&str> {
    bearer(parts)
}
