//! Driver application (PRD A-4, A-5, A-6), from the driver app.
//!
//! The application is a draft (`not_started`) until submitted; it is then `pending` while ops
//! review each document and the vehicle, assign tiers and decide. A rejected application can
//! be corrected and submitted again. Vehicle and payout details can't be changed while under
//! review or once approved (that goes through support until Phase 5 adds re-review).

use axum::{Json, extract::State, http::StatusCode};
use fuber_common::validate::{Bounded, Line, PersonName, Plate};
use fuber_db::drivers::{EXPIRING_DOCUMENTS, REQUIRED_DOCUMENTS, refresh_can_go_online};
use fuber_domain::Tier;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, DriverUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::{
        legal::{LegalDocument, pending_consents},
        me::usable_upload,
        uploads::DocumentKind,
    },
};

const PAYOUT_AAD: &str = "driver_profiles.payout_account";

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Vehicle {
    #[schema(value_type = String, example = "Toyota")]
    pub make: Line<40>,
    #[schema(value_type = String, example = "Corolla")]
    pub model: Line<40>,
    #[schema(value_type = String, example = "Silver")]
    pub colour: Line<30>,
    #[schema(value_type = i64, example = 2019)]
    pub year: Bounded<1990, 2100>,
    #[schema(value_type = String, example = "N 9090 W")]
    pub plate: Plate,
    #[schema(value_type = i64, minimum = 4, maximum = 8)]
    pub seats: Bounded<4, 8>,
    pub wheelchair_accessible: bool,
}

/// Bank account for weekly payouts. Stored encrypted; never returned by the API.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PayoutAccount {
    #[schema(value_type = String, example = "Bank Windhoek")]
    pub bank: Line<60>,
    #[schema(value_type = String)]
    pub account_holder: PersonName,
    #[schema(value_type = String, example = "8001234567")]
    pub account_number: Digits<6, 16>,
    #[schema(value_type = String, example = "483872")]
    pub branch_code: Digits<4, 8>,
}

/// `MIN..=MAX` ASCII digits (spaces removed).
#[derive(Debug)]
pub struct Digits<const MIN: usize, const MAX: usize>(String);

