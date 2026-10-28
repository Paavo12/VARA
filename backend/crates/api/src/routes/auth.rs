//! Phone + SMS one-time-code sign-in (PRD A-1, TRD §7.1 and §7.4).
//!
//! The OTP table never holds phone numbers or codes: the phone is stored as an HMAC and the
//! code as an HMAC bound to its challenge. Responses never reveal whether a number already
//! has an account.

use axum::{Json, extract::State, http::StatusCode};
use fuber_common::{
    crypto::{ct_eq, random_bytes},
    telemetry::mask_phone,
    validate::{Line, PhoneE164},
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser, NewSession, TokenPair, issue_session, refresh_hash},
    error::{ApiError, ErrorBody},
    extract::{AppVersion, ClientIp, ValidJson},
};

pub const OTP_TTL: Duration = Duration::minutes(5);
pub const OTP_MAX_ATTEMPTS: i16 = 5;
/// Gap before the first resend; doubles with each further code for the same number in an hour.
const RESEND_BASE_S: i64 = 30;

fn phone_hash(s: &AppState, phone: &PhoneE164) -> Vec<u8> {
    s.keys.hasher.hash("phone", phone.as_str().as_bytes()).to_vec()
}

fn code_hash(s: &AppState, challenge: Uuid, code: &str) -> Vec<u8> {
    s.keys.hasher.hash("otp", format!("{challenge}:{code}").as_bytes()).to_vec()
}

