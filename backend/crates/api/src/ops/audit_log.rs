//! The audit log viewer (PRD O-7), admins only: who did what to whom, newest first, filtered by
//! staff member, person, action or target, 100 at a time. Reading the log is itself audited.

use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::ClientIp,
};

const PAGE: i64 = 100;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
    pub staff_id: Option<Uuid>,
    /// The person the action concerns.
    pub user_id: Option<Uuid>,
    /// An action (`user.suspend`) or a prefix ending in a dot (`user.`).
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    /// Entries older than this id (the last `id` of the previous page).
    pub before: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEntry {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub staff_id: Option<Uuid>,
    pub staff_email: Option<String>,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    #[schema(value_type = Option<Object>)]
    pub detail: Option<Value>,
    pub ip: Option<String>,
}

type Row = (i64, OffsetDateTime, Option<Uuid>, Option<String>, Option<Uuid>, String, String, String, Option<Value>, Option<String>);

#[utoipa::path(
    get, path = "/v1/ops/audit", operation_id = "audit_log", tag = "ops", security(("staff" = [])),
    params(AuditQuery),
    responses(
        (status = 200, description = "Up to 100 entries, newest first; page with `before`", body = Vec<AuditEntry>),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 422, description = "A filter is too long", body = ErrorBody),
    )
)]
pub async fn list(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    staff.require_admin()?;
    for f in [&q.action, &q.target_type, &q.target_id].into_iter().flatten() {
        if f.chars().count() > 100 {
            return Err(ApiError::invalid("Filters are at most 100 characters"));
        }
    }
    // `user.` matches every user action; anything else matches exactly.
    let (action_exact, action_prefix) = match q.action.as_deref() {
        Some(a) if a.ends_with('.') => (None, Some(format!("{}%", a.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")))),
        Some(a) => (Some(a.to_owned()), None),
        None => (None, None),
    };
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.id, a.at, a.staff_id, su.email::text, a.user_id, a.action, a.target_type, a.target_id, a.detail, host(a.ip)
         FROM audit_log a LEFT JOIN staff_users su ON su.id = a.staff_id
         WHERE ($1::uuid IS NULL OR a.staff_id = $1) AND ($2::uuid IS NULL OR a.user_id = $2)
           AND ($3::text IS NULL OR a.action = $3) AND ($4::text IS NULL OR a.action LIKE $4)
           AND ($5::text IS NULL OR a.target_type = $5) AND ($6::text IS NULL OR a.target_id = $6)
           AND ($7::int8 IS NULL OR a.id < $7)
         ORDER BY a.id DESC LIMIT $8",
    )
    .bind(q.staff_id)
    .bind(q.user_id)
    .bind(action_exact)
    .bind(action_prefix)
    .bind(&q.target_type)
    .bind(&q.target_id)
    .bind(q.before)
    .bind(PAGE)
    .fetch_all(&s.db)
    .await?;
    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: q.user_id,
            action: "audit.view",
            target_type: "audit_log",
            target_id: "query".into(),
            detail: Some(json!({ "staff_id": q.staff_id, "action": q.action, "target_type": q.target_type, "results": rows.len() })),
            ip: Some(ip),
        },
    )
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, at, staff_id, staff_email, user_id, action, target_type, target_id, detail, ip)| AuditEntry {
                id,
                at,
                staff_id,
                staff_email,
                user_id,
                action,
                target_type,
                target_id,
                detail,
                ip,
            })
            .collect(),
    ))
}
