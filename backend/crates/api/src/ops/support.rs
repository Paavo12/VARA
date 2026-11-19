//! Support tools (PRD O-2, O-6) for support agents and admins: find a user, see their recent
//! trips and reports, suspend or reinstate them with a reason, adjust priority points, and work
//! through the reports that are not safety incidents (lost items, fares, other).
//!
//! Every view of a person and every change is in the audit log. A suspension takes effect on
//! the next request (sessions are checked each time) and takes a driver offline; a trip already
//! in progress is left to finish, because stopping a car mid-trip is less safe.
//!
//! Admins also grant Map Expert status (a flat 15 % fee on every tier) to a driver until a date.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use fuber_common::validate::PlainText;
use fuber_domain::priority;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UserSearch {
    /// A phone number (`+264…` or `081…`), or part of a name (at least 2 characters).
    pub q: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserItem {
    pub user_id: Uuid,
    pub full_name: Option<String>,
    pub display_name: Option<String>,
    pub phone: Option<String>,
    /// `pending`, `active`, `suspended` or `deleted`.
    pub status: String,
    pub is_rider: bool,
    pub is_driver: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

type UserRow = (Uuid, Option<String>, Option<String>, Option<String>, String, bool, bool, OffsetDateTime);

const SELECT_USER: &str = "SELECT u.id, u.full_name, u.display_name, u.phone_e164, u.status::text,
        EXISTS (SELECT 1 FROM rider_profiles rp WHERE rp.user_id = u.id),
        EXISTS (SELECT 1 FROM driver_profiles dp WHERE dp.user_id = u.id), u.created_at
    FROM users u";

fn user_item((user_id, full_name, display_name, phone, status, is_rider, is_driver, created_at): UserRow) -> UserItem {
    UserItem { user_id, full_name, display_name, phone, status, is_rider, is_driver, created_at }
}

/// `081 234 5678` or `+264 81 234 5678` → `+264812345678`, when it looks like a number.
fn as_phone(q: &str) -> Option<String> {
    let digits: String = q.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    if digits.len() < 9 || q.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    Some(match digits.strip_prefix('0') {
        Some(rest) => format!("+264{rest}"),
        None if digits.starts_with('+') => digits,
        None => format!("+{digits}"),
    })
}

#[utoipa::path(
    get, path = "/v1/ops/users", operation_id = "user_search", tag = "ops", security(("staff" = [])),
    params(UserSearch),
    responses(
        (status = 200, description = "Up to 50 matches: the exact phone number, or names containing the text", body = Vec<UserItem>),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 422, description = "Search text too short", body = ErrorBody),
    )
)]
pub async fn search(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Query(q): Query<UserSearch>,
) -> Result<Json<Vec<UserItem>>, ApiError> {
    staff.require_support()?;
    let text = q.q.trim();
    if text.chars().count() < 2 || text.chars().count() > 80 {
        return Err(ApiError::invalid("Search for a phone number or at least 2 letters of a name."));
    }
    let rows: Vec<UserRow> = match as_phone(text) {
        Some(phone) => sqlx::query_as(&format!("{SELECT_USER} WHERE u.phone_e164 = $1")).bind(phone).fetch_all(&s.db).await?,
        None => {
            let like = format!("%{}%", text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            sqlx::query_as(&format!(
                "{SELECT_USER} WHERE u.full_name ILIKE $1 OR u.display_name ILIKE $1 ORDER BY u.created_at DESC LIMIT 50"
            ))
            .bind(like)
            .fetch_all(&s.db)
            .await?
        }
    };
    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: None,
            action: "user.search",
            target_type: "user",
            target_id: "search".into(),
            detail: Some(json!({ "results": rows.len() })),
            ip: Some(ip),
        },
    )
    .await?;
    Ok(Json(rows.into_iter().map(user_item).collect()))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserTrip {
    pub ride_id: Uuid,
    pub code: String,
    /// `rider` or `driver`: her side of this trip.
    pub as_party: String,
    pub state: String,
    pub pickup_address: String,
    pub dropoff_address: String,
    pub fare: fuber_domain::Cents,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
}

/// Ride, code, her side, state, pickup, drop-off, fare, requested at.
type TripRow = (Uuid, String, String, String, String, String, i64, OffsetDateTime);

