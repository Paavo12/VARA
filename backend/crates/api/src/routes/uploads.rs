//! File uploads for identity photos and driver documents (TRD §7.2).
//!
//! 1. `POST /uploads` declares purpose, type and size and returns a signed URL valid for 15 min.
//! 2. The app sends the bytes to that URL (object storage, never the JSON API).
//! 3. `POST /uploads/{id}/complete` queues the worker, which checks magic bytes, scans for
//!    viruses, re-encodes images and marks the upload `clean` or `rejected`.
//! 4. The upload ID is then attached with `POST /me/verification` or `POST /driver/documents`.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_common::validate::Bounded;
use fuber_db::jobs;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
};

pub const UPLOAD_URL_TTL: Duration = Duration::minutes(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Identity,
    Selfie,
    Licence,
    Insurance,
    PoliceClearance,
    TransportPermit,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentKind::Identity => "identity",
            DocumentKind::Selfie => "selfie",
            DocumentKind::Licence => "licence",
            DocumentKind::Insurance => "insurance",
            DocumentKind::PoliceClearance => "police_clearance",
            DocumentKind::TransportPermit => "transport_permit",
        }
    }

    /// Identity and selfie must be photos; paper documents may also be PDFs.
    fn allows(self, t: ContentType) -> bool {
        t != ContentType::Pdf || !matches!(self, DocumentKind::Identity | DocumentKind::Selfie)
    }

    fn driver_only(self) -> bool {
        !matches!(self, DocumentKind::Identity | DocumentKind::Selfie)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ContentType {
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "application/pdf")]
    Pdf,
}

impl ContentType {
    fn as_str(self) -> &'static str {
        match self {
            ContentType::Jpeg => "image/jpeg",
            ContentType::Png => "image/png",
            ContentType::Pdf => "application/pdf",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUpload {
    pub purpose: DocumentKind,
    pub content_type: ContentType,
    /// Exact size of the file in bytes, at most 10 MB.
    #[schema(value_type = i64, minimum = 1, maximum = 10485760)]
    pub size_bytes: Bounded<1, 10_485_760>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadTarget {
    pub upload_id: Uuid,
    /// Send the file with this method to `url`, with exactly these headers.
    pub method: &'static str,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[utoipa::path(
    post, path = "/v1/uploads", tag = "uploads", security(("bearer" = [])),
    request_body = CreateUpload,
    responses(
        (status = 201, body = UploadTarget),
        (status = 403, description = "Driver documents can only be uploaded from the driver app", body = ErrorBody),
        (status = 422, body = ErrorBody),
        (status = 429, description = "20 uploads per hour", body = ErrorBody),
    )
)]
pub async fn create(
    State(s): State<AppState>,
    user: AuthUser,
    ValidJson(req): ValidJson<CreateUpload>,
) -> Result<(StatusCode, Json<UploadTarget>), ApiError> {
    if req.purpose.driver_only() && user.app != App::Driver {
        return Err(ApiError::forbidden("wrong_app", "Driver documents are uploaded from the VARA driver app."));
    }
    if !req.purpose.allows(req.content_type) {
        return Err(ApiError::invalid("this photo must be a JPEG or PNG image"));
    }
    s.limiter.check("uploads", &user.user_id.to_string(), s.limits.uploads).await?;
    let expires_at = OffsetDateTime::now_utc() + UPLOAD_URL_TTL;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO uploads (user_id, purpose, declared_type, declared_size, url_expires_at)
         VALUES ($1, $2::document_kind, $3, $4, $5) RETURNING id",
    )
    .bind(user.user_id)
    .bind(req.purpose.as_str())
    .bind(req.content_type.as_str())
    .bind(req.size_bytes.get() as i32)
    .bind(expires_at)
    .fetch_one(&s.db)
    .await?;
    let p = s
        .store
        .presign_put(&quarantine_key(id), req.content_type.as_str(), req.size_bytes.get() as u64, expires_at.unix_timestamp())
        .await
        .map_err(ApiError::internal)?;
    let headers = p.headers.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    Ok((StatusCode::CREATED, Json(UploadTarget { upload_id: id, method: p.method, url: p.url, headers, expires_at })))
}

/// Where the raw, unchecked bytes land. Nothing but the worker ever reads from here.
pub fn quarantine_key(id: Uuid) -> String {
    format!("quarantine/{id}")
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadStatus {
    pub upload_id: Uuid,
    pub purpose: String,
    /// `awaiting_upload`, `processing`, `clean` or `rejected`.
    pub status: String,
    /// Why the file was refused, written for the user.
    pub reject_reason: Option<String>,
}

async fn load(s: &AppState, user: &AuthUser, id: Uuid) -> Result<UploadStatus, ApiError> {
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT purpose::text, status::text, reject_reason FROM uploads WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user.user_id)
            .fetch_optional(&s.db)
            .await?;
    let (purpose, status, reject_reason) = row.ok_or_else(ApiError::not_found)?;
    Ok(UploadStatus { upload_id: id, purpose, status, reject_reason })
}

#[utoipa::path(
    get, path = "/v1/uploads/{id}", tag = "uploads", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 200, body = UploadStatus), (status = 404, body = ErrorBody))
)]
pub async fn status(State(s): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> Result<Json<UploadStatus>, ApiError> {
    load(&s, &user, id).await.map(Json)
}

#[utoipa::path(
    post, path = "/v1/uploads/{id}/complete", tag = "uploads", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 202, description = "Queued for checking", body = UploadStatus),
        (status = 404, body = ErrorBody),
        (status = 409, description = "`upload_expired` or already completed", body = ErrorBody),
    )
)]
pub async fn complete(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<UploadStatus>), ApiError> {
    let mut tx = s.db.begin().await?;
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT status::text, url_expires_at + interval '5 minutes' > now() FROM uploads WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, in_time) = row.ok_or_else(ApiError::not_found)?;
    if status != "awaiting_upload" {
        return Err(ApiError::conflict("upload_already_completed", "This upload was already completed."));
    }
    if !in_time {
        return Err(ApiError::conflict("upload_expired", "This upload expired. Please choose the file again."));
    }
    sqlx::query("UPDATE uploads SET status = 'processing' WHERE id = $1").bind(id).execute(&mut *tx).await?;
    jobs::enqueue(&mut *tx, jobs::kind::PROCESS_UPLOAD, json!({ "upload_id": id }), None, Some(&format!("upload:{id}"))).await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(load(&s, &user, id).await?)))
}
