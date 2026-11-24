//! Staff sign-in: email + password + TOTP, all three required (TRD §7.4). The session token is
//! set as an `HttpOnly` cookie (see the module docs of `ops`), never returned to page scripts.

use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
};
use fuber_common::{
    crypto::{random_token, totp},
    validate::Email,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{Staff, csrf_token, session_cookie, session_hash};
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

pub const STAFF_SESSION_TTL: Duration = Duration::hours(8);
pub const TOTP_AAD: &str = "staff_users.totp_secret";

/// Verified against when the email is unknown, so a wrong email takes as long as a wrong password.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Ou2bE3qXvnTe7K6tQ4u7mCq4rM+PZlMG4bxfMHMp1yQ";

/// id, password hash, sealed TOTP secret, last TOTP step used, roles.
type StaffRow = (Uuid, String, Option<Vec<u8>>, Option<i64>, Vec<String>);

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Login {
    #[schema(value_type = String)]
    pub email: Email,
    #[schema(value_type = String, format = Password)]
    pub password: Password,
    /// 6-digit code from the authenticator app.
    #[schema(value_type = String, example = "123456")]
    pub totp: String,
}

/// 1–200 characters; never logged (no `Debug` output of the value).
#[derive(Deserialize)]
#[serde(try_from = "String")]
pub struct Password(String);

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(***)")
    }
}

impl TryFrom<String> for Password {
    type Error = &'static str;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        if (1..=200).contains(&s.chars().count()) { Ok(Password(s)) } else { Err("password must be 1–200 characters") }
    }
}

/// The signed-in session. The token itself is in the `vara_ops` cookie.
#[derive(Debug, Serialize, ToSchema)]
pub struct StaffSession {
    /// Send as `X-CSRF-Token` with every request that changes something.
    pub csrf_token: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub roles: Vec<String>,
}

#[utoipa::path(
    post, path = "/v1/ops/auth/login", tag = "ops",
    request_body = Login,
    responses(
        (status = 200, description = "Signed in; the session is in the `vara_ops` cookie (8 hours)", body = StaffSession),
        (status = 401, description = "Email, password or code is wrong (never says which)", body = ErrorBody),
        (status = 429, description = "5 attempts per 15 minutes per account and IP, 20 per hour per account", body = ErrorBody),
    )
)]
pub async fn login(
    State(s): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidJson(req): ValidJson<Login>,
) -> Result<([(header::HeaderName, HeaderValue); 1], Json<StaffSession>), ApiError> {
    let email_key: String =
        s.keys.hasher.hash("staff-email", req.email.as_str().as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect();
    s.limiter.check("ops_login", &format!("{email_key}:{ip}"), s.limits.ops_login).await?;
    // Also per account, whatever the address. A real staff member locked out by someone else's
    // guessing waits an hour at most; an admin can see the failed attempts in the audit log.
    s.limiter.check("ops_login_account", &email_key, s.limits.ops_login_account).await?;

    let row: Option<StaffRow> = sqlx::query_as(
        "SELECT id, password_hash, totp_secret, totp_last_step, roles::text[] FROM staff_users WHERE email = $1 AND disabled_at IS NULL",
    )
    .bind(req.email.as_str())
    .fetch_optional(&s.db)
    .await?;

    let hash = row.as_ref().map_or(DUMMY_HASH, |r| r.1.as_str()).to_owned();
    let password = req.password.0.clone();
    let password_ok = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
    })
    .await
    .map_err(ApiError::internal)?;

    let step = match (&row, password_ok) {
        (Some((_, _, Some(sealed), last, _)), true) => {
            let secret = s.keys.sealer.open(TOTP_AAD, sealed).map_err(ApiError::internal)?;
            totp::verify(&secret, req.totp.trim(), OffsetDateTime::now_utc().unix_timestamp()).filter(|st| last.is_none_or(|l| *st > l))
        }
        _ => None,
    };
    let Some(((staff_id, _, _, _, roles), step)) = row.zip(step) else {
        audit::record(
            &s.db,
            audit::Entry {
                staff_id: None,
                user_id: None,
                action: "staff.login_failed",
                target_type: "staff_email",
                target_id: email_key,
                detail: None,
                ip: Some(ip),
            },
        )
        .await?;
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "login_failed", "Email, password or code is wrong."));
    };

    let token = random_token();
    let expires_at = OffsetDateTime::now_utc() + STAFF_SESSION_TTL;
    let mut tx = s.db.begin().await?;
    // The step check above is repeated here atomically, so two logins racing with one code can't both win.
    let updated = sqlx::query(
        "UPDATE staff_users SET totp_last_step = $2, last_login_at = now() WHERE id = $1 AND (totp_last_step IS NULL OR totp_last_step < $2)",
    )
    .bind(staff_id)
    .bind(step)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "login_failed", "Email, password or code is wrong."));
    }
    sqlx::query("INSERT INTO staff_sessions (staff_id, token_hash, ip, expires_at) VALUES ($1, $2, $3::inet, $4)")
        .bind(staff_id)
        .bind(session_hash(&s, &token))
        .bind(ip.to_string())
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff_id),
            user_id: None,
            action: "staff.login",
            target_type: "staff",
            target_id: staff_id.to_string(),
            detail: Some(json!({ "roles": roles })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    let session = StaffSession { csrf_token: csrf_token(&s, &token), expires_at, roles };
    Ok(([(header::SET_COOKIE, session_cookie(&s, Some(&token)))], Json(session)))
}

#[utoipa::path(
    get, path = "/v1/ops/auth/session", operation_id = "ops_session", tag = "ops", security(("staff" = [])),
    responses(
        (status = 200, description = "Still signed in (the console calls this after a reload)", body = StaffSession),
        (status = 401, description = "No session, or it expired or was revoked", body = ErrorBody),
    )
)]
pub async fn session(State(s): State<AppState>, staff: Staff) -> Result<Json<StaffSession>, ApiError> {
    let expires_at: OffsetDateTime =
        sqlx::query_scalar("SELECT expires_at FROM staff_sessions WHERE id = $1").bind(staff.session_id).fetch_one(&s.db).await?;
    Ok(Json(StaffSession { csrf_token: staff.csrf_token, expires_at, roles: staff.roles }))
}

#[utoipa::path(post, path = "/v1/ops/auth/logout", operation_id = "ops_logout", tag = "ops", security(("staff" = [])), responses((status = 204)))]
pub async fn logout(State(s): State<AppState>, staff: Staff) -> Result<([(header::HeaderName, HeaderValue); 1], StatusCode), ApiError> {
    sqlx::query("UPDATE staff_sessions SET revoked_at = now() WHERE id = $1").bind(staff.session_id).execute(&s.db).await?;
    Ok(([(header::SET_COOKIE, session_cookie(&s, None))], StatusCode::NO_CONTENT))
}