/// A uniformly random 6-digit code (rejection sampling avoids modulo bias).
fn new_code() -> String {
    loop {
        let n = u32::from_le_bytes(random_bytes::<4>());
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OtpRequest {
    /// Namibian mobile number; `081 123 4567` and `+264811234567` are both accepted.
    #[schema(value_type = String, example = "+264811234567")]
    pub phone: PhoneE164,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtpChallenge {
    pub challenge_id: Uuid,
    pub expires_in_s: i64,
    /// Seconds before another code can be requested for this number.
    pub resend_after_s: i64,
}

#[utoipa::path(
    post, path = "/v1/auth/otp/request", tag = "auth",
    request_body = OtpRequest,
    responses(
        (status = 202, description = "Code sent by SMS", body = OtpChallenge),
        (status = 422, description = "Not a valid Namibian mobile number", body = ErrorBody),
        (status = 429, description = "Too many codes for this number or IP; see Retry-After", body = ErrorBody),
        (status = 503, description = "SMS could not be sent", body = ErrorBody),
    )
)]
pub async fn request_otp(
    State(s): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidJson(req): ValidJson<OtpRequest>,
) -> Result<(StatusCode, Json<OtpChallenge>), ApiError> {
    let ph = phone_hash(&s, &req.phone);
    s.limiter.check("otp_phone", &hex(&ph), s.limits.otp_phone).await?;

    // Exponential cooldown between codes for the same number.
    let (recent, last): (i64, Option<OffsetDateTime>) = sqlx::query_as(
        "SELECT count(*), max(created_at) FROM otp_challenges WHERE phone_hash = $1 AND created_at > now() - interval '1 hour'",
    )
    .bind(&ph)
    .fetch_one(&s.db)
    .await?;
    let now = OffsetDateTime::now_utc();
    if let Some(last) = last {
        let wait = cooldown_s(recent) - (now - last).whole_seconds();
        if wait > 0 {
            return Err(ApiError::rate_limited(std::time::Duration::from_secs(wait as u64)));
        }
    }

    let id = Uuid::new_v4();
    let code = new_code();
    sqlx::query("INSERT INTO otp_challenges (id, phone_hash, code_hash, expires_at, ip) VALUES ($1, $2, $3, $4, $5::inet)")
        .bind(id)
        .bind(&ph)
        .bind(code_hash(&s, id, &code))
        .bind(now + OTP_TTL)
        .bind(ip.to_string())
        .execute(&s.db)
        .await?;

    let body = format!("{code} is your FUber code. It expires in 5 minutes. FUber will never ask you for this code.");
    if let Err(e) = s.sms.send(&req.phone, &body).await {
        tracing::error!(error = %e, to = %mask_phone(req.phone.as_str()), "OTP SMS failed");
        return Err(ApiError::unavailable("sms_unavailable", "We couldn't send the code. Please try again in a minute."));
    }
    let challenge = OtpChallenge { challenge_id: id, expires_in_s: OTP_TTL.whole_seconds(), resend_after_s: cooldown_s(recent + 1) };
    Ok((StatusCode::ACCEPTED, Json(challenge)))
}

/// 30 s after the first code, then 60, 120, 240 … capped at 15 minutes.
fn cooldown_s(codes_in_last_hour: i64) -> i64 {
    if codes_in_last_hour <= 0 { 0 } else { (RESEND_BASE_S << (codes_in_last_hour - 1).min(5)).min(900) }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OtpVerify {
    pub challenge_id: Uuid,
    /// The same number the code was sent to.
    #[schema(value_type = String, example = "+264811234567")]
    pub phone: PhoneE164,
    #[schema(value_type = String, example = "123456")]
    pub code: OtpCode,
    pub app: App,
    /// Shown in the user's list of signed-in devices, e.g. "Samsung Galaxy A15".
    #[schema(value_type = Option<String>)]
    pub device_name: Option<Line<80>>,
}

/// Exactly six ASCII digits.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct OtpCode(String);

impl TryFrom<String> for OtpCode {
    type Error = &'static str;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let s = s.trim().to_owned();
        if s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) { Ok(OtpCode(s)) } else { Err("code must be 6 digits") }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignedIn {
    #[serde(flatten)]
    pub tokens: TokenPair,
    pub user_id: Uuid,
    /// True when this sign-in created the account: the app continues to "Your details".
    pub new_user: bool,
}

#[utoipa::path(
    post, path = "/v1/auth/otp/verify", tag = "auth",
    request_body = OtpVerify,
    responses(
        (status = 200, description = "Signed in", body = SignedIn),
        (status = 400, description = "`otp_invalid` (wrong or expired) or `otp_locked` (5 wrong attempts)", body = ErrorBody),
        (status = 403, description = "`account_suspended` or `account_deleted`", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
    )
)]
pub async fn verify_otp(
    State(s): State<AppState>,
    AppVersion(app_version): AppVersion,
    ValidJson(req): ValidJson<OtpVerify>,
) -> Result<Json<SignedIn>, ApiError> {
    let invalid = || ApiError::new(StatusCode::BAD_REQUEST, "otp_invalid", "That code is wrong or has expired.");
    let mut tx = s.db.begin().await?;
    let row: Option<(Vec<u8>, Vec<u8>, i16, bool)> = sqlx::query_as(
        "SELECT phone_hash, code_hash, attempts, (consumed_at IS NULL AND expires_at > now())
         FROM otp_challenges WHERE id = $1 FOR UPDATE",
    )
    .bind(req.challenge_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((stored_phone, stored_code, attempts, usable)) = row else { return Err(invalid()) };
    if !usable {
        return Err(invalid());
    }
    if attempts >= OTP_MAX_ATTEMPTS {
        return Err(locked());
    }
    let phone_ok = ct_eq(&stored_phone, &phone_hash(&s, &req.phone));
    let code_ok = ct_eq(&stored_code, &code_hash(&s, req.challenge_id, &req.code.0));
    if !(phone_ok && code_ok) {
        sqlx::query("UPDATE otp_challenges SET attempts = attempts + 1 WHERE id = $1").bind(req.challenge_id).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::warn!(attempt = attempts + 1, "OTP verification failed");
        return Err(if attempts + 1 >= OTP_MAX_ATTEMPTS { locked() } else { invalid() });
    }
    sqlx::query("UPDATE otp_challenges SET consumed_at = now() WHERE id = $1").bind(req.challenge_id).execute(&mut *tx).await?;

    // Find or create the account. ON CONFLICT keeps two simultaneous first sign-ins safe.
    let inserted: Option<Uuid> =
        sqlx::query_scalar("INSERT INTO users (phone_e164) VALUES ($1) ON CONFLICT (phone_e164) DO NOTHING RETURNING id")
            .bind(req.phone.as_str())
            .fetch_optional(&mut *tx)
            .await?;
    let new_user = inserted.is_some();
    let (user_id, status): (Uuid, String) = sqlx::query_as("SELECT id, status::text FROM users WHERE phone_e164 = $1 FOR UPDATE")
        .bind(req.phone.as_str())
        .fetch_one(&mut *tx)
        .await?;
    match status.as_str() {
        "suspended" => return Err(ApiError::forbidden("account_suspended", "Your account is suspended. Please contact FUber support.")),
        "deleted" => {
            return Err(ApiError::forbidden(
                "account_deleted",
                "This account is being deleted. Contact FUber support within 30 days of deleting it if you want it back.",
            ));
        }
        _ => {}
    }
    if req.app == App::Rider {
        sqlx::query("INSERT INTO rider_profiles (user_id) VALUES ($1) ON CONFLICT DO NOTHING").bind(user_id).execute(&mut *tx).await?;
    }
    let tokens = issue_session(
        &mut tx,
        &s.keys,
        NewSession {
            user_id,
            app: req.app,
            family_id: None,
            device_name: req.device_name.as_ref().map(Line::as_str),
            app_version: app_version.as_deref(),
        },
    )
    .await?;
    tx.commit().await?;
    tracing::info!(%user_id, app = req.app.as_str(), new_user, "signed in");
    Ok(Json(SignedIn { tokens, user_id, new_user }))
}

fn locked() -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "otp_locked", "Too many wrong codes. Please request a new code.")
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    #[schema(value_type = String)]
    pub refresh_token: RefreshToken,
}

