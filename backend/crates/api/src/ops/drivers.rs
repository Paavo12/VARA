//! Driver application review (PRD A-4 – A-6): each document, the vehicle, approved tiers and
//! the final decision. `can_go_online` is recomputed after every change.

use axum::{
    Json,
    extract::{Path, State},
};
use fuber_db::drivers::{REQUIRED_DOCUMENTS, refresh_can_go_online};
use fuber_domain::Tier;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{Decision, Staff, Verdict, presign_get, verifications::IMAGE_URL_TTL};
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct DriverQueueItem {
    pub user_id: Uuid,
    pub full_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub submitted_at: OffsetDateTime,
    pub requested_tiers: Vec<String>,
    pub documents_in_review: i64,
}

#[utoipa::path(
    get, path = "/v1/ops/drivers", operation_id = "driver_queue", tag = "ops", security(("staff" = [])),
    responses((status = 200, description = "Submitted applications, oldest first", body = Vec<DriverQueueItem>))
)]
pub async fn queue(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<DriverQueueItem>>, ApiError> {
    staff.require_reviewer()?;
    let rows: Vec<QueueRow> = sqlx::query_as(
        "SELECT dp.user_id, u.full_name, dp.submitted_at, dp.requested_tiers::text[],
                (SELECT count(*) FROM driver_documents d WHERE d.driver_id = dp.user_id AND d.superseded_by IS NULL AND d.status = 'in_review')
         FROM driver_profiles dp JOIN users u ON u.id = dp.user_id
         WHERE dp.application_status = 'pending' AND u.status <> 'deleted'
         ORDER BY dp.submitted_at LIMIT 100",
    )
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(user_id, full_name, submitted_at, requested_tiers, documents_in_review)| DriverQueueItem {
                user_id,
                full_name,
                submitted_at,
                requested_tiers,
                documents_in_review,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewDocument {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    #[schema(value_type = Option<String>, format = Date)]
    pub expires_on: Option<String>,
    pub reject_reason: Option<String>,
    pub mime_type: Option<String>,
    /// Short-lived link to the sanitised file, once it has passed the upload checks.
    pub url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewVehicle {
    pub id: Uuid,
    pub make: String,
    pub model: String,
    pub colour: String,
    pub year: i16,
    pub plate: String,
    pub seats: i16,
    pub wheelchair_accessible: bool,
    pub approved: bool,
    pub reject_reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DriverReview {
    pub user_id: Uuid,
    pub full_name: Option<String>,
    #[schema(value_type = Option<String>, format = Date)]
    pub date_of_birth: Option<String>,
    pub application_status: String,
    pub decision_reason: Option<String>,
    pub requested_tiers: Vec<String>,
    pub approved_tiers: Vec<String>,
    pub can_go_online: bool,
    pub vehicle: Option<ReviewVehicle>,
    pub documents: Vec<ReviewDocument>,
}

type QueueRow = (Uuid, Option<String>, OffsetDateTime, Vec<String>, i64);
type DocRow = (Uuid, String, String, Option<Date>, Option<String>, Option<String>, Option<String>);
type ReviewRow = (Option<String>, Option<Vec<u8>>, String, Option<String>, Vec<String>, bool);
type VehicleRow = (Uuid, String, String, String, i16, String, i16, bool, bool, Option<String>);

#[utoipa::path(
    get, path = "/v1/ops/drivers/{id}", operation_id = "driver_detail", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path, description = "The driver's user ID")),
    responses((status = 200, body = DriverReview), (status = 404, body = ErrorBody))
)]
pub async fn detail(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
) -> Result<Json<DriverReview>, ApiError> {
    staff.require_reviewer()?;
    let row: Option<ReviewRow> = sqlx::query_as(
        "SELECT u.full_name, u.date_of_birth, dp.application_status::text, dp.decision_reason, dp.requested_tiers::text[], dp.can_go_online
         FROM driver_profiles dp JOIN users u ON u.id = dp.user_id WHERE dp.user_id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let (full_name, dob, application_status, decision_reason, requested_tiers, can_go_online) = row.ok_or_else(ApiError::not_found)?;
    let approved_tiers: Vec<String> =
        sqlx::query_scalar("SELECT tier::text FROM driver_tiers WHERE driver_id = $1 ORDER BY tier").bind(id).fetch_all(&s.db).await?;
    let vehicle: Option<VehicleRow> = sqlx::query_as(
        "SELECT id, make, model, colour, year, plate, seats, wheelchair_accessible, approved, reject_reason
         FROM vehicles WHERE driver_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let docs: Vec<DocRow> = sqlx::query_as(
        "SELECT id, kind::text, status::text, expires_on, reject_reason, mime_type, storage_key
         FROM driver_documents WHERE driver_id = $1 AND superseded_by IS NULL ORDER BY kind",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;

    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "driver_documents.view",
            target_type: "driver",
            target_id: id.to_string(),
            detail: None,
            ip: Some(ip),
        },
    )
    .await?;
    let exp = (OffsetDateTime::now_utc() + IMAGE_URL_TTL).unix_timestamp();
    let date_of_birth = dob.map(|d| s.keys.sealer.open_str("users.date_of_birth", &d)).transpose().map_err(ApiError::internal)?;
    let mut documents = Vec::with_capacity(docs.len());
    for (id, kind, status, exp_on, reject_reason, mime_type, key) in docs {
        let url = presign_get(&s, key, exp).await?;
        documents.push(ReviewDocument { id, kind, status, expires_on: exp_on.map(|d| d.to_string()), reject_reason, mime_type, url });
    }
    Ok(Json(DriverReview {
        user_id: id,
        full_name,
        date_of_birth,
        application_status,
        decision_reason,
        requested_tiers,
        approved_tiers,
        can_go_online,
        vehicle: vehicle.map(|(id, make, model, colour, year, plate, seats, wheelchair_accessible, approved, reject_reason)| {
            ReviewVehicle { id, make, model, colour, year, plate, seats, wheelchair_accessible, approved, reject_reason }
        }),
        documents,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DocumentDecided {
    pub id: Uuid,
    pub status: String,
    pub driver_can_go_online: bool,
}

#[utoipa::path(
    post, path = "/v1/ops/documents/{id}/decision", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Decision,
    responses((status = 200, body = DocumentDecided), (status = 409, description = "`not_in_review`", body = ErrorBody))
)]
pub async fn decide_document(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(d): ValidJson<Decision>,
) -> Result<Json<DocumentDecided>, ApiError> {
    staff.require_reviewer()?;
    let reason = d.reason()?;
    let status = if d.decision == Verdict::Approve { "approved" } else { "rejected" };
    let mut tx = s.db.begin().await?;
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE driver_documents SET status = $2::document_status, reject_reason = $3, reviewed_by = $4, reviewed_at = now()
         WHERE id = $1 AND status = 'in_review' AND superseded_by IS NULL RETURNING driver_id, kind::text",
    )
    .bind(id)
    .bind(status)
    .bind(reason)
    .bind(staff.id)
    .fetch_optional(&mut *tx)
    .await?;
    let (driver_id, kind) = row.ok_or_else(|| ApiError::conflict("not_in_review", "This document isn't waiting for a decision."))?;
    let can_go_online = refresh_can_go_online(&mut *tx, driver_id).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(driver_id),
            action: if d.decision == Verdict::Approve { "driver_document.approve" } else { "driver_document.reject" },
            target_type: "driver_document",
            target_id: id.to_string(),
            detail: Some(json!({ "kind": kind, "reason": reason })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(DocumentDecided { id, status: status.to_owned(), driver_can_go_online: can_go_online }))
}

#[utoipa::path(
    post, path = "/v1/ops/vehicles/{id}/decision", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Decision,
    responses((status = 200, body = DocumentDecided), (status = 404, body = ErrorBody))
)]
pub async fn decide_vehicle(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(d): ValidJson<Decision>,
) -> Result<Json<DocumentDecided>, ApiError> {
    staff.require_reviewer()?;
    let reason = d.reason()?;
    let approve = d.decision == Verdict::Approve;
    let mut tx = s.db.begin().await?;
    let driver_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE vehicles SET approved = $2, reject_reason = $3, reviewed_by = $4, reviewed_at = now() WHERE id = $1 RETURNING driver_id",
    )
    .bind(id)
    .bind(approve)
    .bind(reason)
    .bind(staff.id)
    .fetch_optional(&mut *tx)
    .await?;
    let driver_id = driver_id.ok_or_else(ApiError::not_found)?;
    if approve {
        sqlx::query("UPDATE driver_profiles SET active_vehicle_id = coalesce(active_vehicle_id, $2) WHERE user_id = $1")
            .bind(driver_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let can_go_online = refresh_can_go_online(&mut *tx, driver_id).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(driver_id),
            action: if approve { "vehicle.approve" } else { "vehicle.reject" },
            target_type: "vehicle",
            target_id: id.to_string(),
            detail: reason.map(|r| json!({ "reason": r })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(DocumentDecided { id, status: if approve { "approved" } else { "rejected" }.into(), driver_can_go_online: can_go_online }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTiers {
    /// The full set of approved tiers; an empty list removes them all.
    pub tiers: Vec<Tier>,
}

#[utoipa::path(
    put, path = "/v1/ops/drivers/{id}/tiers", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = SetTiers,
    responses((status = 200, body = DriverReview), (status = 404, body = ErrorBody))
)]
pub async fn set_tiers(
    State(s): State<AppState>,
    staff: Staff,
    ip: ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<SetTiers>,
) -> Result<Json<DriverReview>, ApiError> {
    staff.require_reviewer()?;
    let mut tiers: Vec<String> =
        req.tiers.iter().filter_map(|t| serde_json::to_value(t).ok().and_then(|v| v.as_str().map(str::to_owned))).collect();
    tiers.sort();
    tiers.dedup();
    let mut tx = s.db.begin().await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM driver_profiles WHERE user_id = $1)").bind(id).fetch_one(&mut *tx).await?;
    if !exists {
        return Err(ApiError::not_found());
    }
    sqlx::query("DELETE FROM driver_tiers WHERE driver_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO driver_tiers (driver_id, tier, approved_by) SELECT $1, t::ride_tier, $3 FROM unnest($2::text[]) AS t")
        .bind(id)
        .bind(&tiers)
        .bind(staff.id)
        .execute(&mut *tx)
        .await?;
    refresh_can_go_online(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "driver.set_tiers",
            target_type: "driver",
            target_id: id.to_string(),
            detail: Some(json!({ "tiers": tiers })),
            ip: Some(ip.0),
        },
    )
    .await?;
    tx.commit().await?;
    detail(State(s), staff, ip, Path(id)).await
}

#[utoipa::path(
    post, path = "/v1/ops/drivers/{id}/decision", operation_id = "decide_driver", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Decision,
    responses(
        (status = 200, body = DriverReview),
        (status = 409, description = "`not_pending`, or `not_ready`: documents, vehicle or tiers not all approved", body = ErrorBody),
    )
)]
pub async fn decide(
    State(s): State<AppState>,
    staff: Staff,
    ip: ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(d): ValidJson<Decision>,
) -> Result<Json<DriverReview>, ApiError> {
    staff.require_reviewer()?;
    let reason = d.reason()?;
    let mut tx = s.db.begin().await?;
    let status: Option<String> = sqlx::query_scalar("SELECT application_status::text FROM driver_profiles WHERE user_id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    match status.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("pending") => {}
        Some(_) => return Err(ApiError::conflict("not_pending", "This application isn't waiting for a decision.")),
    }
    if d.decision == Verdict::Approve {
        let (docs_ok, vehicle_ok, tiers_ok): (bool, bool, bool) = sqlx::query_as(
            "SELECT
               (SELECT count(DISTINCT kind) FROM driver_documents WHERE driver_id = $1 AND superseded_by IS NULL AND status = 'approved'
                  AND kind::text = ANY($2) AND (expires_on IS NULL OR expires_on >= current_date)) = cardinality($2),
               EXISTS (SELECT 1 FROM vehicles WHERE driver_id = $1 AND approved),
               EXISTS (SELECT 1 FROM driver_tiers WHERE driver_id = $1)",
        )
        .bind(id)
        .bind(&REQUIRED_DOCUMENTS[..])
        .fetch_one(&mut *tx)
        .await?;
        if !(docs_ok && vehicle_ok && tiers_ok) {
            let missing: Vec<&str> = [(!docs_ok, "documents"), (!vehicle_ok, "vehicle"), (!tiers_ok, "tiers")]
                .into_iter()
                .filter(|(m, _)| *m)
                .map(|(_, n)| n)
                .collect();
            return Err(ApiError::conflict("not_ready", format!("Approve these first: {}.", missing.join(", "))));
        }
        sqlx::query(
            "UPDATE driver_profiles SET application_status = 'approved', approved_at = now(), decision_reason = $2 WHERE user_id = $1",
        )
        .bind(id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE users SET status = 'active' WHERE id = $1 AND status = 'pending'").bind(id).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE driver_profiles SET application_status = 'rejected', decision_reason = $2 WHERE user_id = $1")
            .bind(id)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
    }
    refresh_can_go_online(&mut *tx, id).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: if d.decision == Verdict::Approve { "driver.approve" } else { "driver.reject" },
            target_type: "driver",
            target_id: id.to_string(),
            detail: reason.map(|r| json!({ "reason": r })),
            ip: Some(ip.0),
        },
    )
    .await?;
    tx.commit().await?;
    detail(State(s), staff, ip, Path(id)).await
}
