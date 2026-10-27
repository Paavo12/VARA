//! Rider identity verification review (PRD A-2, App Flow §9).

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use serde_json::json;
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{Decision, Staff, Verdict};
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

/// Image links given to reviewers expire quickly; every link issued is audit-logged.
pub const IMAGE_URL_TTL: Duration = Duration::minutes(5);

#[derive(Debug, Serialize, ToSchema)]
pub struct QueueItem {
    pub id: Uuid,
    pub user_id: Uuid,
    pub display_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub submitted_at: OffsetDateTime,
    /// Earlier verifications by this person (all rejected).
    pub previous_attempts: i64,
}

#[utoipa::path(
    get, path = "/v1/ops/verifications", tag = "ops", security(("staff" = [])),
    responses((status = 200, description = "Pending verifications whose photos passed the upload checks, oldest first", body = Vec<QueueItem>))
)]
pub async fn queue(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<QueueItem>>, ApiError> {
    staff.require_reviewer()?;
    let rows: Vec<(Uuid, Uuid, Option<String>, OffsetDateTime, i64)> = sqlx::query_as(
        "SELECT iv.id, iv.user_id, u.display_name, iv.created_at,
                (SELECT count(*) FROM identity_verifications p WHERE p.user_id = iv.user_id AND p.id <> iv.id)
         FROM identity_verifications iv JOIN users u ON u.id = iv.user_id
         WHERE iv.status = 'pending' AND iv.id_document_key IS NOT NULL AND iv.selfie_key IS NOT NULL AND u.status <> 'deleted'
         ORDER BY iv.created_at LIMIT 100",
    )
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, user_id, display_name, submitted_at, previous_attempts)| QueueItem {
                id,
                user_id,
                display_name,
                submitted_at,
                previous_attempts,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PreviousAttempt {
    pub status: String,
    pub reject_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VerificationDetail {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub full_name: Option<String>,
    #[schema(value_type = Option<String>, format = Date)]
    pub date_of_birth: Option<String>,
    pub national_id: Option<String>,
    /// Short-lived links to the sanitised images.
    pub id_document_url: Option<String>,
    pub selfie_url: Option<String>,
    pub previous: Vec<PreviousAttempt>,
}

type DetailRow = (Uuid, String, Option<String>, Option<Vec<u8>>, Option<Vec<u8>>, Option<String>, Option<String>);

#[utoipa::path(
    get, path = "/v1/ops/verifications/{id}", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 200, body = VerificationDetail), (status = 404, body = ErrorBody))
)]
pub async fn detail(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
) -> Result<Json<VerificationDetail>, ApiError> {
    staff.require_reviewer()?;
    let row: Option<DetailRow> = sqlx::query_as(
        "SELECT iv.user_id, iv.status::text, u.full_name, u.date_of_birth, u.national_id, iv.id_document_key, iv.selfie_key
         FROM identity_verifications iv JOIN users u ON u.id = iv.user_id WHERE iv.id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let (user_id, status, full_name, dob, nid, id_key, selfie_key) = row.ok_or_else(ApiError::not_found)?;
    let open = |aad: &str, v: Option<Vec<u8>>| v.map(|v| s.keys.sealer.open_str(aad, &v)).transpose().map_err(ApiError::internal);
    let previous: Vec<(String, Option<String>, OffsetDateTime)> = sqlx::query_as(
        "SELECT status::text, reject_reason, created_at FROM identity_verifications WHERE user_id = $1 AND id <> $2 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .bind(id)
    .fetch_all(&s.db)
    .await?;

    // Log the view before handing out the links.
    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(user_id),
            action: "identity.view",
            target_type: "identity_verification",
            target_id: id.to_string(),
            detail: None,
            ip: Some(ip),
        },
    )
    .await?;
    let exp = (OffsetDateTime::now_utc() + IMAGE_URL_TTL).unix_timestamp();
    Ok(Json(VerificationDetail {
        id,
        user_id,
        status,
        full_name,
        date_of_birth: open("users.date_of_birth", dob)?,
        national_id: open("users.national_id", nid)?,
        id_document_url: id_key.map(|k| s.store.presign_get(&k, exp)),
        selfie_url: selfie_key.map(|k| s.store.presign_get(&k, exp)),
        previous: previous
            .into_iter()
            .map(|(status, reject_reason, created_at)| PreviousAttempt { status, reject_reason, created_at })
            .collect(),
    }))
}

#[utoipa::path(
    post, path = "/v1/ops/verifications/{id}/decision", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Decision,
    responses(
        (status = 200, body = VerificationDetail),
        (status = 409, description = "`not_pending`: already decided, or photos still being checked", body = ErrorBody),
        (status = 422, description = "Rejection without a reason", body = ErrorBody),
    )
)]
pub async fn decide(
    State(s): State<AppState>,
    staff: Staff,
    ip: ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(d): ValidJson<Decision>,
) -> Result<Json<VerificationDetail>, ApiError> {
    staff.require_reviewer()?;
    let reason = d.reason()?;
    let mut tx = s.db.begin().await?;
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE identity_verifications SET status = $2::verification_status, reject_reason = $3, reviewed_by = $4, reviewed_at = now()
         WHERE id = $1 AND status = 'pending' AND id_document_key IS NOT NULL AND selfie_key IS NOT NULL
         RETURNING user_id",
    )
    .bind(id)
    .bind(if d.decision == Verdict::Approve { "approved" } else { "rejected" })
    .bind(reason)
    .bind(staff.id)
    .fetch_optional(&mut *tx)
    .await?;
    let user_id = user_id.ok_or_else(|| ApiError::conflict("not_pending", "This verification isn't waiting for a decision."))?;
    match d.decision {
        Verdict::Approve => {
            sqlx::query(
                "UPDATE users SET verification = 'approved', verified_at = now(), status = CASE WHEN status = 'pending' THEN 'active' ELSE status END
                 WHERE id = $1",
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        Verdict::Reject => {
            sqlx::query("UPDATE users SET verification = 'rejected' WHERE id = $1").bind(user_id).execute(&mut *tx).await?;
        }
    }
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(user_id),
            action: if d.decision == Verdict::Approve { "identity.approve" } else { "identity.reject" },
            target_type: "identity_verification",
            target_id: id.to_string(),
            detail: reason.map(|r| json!({ "reason": r })),
            ip: Some(ip.0),
        },
    )
    .await?;
    tx.commit().await?;
    tracing::info!(verification = %id, decision = ?d.decision, "identity verification decided");
    detail(State(s), staff, ip, Path(id)).await
}