#[derive(Debug, Serialize, ToSchema)]
pub struct UserReport {
    pub id: Uuid,
    pub ride_id: Option<Uuid>,
    pub category: String,
    pub text: String,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserDetail {
    pub user: UserItem,
    pub suspended_reason: Option<String>,
    pub verification: String,
    pub rider_priority: Option<i32>,
    pub driver_priority: Option<i32>,
    /// A driver's Map Expert status (flat 15 % fee) ends then; in the past means it lapsed.
    #[serde(with = "time::serde::rfc3339::option")]
    pub map_expert_until: Option<OffsetDateTime>,
    /// The last 20 trips on either side.
    pub trips: Vec<UserTrip>,
    /// Reports she made, newest first.
    pub reports: Vec<UserReport>,
}

#[utoipa::path(
    get, path = "/v1/ops/users/{id}", operation_id = "user_detail", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "The person, her last trips and her reports (the view is audited)", body = UserDetail),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 404, description = "No such user", body = ErrorBody),
    )
)]
pub async fn detail(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
) -> Result<Json<UserDetail>, ApiError> {
    staff.require_support()?;
    let row: Option<UserRow> = sqlx::query_as(&format!("{SELECT_USER} WHERE u.id = $1")).bind(id).fetch_optional(&s.db).await?;
    let user = row.map(user_item).ok_or_else(ApiError::not_found)?;
    type Extra = (Option<String>, String, Option<i32>, Option<i32>, Option<OffsetDateTime>);
    let (suspended_reason, verification, rider_priority, driver_priority, map_expert_until): Extra = sqlx::query_as(
        "SELECT u.suspended_reason, u.verification::text,
                (SELECT priority_points FROM rider_profiles WHERE user_id = u.id),
                (SELECT priority_points FROM driver_profiles WHERE user_id = u.id),
                (SELECT map_expert_until FROM driver_profiles WHERE user_id = u.id)
         FROM users u WHERE u.id = $1",
    )
    .bind(id)
    .fetch_one(&s.db)
    .await?;
    let trips: Vec<TripRow> = sqlx::query_as(
        "SELECT id, code, CASE WHEN rider_id = $1 THEN 'rider' ELSE 'driver' END, state::text, pickup_address, dropoff_address,
                fare_cents, requested_at
         FROM rides WHERE rider_id = $1 OR driver_id = $1 ORDER BY requested_at DESC LIMIT 20",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    let reports: Vec<(Uuid, Option<Uuid>, String, String, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT id, ride_id, category, body, status, created_at FROM issue_reports WHERE reporter_id = $1 ORDER BY created_at DESC LIMIT 50",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "user.view",
            target_type: "user",
            target_id: id.to_string(),
            detail: None,
            ip: Some(ip),
        },
    )
    .await?;
    Ok(Json(UserDetail {
        user,
        suspended_reason,
        verification,
        rider_priority,
        driver_priority,
        map_expert_until,
        trips: trips
            .into_iter()
            .map(|(ride_id, code, as_party, state, pickup_address, dropoff_address, fare, requested_at)| UserTrip {
                ride_id,
                code,
                as_party,
                state,
                pickup_address,
                dropoff_address,
                fare: fuber_domain::Cents(fare),
                requested_at,
            })
            .collect(),
        reports: reports
            .into_iter()
            .map(|(id, ride_id, category, text, status, created_at)| UserReport { id, ride_id, category, text, status, created_at })
            .collect(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    /// Why, for the audit log and (for a suspension) the user's record.
    #[schema(value_type = String)]
    pub reason: PlainText<500>,
}

#[utoipa::path(
    post, path = "/v1/ops/users/{id}/suspend", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Reason,
    responses(
        (status = 204, description = "Suspended: signed out of every request from now on; a driver is taken offline"),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 404, description = "No such user", body = ErrorBody),
        (status = 409, description = "Already suspended, or deleted (`not_active`)", body = ErrorBody),
    )
)]
pub async fn suspend(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<Reason>,
) -> Result<StatusCode, ApiError> {
    staff.require_support()?;
    let mut tx = s.db.begin().await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status::text FROM users WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?;
    match status.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("suspended" | "deleted") => return Err(ApiError::conflict("not_active", "This account is already suspended or deleted.")),
        _ => {}
    }
    sqlx::query("UPDATE users SET status = 'suspended', suspended_reason = $2 WHERE id = $1")
        .bind(id)
        .bind(body.reason.as_str())
        .execute(&mut *tx)
        .await?;
    // Off the matching pool at once; an open shift ends.
    sqlx::query("DELETE FROM driver_presence WHERE driver_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE driver_shifts SET ended_at = now() WHERE driver_id = $1 AND ended_at IS NULL").bind(id).execute(&mut *tx).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "user.suspend",
            target_type: "user",
            target_id: id.to_string(),
            detail: Some(json!({ "reason": body.reason.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    tracing::warn!(security_event = "user_suspended", user_id = %id, staff_id = %staff.id, "user suspended");
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/v1/ops/users/{id}/reinstate", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Reason,
    responses(
        (status = 204, description = "Reinstated: active again if verified, else back to pending"),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 404, description = "No such user", body = ErrorBody),
        (status = 409, description = "Not suspended (`not_suspended`)", body = ErrorBody),
    )
)]
pub async fn reinstate(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<Reason>,
) -> Result<StatusCode, ApiError> {
    staff.require_support()?;
    let mut tx = s.db.begin().await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status::text FROM users WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?;
    match status.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("suspended") => {}
        Some(_) => return Err(ApiError::conflict("not_suspended", "This account is not suspended.")),
    }
    sqlx::query(
        "UPDATE users SET status = CASE WHEN verification = 'approved' THEN 'active' ELSE 'pending' END::user_status,
                          suspended_reason = NULL
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "user.reinstate",
            target_type: "user",
            target_id: id.to_string(),
            detail: Some(json!({ "reason": body.reason.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MapExpertGrant {
    /// The status lapses at this time; at most a year ahead.
    #[serde(with = "time::serde::rfc3339")]
    pub until: OffsetDateTime,
    /// Why (the zones she maps, who asked), for the audit log.
    #[schema(value_type = String)]
    pub reason: PlainText<500>,
}

#[utoipa::path(
    post, path = "/v1/ops/users/{id}/map-expert", operation_id = "grant_map_expert", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = MapExpertGrant,
    responses(
        (status = 204, description = "Rides she accepts until then pay the flat 15 % Map Expert fee; granting again moves the date"),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 404, description = "No such driver", body = ErrorBody),
        (status = 422, description = "The date is not in the future, or more than a year ahead", body = ErrorBody),
    )
)]
pub async fn grant_map_expert(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<MapExpertGrant>,
) -> Result<StatusCode, ApiError> {
    staff.require_admin()?;
    let now = OffsetDateTime::now_utc();
    if body.until <= now || body.until > now + time::Duration::days(366) {
        return Err(ApiError::invalid("The end date must be in the future and at most a year ahead."));
    }
    let mut tx = s.db.begin().await?;
    let before: Option<Option<OffsetDateTime>> =
        sqlx::query_scalar("SELECT map_expert_until FROM driver_profiles WHERE user_id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(before) = before else { return Err(ApiError::not_found()) };
    sqlx::query("UPDATE driver_profiles SET map_expert_until = $2 WHERE user_id = $1").bind(id).bind(body.until).execute(&mut *tx).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "driver.map_expert_grant",
            target_type: "user",
            target_id: id.to_string(),
            detail: Some(json!({
                "until": body.until.unix_timestamp(),
                "before": before.map(|t| t.unix_timestamp()),
                "reason": body.reason.as_str(),
            })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/v1/ops/users/{id}/map-expert/end", operation_id = "end_map_expert", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Reason,
    responses(
        (status = 204, description = "Ended now: rides she accepts from now on pay the tier's fee"),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 404, description = "No such driver", body = ErrorBody),
        (status = 409, description = "She is not a Map Expert (`not_map_expert`)", body = ErrorBody),
    )
)]
pub async fn end_map_expert(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<Reason>,
) -> Result<StatusCode, ApiError> {
    staff.require_admin()?;
    let mut tx = s.db.begin().await?;
    let active: Option<bool> =
        sqlx::query_scalar("SELECT coalesce(map_expert_until > now(), false) FROM driver_profiles WHERE user_id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    match active {
        None => return Err(ApiError::not_found()),
        Some(false) => return Err(ApiError::conflict("not_map_expert", "This driver is not a Map Expert.")),
        Some(true) => {}
    }
    sqlx::query("UPDATE driver_profiles SET map_expert_until = NULL WHERE user_id = $1").bind(id).execute(&mut *tx).await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "driver.map_expert_end",
            target_type: "user",
            target_id: id.to_string(),
            detail: Some(json!({ "reason": body.reason.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Rider,
    Driver,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorityAdjustment {
    pub role: Side,
    /// Points to add (negative to remove), −50 to 50. The balance stays within 0–150.
    pub delta: i32,
    #[schema(value_type = String)]
    pub reason: PlainText<500>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PriorityBalance {
    pub points: i32,
}

#[utoipa::path(
    post, path = "/v1/ops/users/{id}/priority", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = PriorityAdjustment,
    responses(
        (status = 200, description = "Adjusted (clamped to 0–150) and recorded in the priority ledger", body = PriorityBalance),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 404, description = "No such rider or driver", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
    )
)]
pub async fn adjust_priority(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<PriorityAdjustment>,
) -> Result<Json<PriorityBalance>, ApiError> {
    staff.require_support()?;
    if body.delta == 0 || !(-50..=50).contains(&body.delta) {
        return Err(ApiError::invalid("delta must be between -50 and 50, and not 0"));
    }
    let (table, role) = match body.role {
        Side::Rider => ("rider_profiles", "rider"),
        Side::Driver => ("driver_profiles", "driver"),
    };
    let mut tx = s.db.begin().await?;
    let before: Option<i32> = sqlx::query_scalar(&format!("SELECT priority_points FROM {table} WHERE user_id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let before = before.ok_or_else(ApiError::not_found)?;
    let after = (before + body.delta).clamp(0, priority::MAX);
    sqlx::query(&format!("UPDATE {table} SET priority_points = $2 WHERE user_id = $1")).bind(id).bind(after).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO priority_ledger (user_id, role, delta, reason, balance, staff_id) VALUES ($1, $2, $3, 'staff_adjustment', $4, $5)",
    )
    .bind(id)
    .bind(role)
    .bind(after - before)
    .bind(after)
    .bind(staff.id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(id),
            action: "priority.adjust",
            target_type: "user",
            target_id: id.to_string(),
            detail: Some(json!({ "role": role, "from": before, "to": after, "reason": body.reason.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(PriorityBalance { points: after }))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportQuery {
    /// `open` (default), `in_progress` or `resolved`.
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SupportReport {
    pub id: Uuid,
    pub ride_id: Option<Uuid>,
    pub ride_code: Option<String>,
    pub category: String,
    pub text: String,
    pub status: String,
    /// The safety desk has it too.
    pub incident_id: Option<Uuid>,
    pub reporter_id: Uuid,
    pub reporter_name: Option<String>,
    pub reporter_phone: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

type ReportRow =
    (Uuid, Option<Uuid>, Option<String>, String, String, String, Option<Uuid>, Uuid, Option<String>, Option<String>, OffsetDateTime);

#[utoipa::path(
    get, path = "/v1/ops/reports", operation_id = "report_queue", tag = "ops", security(("staff" = [])),
    params(ReportQuery),
    responses(
        (status = 200, description = "Reported issues, oldest first (open) or newest first", body = Vec<SupportReport>),
        (status = 403, description = "Support and admins only", body = ErrorBody),
    )
)]
pub async fn reports(State(s): State<AppState>, staff: Staff, Query(q): Query<ReportQuery>) -> Result<Json<Vec<SupportReport>>, ApiError> {
    staff.require_support()?;
    let status = q.status.unwrap_or_else(|| "open".into());
    if !["open", "in_progress", "resolved"].contains(&status.as_str()) {
        return Err(ApiError::invalid("status must be open, in_progress or resolved"));
    }
    let rows: Vec<ReportRow> = sqlx::query_as(
        "SELECT ir.id, ir.ride_id, r.code, ir.category, ir.body, ir.status, ir.incident_id, ir.reporter_id, u.display_name, u.phone_e164,
                ir.created_at
         FROM issue_reports ir JOIN users u ON u.id = ir.reporter_id LEFT JOIN rides r ON r.id = ir.ride_id
         WHERE ir.status = $1
         ORDER BY CASE WHEN $1 = 'open' THEN ir.created_at END, ir.created_at DESC LIMIT 200",
    )
    .bind(&status)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, ride_id, ride_code, category, text, status, incident_id, reporter_id, reporter_name, reporter_phone, created_at)| {
                SupportReport {
                    id,
                    ride_id,
                    ride_code,
                    category,
                    text,
                    status,
                    incident_id,
                    reporter_id,
                    reporter_name,
                    reporter_phone,
                    created_at,
                }
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportStatus {
    /// `in_progress` or `resolved`.
    pub status: String,
    /// What was done, for the audit log.
    #[schema(value_type = Option<String>)]
    pub note: Option<PlainText<500>>,
}

#[utoipa::path(
    post, path = "/v1/ops/reports/{id}/status", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = ReportStatus,
    responses(
        (status = 204, description = "Updated"),
        (status = 403, description = "Support and admins only", body = ErrorBody),
        (status = 404, description = "No such report", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
    )
)]
pub async fn set_report_status(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<ReportStatus>,
) -> Result<StatusCode, ApiError> {
    staff.require_support()?;
    if body.status != "in_progress" && body.status != "resolved" {
        return Err(ApiError::invalid("status must be in_progress or resolved"));
    }
    let mut tx = s.db.begin().await?;
    let reporter: Option<Uuid> = sqlx::query_scalar("UPDATE issue_reports SET status = $2 WHERE id = $1 RETURNING reporter_id")
        .bind(id)
        .bind(&body.status)
        .fetch_optional(&mut *tx)
        .await?;
    let reporter = reporter.ok_or_else(ApiError::not_found)?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(reporter),
            action: "report.status",
            target_type: "issue_report",
            target_id: id.to_string(),
            detail: Some(json!({ "status": body.status, "note": body.note.as_ref().map(PlainText::as_str) })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::as_phone;

    #[test]
    fn phone_searches_are_normalised() {
        assert_eq!(as_phone("081 234 5678").as_deref(), Some("+264812345678"));
        assert_eq!(as_phone("+264 81 234 5678").as_deref(), Some("+264812345678"));
        assert_eq!(as_phone("Ndeshi"), None);
        assert_eq!(as_phone("12"), None);
    }
}
