//! The signed-in person's own account: profile (PRD A-2), emergency contacts (A-3), identity
//! verification (A-2) and account deletion (A-8).

use axum::{Json, extract::State, http::StatusCode};
use fuber_common::validate::{Email, NationalId, PersonName, PhoneE164, adult_birth_date};
use fuber_db::jobs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Date, Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, RiderUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::legal::{LegalDocument, pending_consents},
};

/// Deleted accounts are erased after this grace period (docs/05-backend-schema.md §12).
pub const DELETION_GRACE: Duration = Duration::days(30);

const DOB_AAD: &str = "users.date_of_birth";
const NATIONAL_ID_AAD: &str = "users.national_id";
use fuber_db::safety::CONTACT_PHONE_AAD;

#[derive(Debug, Serialize, ToSchema)]
pub struct Me {
    pub id: Uuid,
    pub phone: String,
    pub full_name: Option<String>,
    /// First name, shown to the other party on a ride.
    pub display_name: Option<String>,
    pub email: Option<String>,
    #[schema(value_type = Option<String>, format = Date)]
    pub date_of_birth: Option<String>,
    /// `pending` until verified, then `active`.
    pub status: String,
    /// Name and date of birth are filled in.
    pub profile_complete: bool,
    /// Rider identity verification: `not_started`, `pending`, `approved` or `rejected`.
    pub verification: String,
    pub verification_reject_reason: Option<String>,
    /// Name and date of birth can no longer be changed in the app (identity under review or verified).
    pub identity_locked: bool,
    pub is_rider: bool,
    /// Driver application status, or null if this person has never applied to drive.
    pub driver_application: Option<String>,
    pub emergency_contacts: i64,
    /// Documents to accept before continuing (PRD A-7).
    pub pending_consents: Vec<LegalDocument>,
}

type MeRow =
    (String, Option<String>, Option<String>, Option<String>, Option<Vec<u8>>, String, String, Option<String>, bool, Option<String>, i64);

pub(crate) async fn load(s: &AppState, user: &AuthUser) -> Result<Me, ApiError> {
    let row: Option<MeRow> = sqlx::query_as(
        "SELECT u.phone_e164, u.full_name, u.display_name, u.email::text, u.date_of_birth, u.status::text, u.verification::text,
                (SELECT reject_reason FROM identity_verifications iv WHERE iv.user_id = u.id ORDER BY created_at DESC LIMIT 1),
                EXISTS (SELECT 1 FROM rider_profiles r WHERE r.user_id = u.id),
                (SELECT application_status::text FROM driver_profiles d WHERE d.user_id = u.id),
                (SELECT count(*) FROM emergency_contacts e WHERE e.user_id = u.id)
         FROM users u WHERE u.id = $1",
    )
    .bind(user.user_id)
    .fetch_optional(&s.db)
    .await?;
    let Some((phone, full_name, display_name, email, dob, status, verification, reject, is_rider, driver, contacts)) = row else {
        return Err(ApiError::unauthorized());
    };
    let date_of_birth = dob.map(|d| s.keys.sealer.open_str(DOB_AAD, &d)).transpose().map_err(ApiError::internal)?;
    let identity_locked = identity_locked(&verification, driver.as_deref());
    Ok(Me {
        id: user.user_id,
        phone,
        profile_complete: full_name.is_some() && date_of_birth.is_some(),
        full_name,
        display_name,
        email,
        date_of_birth,
        status,
        verification_reject_reason: if verification == "rejected" { reject } else { None },
        verification,
        identity_locked,
        is_rider,
        driver_application: driver,
        emergency_contacts: contacts,
        pending_consents: pending_consents(&s.db, user.user_id, user.app).await?,
    })
}

/// Identity details are frozen while a reviewer is looking at them or once they are approved;
/// changing them afterwards goes through support.
fn identity_locked(verification: &str, driver_application: Option<&str>) -> bool {
    matches!(verification, "pending" | "approved") || matches!(driver_application, Some("pending" | "approved"))
}

