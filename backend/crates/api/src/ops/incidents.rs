//! The safety desk's incident queue (PRD S-1, O-3; App Flow §9): SOS, PIN lockouts and, later,
//! RideCheck and issue reports. The Phase 5 console polls this and sounds the alarm. Closing a
//! PIN lockout is what releases the driver for new requests (PRD T-3) and ends the blocked trip.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_common::validate::PlainText;
use fuber_db::rides;
use fuber_domain::{Party, RideEvent, RideState};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
    routes::rides::ride_error,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct IncidentItem {
    pub id: Uuid,
    /// `sos`, `pin_lockout`, `ridecheck_deviation`, `ridecheck_stop` or `report`.
    pub kind: String,
    /// `open`, `claimed` or `escalated`.
    pub status: String,
    pub ride_id: Option<Uuid>,
    /// e.g. `VA-4123`.
    pub ride_code: Option<String>,
    /// Who pressed SOS; absent for incidents the system raised.
    pub raised_by: Option<Person>,
    /// The ride's driver: for a PIN lockout, who is held until this incident is closed.
    pub driver: Option<Person>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Person {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub phone: String,
}

type Row = (
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<f64>,
    Option<f64>,
    OffsetDateTime,
);

#[utoipa::path(
    get, path = "/v1/ops/incidents", operation_id = "incident_queue", tag = "ops", security(("staff" = [])),
    responses((status = 200, description = "Incidents not yet closed: SOS first, then newest first", body = Vec<IncidentItem>))
)]
pub async fn queue(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<IncidentItem>>, ApiError> {
    staff.require_safety()?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT i.id, i.kind::text, i.status::text, i.ride_id, r.code, u.id, u.display_name, u.phone_e164,
                d.id, d.display_name, d.phone_e164,
                ST_Y(i.location::geometry), ST_X(i.location::geometry), i.created_at
         FROM safety_incidents i LEFT JOIN rides r ON r.id = i.ride_id LEFT JOIN users u ON u.id = i.raised_by LEFT JOIN users d ON d.id = r.driver_id
         WHERE i.status <> 'closed'
         ORDER BY (i.kind = 'sos') DESC, i.created_at DESC LIMIT 200",
    )
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    kind,
                    status,
                    ride_id,
                    ride_code,
                    user_id,
                    display_name,
                    phone,
                    driver_id,
                    driver_name,
                    driver_phone,
                    lat,
                    lon,
                    created_at,
                )| IncidentItem {
                    id,
                    kind,
                    status,
                    ride_id,
                    ride_code,
                    raised_by: user_id.zip(phone).map(|(user_id, phone)| Person { user_id, display_name, phone }),
                    driver: driver_id.zip(driver_phone).map(|(user_id, phone)| Person { user_id, display_name: driver_name, phone }),
                    lat,
                    lon,
                    created_at,
                },
            )
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct IncidentMessage {
    pub id: i64,
    /// `rider` or `driver`.
    pub sender: String,
    /// The driver on the ride at the time. It changes if one cancelled and another took over.
    pub driver_id: Uuid,
    /// Plain text: render it as text, never as HTML.
    pub text: String,
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,
}

#[utoipa::path(
    get, path = "/v1/ops/incidents/{id}/messages", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Every message on the incident's ride, oldest first, across all its drivers (empty without a ride). Each view goes in the audit log", body = Vec<IncidentMessage>),
        (status = 403, description = "Not safety staff", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
    )
)]
pub async fn messages(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<IncidentMessage>>, ApiError> {
    staff.require_safety()?;
    let ride_id: Option<Uuid> = sqlx::query_scalar::<_, Option<Uuid>>("SELECT ride_id FROM safety_incidents WHERE id = $1")
        .bind(id)
        .fetch_optional(&s.db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    // Logged before the messages are read out, so no staff member can read a chat unrecorded.
    audit::record(
        &s.db,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: None,
            action: "chat.view",
            target_type: "incident",
            target_id: id.to_string(),
            detail: Some(json!({ "ride_id": ride_id })),
            ip: Some(ip),
        },
    )
    .await?;
    let rows = sqlx::query_as(
        "SELECT id, sender::text AS sender, driver_id, body AS text, sent_at FROM ride_messages WHERE ride_id = $1 ORDER BY id",
    )
    .bind(ride_id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(rows))
}

/// What the safety desk found, kept on the incident and in the audit log.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseIncident {
    #[schema(value_type = String, example = "Rider's phone was off; spoke to both, no safety concern.")]
    pub outcome: PlainText<500>,
}

#[utoipa::path(
    post, path = "/v1/ops/incidents/{id}/close", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = CloseIncident,
    responses(
        (status = 204, description = "Closed. For a PIN lockout the blocked trip is cancelled and the driver can get requests again"),
        (status = 422, description = "No outcome given", body = ErrorBody),
        (status = 403, description = "Not safety staff", body = ErrorBody),
        (status = 404, description = "No such incident, or already closed", body = ErrorBody),
    )
)]
pub async fn close(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<CloseIncident>,
) -> Result<StatusCode, ApiError> {
    staff.require_safety()?;
    let mut tx = s.db.begin().await?;
    let row: Option<(String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "UPDATE safety_incidents i SET status = 'closed', outcome = $2, closed_at = now()
         WHERE i.id = $1 AND i.status <> 'closed'
         RETURNING i.kind::text, i.ride_id, (SELECT r.driver_id FROM rides r WHERE r.id = i.ride_id)",
    )
    .bind(id)
    .bind(body.outcome.as_str())
    .fetch_optional(&mut *tx)
    .await?;
    let (kind, ride_id, driver_id) = row.ok_or_else(ApiError::not_found)?;
    if let Some(ride_id) = ride_id.filter(|_| kind == "pin_lockout")
        && rides::lock(&mut tx, ride_id).await.map_err(ride_error)? == RideState::Blocked
    {
        rides::transition(&mut tx, ride_id, RideEvent::SafetyDeskClosed, Party::Staff, Some(staff.id), None).await.map_err(ride_error)?;
    }
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: driver_id.filter(|_| kind == "pin_lockout"),
            action: "incident.close",
            target_type: "incident",
            target_id: id.to_string(),
            detail: Some(json!({ "kind": kind, "outcome": body.outcome.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
