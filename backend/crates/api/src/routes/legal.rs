//! Legal documents and consent (PRD A-7). Consent is recorded per document version with the
//! time, IP and app version; a newer version that requires re-acceptance shows up again in
//! `pending_consents` on `GET /me`.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::{AppVersion, ClientIp, ValidJson},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegalKind {
    Terms,
    Privacy,
    Cookies,
    DriverAgreement,
}

/// Documents each app's users must have accepted.
pub fn required_kinds(app: App) -> &'static [&'static str] {
    match app {
        App::Rider => &["terms", "privacy"],
        App::Driver => &["terms", "privacy", "driver_agreement"],
    }
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LegalDocument {
    pub id: Uuid,
    pub kind: String,
    pub version: String,
    pub url: String,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: OffsetDateTime,
}

const LATEST: &str = "SELECT DISTINCT ON (kind) id, kind, version, url, published_at, requires_reacceptance
                      FROM legal_documents WHERE published_at <= now() ORDER BY kind, published_at DESC";

#[utoipa::path(
    get, path = "/v1/legal/{kind}/latest", tag = "legal",
    params(("kind" = LegalKind, Path)),
    responses((status = 200, body = LegalDocument), (status = 404, description = "Not published yet", body = ErrorBody))
)]
pub async fn latest(State(s): State<AppState>, Path(kind): Path<LegalKind>) -> Result<Json<LegalDocument>, ApiError> {
    let kind = serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    let doc = sqlx::query_as(&format!("SELECT id, kind, version, url, published_at FROM ({LATEST}) l WHERE kind = $1"))
        .bind(kind)
        .fetch_optional(&s.db)
        .await?;
    doc.map(Json).ok_or_else(ApiError::not_found)
}

/// Latest required documents this user has not accepted (or must re-accept).
pub async fn pending_consents<'e>(db: impl PgExecutor<'e>, user_id: Uuid, app: App) -> Result<Vec<LegalDocument>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT l.id, l.kind, l.version, l.url, l.published_at FROM ({LATEST}) l
         WHERE l.kind = ANY($2)
           AND NOT EXISTS (
             SELECT 1 FROM consents c JOIN legal_documents d ON d.id = c.document_id
             WHERE c.user_id = $1 AND d.kind = l.kind AND (d.id = l.id OR NOT l.requires_reacceptance))
         ORDER BY l.kind"
    ))
    .bind(user_id)
    .bind(required_kinds(app))
    .fetch_all(db)
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsentRequest {
    /// IDs from `pending_consents` on `GET /me`; each must be the latest version of its kind.
    pub document_ids: Vec<Uuid>,
}

#[utoipa::path(
    post, path = "/v1/consents", tag = "legal",
    security(("bearer" = [])),
    request_body = ConsentRequest,
    responses(
        (status = 204, description = "Consent recorded"),
        (status = 422, description = "Unknown or outdated document", body = ErrorBody),
    )
)]
pub async fn accept(
    State(s): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    AppVersion(app_version): AppVersion,
    ValidJson(req): ValidJson<ConsentRequest>,
) -> Result<StatusCode, ApiError> {
    if req.document_ids.is_empty() || req.document_ids.len() > 4 {
        return Err(ApiError::invalid("document_ids must list 1 to 4 documents"));
    }
    let current: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM ({LATEST}) l WHERE l.id = ANY($1)"))
        .bind(&req.document_ids)
        .fetch_one(&s.db)
        .await?;
    if current != req.document_ids.len() as i64 {
        return Err(ApiError::invalid("a document is unknown or no longer the latest version"));
    }
    sqlx::query(
        "INSERT INTO consents (user_id, document_id, ip, app_version)
         SELECT $1, d, $3::inet, $4 FROM unnest($2::uuid[]) AS d ON CONFLICT DO NOTHING",
    )
    .bind(user.user_id)
    .bind(&req.document_ids)
    .bind(ip.to_string())
    .bind(app_version)
    .execute(&s.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
