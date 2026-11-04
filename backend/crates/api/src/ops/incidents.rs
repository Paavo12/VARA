//! The safety desk's incident queue (PRD S-1, O-3; App Flow §9): SOS, PIN lockouts and, later,
//! RideCheck and issue reports. The Phase 5 console polls this and sounds the alarm.

use axum::{Json, extract::State};
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Staff;
use crate::{AppState, error::ApiError};

#[derive(Debug, Serialize, ToSchema)]
pub struct IncidentItem {
    pub id: Uuid,
    /// `sos`, `pin_lockout`, `ridecheck_deviation`, `ridecheck_stop` or `report`.
    pub kind: String,
    /// `open`, `claimed` or `escalated`.
    pub status: String,
    pub ride_id: Option<Uuid>,
    /// e.g. `FU-4123`.
    pub ride_code: Option<String>,
    /// Who pressed SOS; absent for incidents the system raised.
    pub raised_by: Option<Person>,
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
                ST_Y(i.location::geometry), ST_X(i.location::geometry), i.created_at
         FROM safety_incidents i LEFT JOIN rides r ON r.id = i.ride_id LEFT JOIN users u ON u.id = i.raised_by
         WHERE i.status <> 'closed'
         ORDER BY (i.kind = 'sos') DESC, i.created_at DESC LIMIT 200",
    )
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, kind, status, ride_id, ride_code, user_id, display_name, phone, lat, lon, created_at)| IncidentItem {
                id,
                kind,
                status,
                ride_id,
                ride_code,
                raised_by: user_id.zip(phone).map(|(user_id, phone)| Person { user_id, display_name, phone }),
                lat,
                lon,
                created_at,
            })
            .collect(),
    ))
}