impl<'de, const MIN: usize, const MAX: usize> Deserialize<'de> for Digits<MIN, MAX> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = String::deserialize(d)?.chars().filter(|c| *c != ' ').collect();
        if (MIN..=MAX).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()) {
            Ok(Digits(s))
        } else {
            Err(serde::de::Error::custom(format!("must be {MIN} to {MAX} digits")))
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveApplication {
    /// Ride types the driver wants to offer; ops decide which are approved.
    pub requested_tiers: Vec<Tier>,
    pub vehicle: Vehicle,
    /// Omit to keep the account already saved.
    pub payout_account: Option<PayoutAccount>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DriverDocument {
    pub id: Uuid,
    pub kind: String,
    /// `uploading` (being checked), `in_review`, `approved`, `rejected` or `expired`.
    pub status: String,
    #[schema(value_type = Option<String>, format = Date)]
    pub expires_on: Option<String>,
    pub reject_reason: Option<String>,
    /// Set when the file itself was refused by the upload checks.
    pub upload_reject_reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VehicleStatus {
    pub id: Uuid,
    #[serde(flatten)]
    pub vehicle: Vehicle,
    pub approved: bool,
    pub reject_reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Application {
    /// `not_started` (draft), `pending`, `approved` or `rejected`.
    pub status: String,
    pub decision_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub submitted_at: Option<OffsetDateTime>,
    pub requested_tiers: Vec<Tier>,
    pub approved_tiers: Vec<Tier>,
    pub vehicle: Option<VehicleStatus>,
    pub payout_account_saved: bool,
    pub documents: Vec<DriverDocument>,
    /// Required document kinds with no current upload, or whose current one was rejected or expired.
    pub missing_documents: Vec<String>,
    pub pending_consents: Vec<LegalDocument>,
    pub profile_complete: bool,
    /// Everything needed to submit is present.
    pub ready_to_submit: bool,
    /// All checks passed: the driver may go online (Phase 2).
    pub can_go_online: bool,
}

fn parse_tier(s: &str) -> Option<Tier> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

fn tier_str(t: Tier) -> String {
    serde_json::to_value(t).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

type RawProfileRow = (Option<String>, Option<String>, Option<OffsetDateTime>, Option<Vec<String>>, Option<bool>, Option<bool>, bool);
type DocRow = (Uuid, String, String, Option<Date>, Option<String>, Option<String>);
type ProfileRow = (String, Option<String>, Option<OffsetDateTime>, Vec<String>, bool, bool, bool);
type VehicleRow = (Uuid, String, String, String, i16, String, i16, bool, bool, Option<String>);

async fn load(s: &AppState, user: &AuthUser) -> Result<Application, ApiError> {
    let profile: Option<ProfileRow> = sqlx::query_as(
        "SELECT dp.application_status::text, dp.decision_reason, dp.submitted_at, dp.requested_tiers::text[],
                dp.payout_account IS NOT NULL, dp.can_go_online, u.full_name IS NOT NULL AND u.date_of_birth IS NOT NULL
         FROM users u LEFT JOIN driver_profiles dp ON dp.user_id = u.id WHERE u.id = $1",
    )
    .bind(user.user_id)
    .fetch_optional(&s.db)
    .await?
    .map(|(st, r, at, t, p, g, c): RawProfileRow| {
        (st.unwrap_or_else(|| "not_started".into()), r, at, t.unwrap_or_default(), p.unwrap_or(false), g.unwrap_or(false), c)
    });
    let (status, decision_reason, submitted_at, requested, payout, can_go_online, profile_complete) =
        profile.ok_or_else(ApiError::unauthorized)?;

    let approved_tiers: Vec<String> = sqlx::query_scalar("SELECT tier::text FROM driver_tiers WHERE driver_id = $1 ORDER BY tier")
        .bind(user.user_id)
        .fetch_all(&s.db)
        .await?;
    let vehicle: Option<VehicleRow> = sqlx::query_as(
        "SELECT id, make, model, colour, year, plate, seats, wheelchair_accessible, approved, reject_reason
         FROM vehicles WHERE driver_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user.user_id)
    .fetch_optional(&s.db)
    .await?;
    let vehicle = vehicle
        .map(|(id, make, model, colour, year, plate, seats, wc, approved, reject_reason)| -> Result<VehicleStatus, ApiError> {
            let bad = |e: fuber_common::validate::Invalid| ApiError::internal(e.0);
            Ok(VehicleStatus {
                id,
                vehicle: Vehicle {
                    make: Line::parse(&make).map_err(bad)?,
                    model: Line::parse(&model).map_err(bad)?,
                    colour: Line::parse(&colour).map_err(bad)?,
                    year: i64::from(year).try_into().map_err(bad)?,
                    plate: Plate::parse(&plate).map_err(bad)?,
                    seats: i64::from(seats).try_into().map_err(bad)?,
                    wheelchair_accessible: wc,
                },
                approved,
                reject_reason,
            })
        })
        .transpose()?;

    let docs: Vec<DocRow> = sqlx::query_as(
        "SELECT d.id, d.kind::text, d.status::text, d.expires_on, d.reject_reason, u.reject_reason
         FROM driver_documents d LEFT JOIN uploads u ON u.id = d.upload_id
         WHERE d.driver_id = $1 AND d.superseded_by IS NULL ORDER BY d.kind",
    )
    .bind(user.user_id)
    .fetch_all(&s.db)
    .await?;
    let documents: Vec<DriverDocument> = docs
        .into_iter()
        .map(|(id, kind, status, exp, reason, upload_reason)| DriverDocument {
            id,
            kind,
            status,
            expires_on: exp.map(|d| d.to_string()),
            reject_reason: reason,
            upload_reject_reason: upload_reason,
        })
        .collect();
    let missing_documents: Vec<String> = REQUIRED_DOCUMENTS
        .iter()
        .filter(|k| {
            !documents
                .iter()
                .any(|d| d.kind == **k && !matches!(d.status.as_str(), "rejected" | "expired") && d.upload_reject_reason.is_none())
        })
        .map(|k| (*k).to_owned())
        .collect();
    let pending_consents = pending_consents(&s.db, user.user_id, user.app).await?;
    let ready_to_submit = matches!(status.as_str(), "not_started" | "rejected")
        && profile_complete
        && vehicle.is_some()
        && !requested.is_empty()
        && payout
        && missing_documents.is_empty()
        && pending_consents.is_empty();
    Ok(Application {
        status,
        decision_reason,
        submitted_at,
        requested_tiers: requested.iter().filter_map(|t| parse_tier(t)).collect(),
        approved_tiers: approved_tiers.iter().filter_map(|t| parse_tier(t)).collect(),
        vehicle,
        payout_account_saved: payout,
        documents,
        missing_documents,
        pending_consents,
        profile_complete,
        ready_to_submit,
        can_go_online,
    })
}

#[utoipa::path(get, path = "/v1/driver/application", tag = "driver", security(("bearer" = [])), responses((status = 200, body = Application)))]
pub async fn get_application(State(s): State<AppState>, DriverUser(user): DriverUser) -> Result<Json<Application>, ApiError> {
    load(&s, &user).await.map(Json)
}

/// Creates the driver profile on first use (PRD A-9: an existing rider can apply too).
async fn ensure_profile(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, user_id: Uuid) -> Result<String, ApiError> {
    sqlx::query("INSERT INTO driver_profiles (user_id) VALUES ($1) ON CONFLICT DO NOTHING").bind(user_id).execute(&mut **tx).await?;
    Ok(sqlx::query_scalar("SELECT application_status::text FROM driver_profiles WHERE user_id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?)
}

#[utoipa::path(
    put, path = "/v1/driver/application", tag = "driver", security(("bearer" = [])),
    request_body = SaveApplication,
    responses(
        (status = 200, body = Application),
        (status = 409, description = "`application_locked` (under review or approved) or `plate_taken`", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn save_application(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    ValidJson(req): ValidJson<SaveApplication>,
) -> Result<Json<Application>, ApiError> {
    let mut tiers: Vec<String> = req.requested_tiers.iter().map(|t| tier_str(*t)).collect();
    tiers.sort();
    tiers.dedup();
    if tiers.is_empty() {
        return Err(ApiError::invalid("choose at least one ride type"));
    }
    let next_year = i64::from(OffsetDateTime::now_utc().year()) + 1;
    if req.vehicle.year.get() > next_year {
        return Err(ApiError::invalid("vehicle year is in the future"));
    }

    let mut tx = s.db.begin().await?;
    let status = ensure_profile(&mut tx, user.user_id).await?;
    if matches!(status.as_str(), "pending" | "approved") {
        return Err(ApiError::conflict(
            "application_locked",
            "Your application is being reviewed or approved. Contact support to change it.",
        ));
    }
    let payout = req.payout_account.as_ref().map(|p| {
        let json = serde_json::json!({ "bank": p.bank.as_str(), "account_holder": p.account_holder.as_str(), "account_number": p.account_number.0, "branch_code": p.branch_code.0 });
        s.keys.sealer.seal(PAYOUT_AAD, json.to_string().as_bytes())
    });
    sqlx::query(
        "UPDATE driver_profiles SET requested_tiers = $2::ride_tier[], payout_account = coalesce($3, payout_account) WHERE user_id = $1",
    )
    .bind(user.user_id)
    .bind(&tiers)
    .bind(payout)
    .execute(&mut *tx)
    .await?;

    let v = &req.vehicle;
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM vehicles WHERE driver_id = $1 ORDER BY created_at DESC LIMIT 1")
        .bind(user.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    let q = match existing {
        Some(_) => {
            "UPDATE vehicles SET make = $2, model = $3, colour = $4, year = $5, plate = $6, seats = $7, wheelchair_accessible = $8,
                            approved = false, reject_reason = NULL, reviewed_by = NULL, reviewed_at = NULL
                     WHERE id = $1"
        }
        None => {
            "INSERT INTO vehicles (driver_id, make, model, colour, year, plate, seats, wheelchair_accessible) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        }
    };
    sqlx::query(q)
        .bind(existing.unwrap_or(user.user_id))
        .bind(v.make.as_str())
        .bind(v.model.as_str())
        .bind(v.colour.as_str())
        .bind(v.year.get() as i16)
        .bind(v.plate.as_str())
        .bind(v.seats.get() as i16)
        .bind(v.wheelchair_accessible)
        .execute(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                ApiError::conflict("plate_taken", "This number plate is registered to another driver.")
            }
            _ => ApiError::internal(e),
        })?;
    tx.commit().await?;
    load(&s, &user).await.map(Json)
}

#[utoipa::path(
    post, path = "/v1/driver/application/submit", tag = "driver", security(("bearer" = [])),
    responses(
        (status = 200, description = "Submitted for review", body = Application),
        (status = 409, description = "`application_incomplete` (see `ready_to_submit` on GET) or `application_locked`", body = ErrorBody),
        (status = 429, body = ErrorBody),
    )
)]
pub async fn submit(State(s): State<AppState>, DriverUser(user): DriverUser) -> Result<Json<Application>, ApiError> {
    s.limiter.check("driver_submit", &user.user_id.to_string(), s.limits.driver_submit).await?;
    let app = load(&s, &user).await?;
    if matches!(app.status.as_str(), "pending" | "approved") {
        return Err(ApiError::conflict("application_locked", "Your application has already been submitted."));
    }
    if !app.ready_to_submit {
        return Err(ApiError::conflict("application_incomplete", "Some details or documents are still missing."));
    }
    let updated = sqlx::query(
        "UPDATE driver_profiles SET application_status = 'pending', submitted_at = now(), decision_reason = NULL
         WHERE user_id = $1 AND application_status IN ('not_started', 'rejected')",
    )
    .bind(user.user_id)
    .execute(&s.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::conflict("application_locked", "Your application has already been submitted."));
    }
    load(&s, &user).await.map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddDocument {
    pub kind: DocumentKind,
    /// A completed upload with the same purpose as `kind`.
    pub upload_id: Uuid,
    /// Required for licence, insurance, police clearance and transport permit; must be in the future.
    #[schema(value_type = Option<String>, format = Date, example = "2027-06-30")]
    pub expires_on: Option<String>,
}

#[utoipa::path(
    post, path = "/v1/driver/documents", tag = "driver", security(("bearer" = [])),
    request_body = AddDocument,
    responses(
        (status = 201, description = "Added; replaces the current document of this kind", body = DriverDocument),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn add_document(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    ValidJson(req): ValidJson<AddDocument>,
) -> Result<(StatusCode, Json<DriverDocument>), ApiError> {
    let kind = req.kind.as_str();
    let today = OffsetDateTime::now_utc().date();
    let expires_on = match (&req.expires_on, EXPIRING_DOCUMENTS.contains(&kind)) {
        (Some(raw), true) => {
            let d = Date::parse(raw, time::macros::format_description!("[year]-[month]-[day]"))
                .map_err(|_| ApiError::invalid("expires_on must be a date like 2027-06-30"))?;
            if d <= today {
                return Err(ApiError::invalid("this document has already expired"));
            }
            if d.year() > today.year() + 15 {
                return Err(ApiError::invalid("expiry date is too far in the future"));
            }
            Some(d)
        }
        (None, true) => return Err(ApiError::invalid("expires_on is required for this document")),
        (Some(_), false) => return Err(ApiError::invalid("this document has no expiry date")),
        (None, false) => None,
    };

    let mut tx = s.db.begin().await?;
    let status = ensure_profile(&mut tx, user.user_id).await?;
    if status == "pending" {
        return Err(ApiError::conflict(
            "application_locked",
            "Your application is being reviewed. You can replace documents once a decision is made.",
        ));
    }
    let clean_key = usable_upload(&mut tx, user.user_id, req.upload_id, kind).await?;
    let new_id = Uuid::new_v4();
    // The deferred foreign key lets the old row point at the new one before it exists.
    sqlx::query(
        "UPDATE driver_documents SET superseded_by = $3 WHERE driver_id = $1 AND kind = $2::document_kind AND superseded_by IS NULL",
    )
    .bind(user.user_id)
    .bind(kind)
    .bind(new_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO driver_documents (id, driver_id, kind, upload_id, expires_on, status, storage_key, mime_type, size_bytes, sha256)
         SELECT $1, $2, $3::document_kind, u.id, $4, CASE WHEN u.status = 'clean' THEN 'in_review' ELSE 'uploading' END::document_status,
                u.storage_key, u.mime_type, u.size_bytes, u.sha256
         FROM uploads u WHERE u.id = $5",
    )
    .bind(new_id)
    .bind(user.user_id)
    .bind(kind)
    .bind(expires_on)
    .bind(req.upload_id)
    .execute(&mut *tx)
    .await?;
    refresh_can_go_online(&mut *tx, user.user_id).await?;
    tx.commit().await?;
    let doc = DriverDocument {
        id: new_id,
        kind: kind.to_owned(),
        status: if clean_key.is_some() { "in_review" } else { "uploading" }.to_owned(),
        expires_on: expires_on.map(|d| d.to_string()),
        reject_reason: None,
        upload_reject_reason: None,
    };
    Ok((StatusCode::CREATED, Json(doc)))
}
