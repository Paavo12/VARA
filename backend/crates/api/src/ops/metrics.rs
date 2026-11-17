//! Reporting (PRD O-8): counts over the last 1–90 days and right now, for any staff role.
//! Aggregates only: no names, numbers or places.

use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::Staff;
use crate::{
    AppState,
    error::{ApiError, ErrorBody},
};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MetricsQuery {
    /// Days to count back from now, 1–90 (default 7).
    #[serde(default)]
    pub days: Option<i32>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Metrics {
    pub days: i32,
    /// Rides requested in the period (scheduled ones by their request time).
    pub requested: i64,
    pub completed: i64,
    pub cancelled_by_rider: i64,
    pub cancelled_by_driver: i64,
    /// Requests that found no driver at least once.
    pub no_drivers: i64,
    /// Completed ÷ requested, 0–1 (absent with no requests).
    pub completion_rate: Option<f64>,
    /// Median minutes from request to a driver accepting.
    pub median_match_min: Option<f64>,
    pub sos: i64,
    pub ridechecks_asked: i64,
    pub ridechecks_unanswered: i64,
    pub pin_lockouts: i64,
    pub safety_reports: i64,
    pub other_reports: i64,
    /// Median hours from a rider submitting her ID to the decision.
    pub median_verification_h: Option<f64>,
    pub drivers_online_now: i64,
    pub trips_in_progress_now: i64,
    pub incidents_open_now: i64,
}

#[utoipa::path(
    get, path = "/v1/ops/metrics", operation_id = "metrics", tag = "ops", security(("staff" = [])),
    params(MetricsQuery),
    responses(
        (status = 200, description = "Counts for the period and right now", body = Metrics),
        (status = 422, description = "days outside 1–90", body = ErrorBody),
    )
)]
pub async fn metrics(State(s): State<AppState>, _staff: Staff, Query(q): Query<MetricsQuery>) -> Result<Json<Metrics>, ApiError> {
    let days = q.days.unwrap_or(7);
    if !(1..=90).contains(&days) {
        return Err(ApiError::invalid("days must be between 1 and 90"));
    }
    let m: Metrics = sqlx::query_as(
        "WITH since AS (SELECT now() - make_interval(days => $1) AS at),
         r AS (SELECT rides.* FROM rides, since WHERE requested_at >= since.at)
         SELECT $1 AS days,
           (SELECT count(*) FROM r) AS requested,
           (SELECT count(*) FROM r WHERE state IN ('COMPLETED', 'RATED')) AS completed,
           (SELECT count(*) FROM r WHERE state = 'CANCELLED' AND cancelled_by = 'rider') AS cancelled_by_rider,
           (SELECT count(*) FROM ride_events e JOIN r ON r.id = e.ride_id WHERE e.actor = 'driver' AND e.to_state = 'SEARCHING') AS cancelled_by_driver,
           (SELECT count(DISTINCT e.ride_id) FROM ride_events e JOIN r ON r.id = e.ride_id WHERE e.to_state = 'NO_DRIVERS') AS no_drivers,
           (SELECT count(*) FILTER (WHERE state IN ('COMPLETED', 'RATED'))::float8 / nullif(count(*), 0) FROM r) AS completion_rate,
           (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY extract(epoch FROM assigned_at - requested_at) / 60)
              FROM r WHERE assigned_at IS NOT NULL) AS median_match_min,
           (SELECT count(*) FROM safety_incidents, since WHERE kind = 'sos' AND created_at >= since.at) AS sos,
           (SELECT count(*) FROM ride_checks, since WHERE asked_at >= since.at) AS ridechecks_asked,
           (SELECT count(*) FROM ride_checks, since WHERE asked_at >= since.at AND incident_id IS NOT NULL) AS ridechecks_unanswered,
           (SELECT count(*) FROM safety_incidents, since WHERE kind = 'pin_lockout' AND created_at >= since.at) AS pin_lockouts,
           (SELECT count(*) FROM issue_reports, since WHERE incident_id IS NOT NULL AND created_at >= since.at) AS safety_reports,
           (SELECT count(*) FROM issue_reports, since WHERE incident_id IS NULL AND created_at >= since.at) AS other_reports,
           (SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY extract(epoch FROM iv.reviewed_at - iv.created_at) / 3600)
              FROM identity_verifications iv, since WHERE iv.reviewed_at >= since.at) AS median_verification_h,
           (SELECT count(*) FROM driver_presence) AS drivers_online_now,
           (SELECT count(*) FROM rides WHERE state IN ('DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED', 'PIN_VERIFIED', 'IN_TRIP'))
             AS trips_in_progress_now,
           (SELECT count(*) FROM safety_incidents WHERE status <> 'closed') AS incidents_open_now",
    )
    .bind(days)
    .fetch_one(&s.db)
    .await?;
    Ok(Json(m))
}