#[utoipa::path(get, path = "/v1/me", tag = "me", security(("bearer" = [])), responses((status = 200, body = Me), (status = 401, body = ErrorBody)))]
pub async fn get(State(s): State<AppState>, user: AuthUser) -> Result<Json<Me>, ApiError> {
    load(&s, &user).await.map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMe {
    #[schema(value_type = Option<String>, example = "Ndeshi Shikongo")]
    pub full_name: Option<PersonName>,
    #[schema(value_type = Option<String>)]
    pub email: Option<Email>,
    /// `YYYY-MM-DD`; must be 18 or older.
    #[schema(value_type = Option<String>, example = "1994-03-12")]
    pub date_of_birth: Option<String>,
}

#[utoipa::path(
    patch, path = "/v1/me", tag = "me", security(("bearer" = [])),
    request_body = UpdateMe,
    responses(
        (status = 200, body = Me),
        (status = 409, description = "`identity_locked`: name and date of birth are under review or verified", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn update(State(s): State<AppState>, user: AuthUser, ValidJson(req): ValidJson<UpdateMe>) -> Result<Json<Me>, ApiError> {
    let dob: Option<Date> = req
        .date_of_birth
        .as_deref()
        .map(|d| adult_birth_date(d, OffsetDateTime::now_utc().date()))
        .transpose()
        .map_err(|e| ApiError::invalid(e.0))?;
    if req.full_name.is_some() || dob.is_some() {
        let (verification, driver): (String, Option<String>) = sqlx::query_as(
            "SELECT u.verification::text, (SELECT application_status::text FROM driver_profiles d WHERE d.user_id = u.id) FROM users u WHERE u.id = $1",
        )
        .bind(user.user_id)
        .fetch_one(&s.db)
        .await?;
        if identity_locked(&verification, driver.as_deref()) {
            return Err(ApiError::conflict(
                "identity_locked",
                "Your name and date of birth are verified. Contact VARA support to change them.",
            ));
        }
    }
    let display = req.full_name.as_ref().map(|n| n.as_str().split(' ').next().unwrap_or_default().chars().take(40).collect::<String>());
    let sealed_dob = dob.map(|d| s.keys.sealer.seal_str(DOB_AAD, &d.to_string()));
    sqlx::query(
        "UPDATE users SET full_name = coalesce($2, full_name), display_name = coalesce($3, display_name),
                          email = coalesce($4, email), date_of_birth = coalesce($5, date_of_birth)
         WHERE id = $1",
    )
    .bind(user.user_id)
    .bind(req.full_name.as_ref().map(PersonName::as_str))
    .bind(display)
    .bind(req.email.as_ref().map(Email::as_str))
    .bind(sealed_dob)
    .execute(&s.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            ApiError::conflict("email_taken", "This email address is used by another account.")
        }
        _ => ApiError::internal(e),
    })?;
    load(&s, &user).await.map(Json)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeletionScheduled {
    #[serde(with = "time::serde::rfc3339")]
    pub erase_after: OffsetDateTime,
}

#[utoipa::path(
    delete, path = "/v1/me", tag = "me", security(("bearer" = [])),
    responses(
        (status = 202, description = "Signed out everywhere; personal data is erased after 30 days", body = DeletionScheduled),
        (status = 409, description = "`active_ride`: finish or cancel the current ride first", body = ErrorBody),
    )
)]
pub async fn delete(State(s): State<AppState>, user: AuthUser) -> Result<(StatusCode, Json<DeletionScheduled>), ApiError> {
    let mut tx = s.db.begin().await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM rides WHERE (rider_id = $1 OR driver_id = $1)
           AND state IN ('REQUESTED','SEARCHING','NO_DRIVERS','DRIVER_ASSIGNED','DRIVER_EN_ROUTE','DRIVER_ARRIVED','PIN_VERIFIED','IN_TRIP','SCHEDULED'))",
    )
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if active {
        return Err(ApiError::conflict("active_ride", "Finish or cancel your current or scheduled ride before deleting your account."));
    }
    let erase_after = OffsetDateTime::now_utc() + DELETION_GRACE;
    sqlx::query("UPDATE users SET status = 'deleted', deleted_at = now() WHERE id = $1").bind(user.user_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'account_deleted' WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE driver_profiles SET can_go_online = false WHERE user_id = $1").bind(user.user_id).execute(&mut *tx).await?;
    jobs::enqueue(
        &mut *tx,
        jobs::kind::ERASE_ACCOUNT,
        json!({ "user_id": user.user_id }),
        Some(erase_after),
        Some(&format!("erase:{}", user.user_id)),
    )
    .await?;
    tx.commit().await?;
    tracing::info!(user_id = %user.user_id, "account deleted; erasure scheduled");
    Ok((StatusCode::ACCEPTED, Json(DeletionScheduled { erase_after })))
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmergencyContact {
    #[schema(value_type = String)]
    pub name: PersonName,
    #[schema(value_type = String, example = "+264812223344")]
    pub phone: PhoneE164,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmergencyContacts {
    /// Up to 3, in order of who to alert first.
    pub contacts: Vec<EmergencyContact>,
}

#[utoipa::path(get, path = "/v1/me/emergency-contacts", tag = "me", security(("bearer" = [])), responses((status = 200, body = EmergencyContacts)))]
pub async fn contacts(State(s): State<AppState>, user: AuthUser) -> Result<Json<EmergencyContacts>, ApiError> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as("SELECT name, phone FROM emergency_contacts WHERE user_id = $1 ORDER BY position")
        .bind(user.user_id)
        .fetch_all(&s.db)
        .await?;
    let contacts = rows
        .into_iter()
        .map(|(name, phone)| {
            let phone = s.keys.sealer.open_str(CONTACT_PHONE_AAD, &phone).map_err(ApiError::internal)?;
            Ok(EmergencyContact { name: PersonName::stored(name), phone: PhoneE164::parse(&phone).map_err(|e| ApiError::internal(e.0))? })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(EmergencyContacts { contacts }))
}

#[utoipa::path(
    put, path = "/v1/me/emergency-contacts", tag = "me", security(("bearer" = [])),
    request_body = EmergencyContacts,
    responses((status = 200, body = EmergencyContacts), (status = 422, body = ErrorBody))
)]
pub async fn replace_contacts(
    State(s): State<AppState>,
    user: AuthUser,
    ValidJson(req): ValidJson<EmergencyContacts>,
) -> Result<Json<EmergencyContacts>, ApiError> {
    if req.contacts.len() > 3 {
        return Err(ApiError::invalid("you can add up to 3 emergency contacts"));
    }
    let own: String = sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = $1").bind(user.user_id).fetch_one(&s.db).await?;
    for (i, c) in req.contacts.iter().enumerate() {
        if c.phone.as_str() == own {
            return Err(ApiError::invalid("an emergency contact can't be your own number"));
        }
        if req.contacts[..i].iter().any(|o| o.phone == c.phone) {
            return Err(ApiError::invalid("each emergency contact needs a different number"));
        }
    }
    let mut tx = s.db.begin().await?;
    sqlx::query("DELETE FROM emergency_contacts WHERE user_id = $1").bind(user.user_id).execute(&mut *tx).await?;
    for (i, c) in req.contacts.iter().enumerate() {
        sqlx::query("INSERT INTO emergency_contacts (user_id, name, phone, position) VALUES ($1, $2, $3, $4)")
            .bind(user.user_id)
            .bind(c.name.as_str())
            .bind(s.keys.sealer.seal_str(CONTACT_PHONE_AAD, c.phone.as_str()))
            .bind(i as i16 + 1)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(req))
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationRequest {
    /// Upload with purpose `identity`: national ID card or passport photo page.
    pub id_document_upload_id: Uuid,
    /// Upload with purpose `selfie`.
    pub selfie_upload_id: Uuid,
    /// 11-digit Namibian ID or passport number. Encrypted; used to stop duplicate accounts.
    #[schema(value_type = Option<String>)]
    pub national_id: Option<NationalId>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VerificationSubmitted {
    pub verification_id: Uuid,
    pub status: &'static str,
}

#[utoipa::path(
    post, path = "/v1/me/verification", tag = "me", security(("bearer" = [])),
    request_body = VerificationRequest,
    responses(
        (status = 202, description = "Waiting for review", body = VerificationSubmitted),
        (status = 409, description = "`profile_incomplete`, `already_verified`, `verification_pending` or `duplicate_identity`", body = ErrorBody),
        (status = 422, description = "An upload is missing, not yours, of the wrong kind or was rejected", body = ErrorBody),
    )
)]
pub async fn submit_verification(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    ValidJson(req): ValidJson<VerificationRequest>,
) -> Result<(StatusCode, Json<VerificationSubmitted>), ApiError> {
    let mut tx = s.db.begin().await?;
    let (complete, verification): (bool, String) = sqlx::query_as(
        "SELECT full_name IS NOT NULL AND date_of_birth IS NOT NULL, verification::text FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if !complete {
        return Err(ApiError::conflict("profile_incomplete", "Add your full name and date of birth first."));
    }
    match verification.as_str() {
        "approved" => return Err(ApiError::conflict("already_verified", "You're already verified.")),
        "pending" => return Err(ApiError::conflict("verification_pending", "Your verification is already being reviewed.")),
        _ => {}
    }
    let id_key = usable_upload(&mut tx, user.user_id, req.id_document_upload_id, "identity").await?;
    let selfie_key = usable_upload(&mut tx, user.user_id, req.selfie_upload_id, "selfie").await?;

    if let Some(nid) = &req.national_id {
        let hash = s.keys.hasher.hash("national_id", nid.as_str().as_bytes()).to_vec();
        sqlx::query("UPDATE users SET national_id = $2, national_id_hash = $3 WHERE id = $1")
            .bind(user.user_id)
            .bind(s.keys.sealer.seal_str(NATIONAL_ID_AAD, nid.as_str()))
            .bind(hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(d) if d.is_unique_violation() => {
                    tracing::warn!(security_event = "duplicate_national_id", user_id = %user.user_id, "ID number already linked to another account");
                    ApiError::conflict("duplicate_identity", "This ID number is already linked to another VARA account. Please contact support.")
                }
                _ => ApiError::internal(e),
            })?;
    }
    let verification_id: Uuid = sqlx::query_scalar(
        "INSERT INTO identity_verifications (user_id, id_upload_id, selfie_upload_id, id_document_key, selfie_key)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user.user_id)
    .bind(req.id_document_upload_id)
    .bind(req.selfie_upload_id)
    .bind(id_key)
    .bind(selfie_key)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE users SET verification = 'pending' WHERE id = $1").bind(user.user_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(VerificationSubmitted { verification_id, status: "pending" })))
}

/// An upload of this user, for this purpose, that has been sent and not rejected. Returns its
/// clean storage key if processing has already finished. Someone else's upload looks exactly
/// like a missing one.
pub(crate) async fn usable_upload(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    upload_id: Uuid,
    purpose: &str,
) -> Result<Option<String>, ApiError> {
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT purpose::text, status::text, storage_key FROM uploads WHERE id = $1 AND user_id = $2")
            .bind(upload_id)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;
    match row {
        None => Err(ApiError::invalid("upload not found")),
        Some((p, _, _)) if p != purpose => Err(ApiError::invalid(format!("upload is not a {} photo", purpose.replace('_', " ")))),
        Some((_, status, _)) if status == "awaiting_upload" => Err(ApiError::invalid("upload was not completed")),
        Some((_, status, _)) if status == "rejected" => Err(ApiError::invalid("upload was rejected; please upload a new photo")),
        Some((_, _, key)) => Ok(key),
    }
}