/// The 43-character base64url token issued at sign-in.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct RefreshToken(String);

impl TryFrom<String> for RefreshToken {
    type Error = &'static str;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        if s.len() == 43 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            Ok(RefreshToken(s))
        } else {
            Err("refresh_token is not valid")
        }
    }
}

#[utoipa::path(
    post, path = "/v1/auth/refresh", tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "New token pair; the old refresh token is now spent", body = TokenPair),
        (status = 401, description = "Unknown, expired, revoked or reused refresh token", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
    )
)]
pub async fn refresh(
    State(s): State<AppState>,
    AppVersion(app_version): AppVersion,
    ValidJson(req): ValidJson<RefreshRequest>,
) -> Result<Json<TokenPair>, ApiError> {
    let hash = refresh_hash(&s.keys, &req.refresh_token.0);
    let mut tx = s.db.begin().await?;
    type Row = (Uuid, Uuid, String, Uuid, Option<String>, Option<String>, bool, String);
    let row: Option<Row> = sqlx::query_as(
        "SELECT s.id, s.user_id, s.app, s.family_id, s.revoked_reason, s.device_name, s.expires_at > now(), u.status::text
         FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.refresh_hash = $1 FOR UPDATE OF s",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((sid, user_id, app, family, revoked, device, live, status)) = row else {
        return Err(ApiError::unauthorized());
    };
    s.limiter.check("refresh", &user_id.to_string(), s.limits.refresh).await?;

    if revoked.as_deref() == Some("rotated") {
        // A spent token came back: someone else has a copy. End every session in the family.
        let n = sqlx::query(
            "UPDATE sessions SET revoked_at = now(), revoked_reason = 'reuse_detected' WHERE family_id = $1 AND revoked_at IS NULL",
        )
        .bind(family)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        tracing::warn!(security_event = "refresh_token_reuse", %user_id, revoked_sessions = n, "refresh token reused; session family revoked");
        return Err(ApiError::unauthorized());
    }
    if revoked.is_some() || !live || status == "deleted" {
        return Err(ApiError::unauthorized());
    }
    if status == "suspended" {
        return Err(ApiError::forbidden("account_suspended", "Your account is suspended. Please contact FUber support."));
    }
    sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'rotated' WHERE id = $1").bind(sid).execute(&mut *tx).await?;
    let app = if app == "driver" { App::Driver } else { App::Rider };
    let tokens = issue_session(
        &mut tx,
        &s.keys,
        NewSession { user_id, app, family_id: Some(family), device_name: device.as_deref(), app_version: app_version.as_deref() },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(tokens))
}

#[utoipa::path(
    post, path = "/v1/auth/logout", tag = "auth",
    security(("bearer" = [])),
    responses((status = 204, description = "This device's session is ended"), (status = 401, description = "Not signed in", body = ErrorBody))
)]
pub async fn logout(State(s): State<AppState>, user: AuthUser) -> Result<StatusCode, ApiError> {
    let mut tx = s.db.begin().await?;
    sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'logout' WHERE id = $1 AND revoked_at IS NULL")
        .bind(user.session_id)
        .execute(&mut *tx)
        .await?;
    // Nobody is left to answer her requests, so a driver who signs out goes offline (demo rule).
    let online_here: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM driver_presence WHERE driver_id = $1 AND session_id = $2)")
        .bind(user.user_id)
        .bind(user.session_id)
        .fetch_one(&mut *tx)
        .await?;
    if online_here {
        fuber_db::presence::go_offline(&mut tx, user.user_id, "signed_out").await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..1000 {
            let c = new_code();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn cooldown_doubles_and_caps() {
        assert_eq!(cooldown_s(0), 0);
        assert_eq!(cooldown_s(1), 30);
        assert_eq!(cooldown_s(2), 60);
        assert_eq!(cooldown_s(3), 120);
        assert_eq!(cooldown_s(50), 900);
    }

    #[test]
    fn otp_code_and_refresh_token_shapes() {
        assert!(OtpCode::try_from("123456".to_string()).is_ok());
        for bad in ["12345", "1234567", "12345a", "１２３４５６"] {
            assert!(OtpCode::try_from(bad.to_string()).is_err(), "{bad}");
        }
        assert!(RefreshToken::try_from("a".repeat(43)).is_ok());
        assert!(RefreshToken::try_from("a".repeat(44)).is_err());
        assert!(RefreshToken::try_from(format!("{}=", "a".repeat(42))).is_err());
    }
}
