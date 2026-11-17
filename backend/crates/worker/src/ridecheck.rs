//! RideCheck (PRD S-3, App Flow §6.2), every 10 s for each trip in progress:
//!
//! 1. **Planned route**: filled once the trip starts (OSRM; a straight line with the local
//!    estimate). Until it is there, only stops are checked.
//! 2. **Ask**: the car is more than 150 m off the planned route on its last two samples, or it
//!    has not moved more than 30 m for 2 minutes away from the destination, or no position has
//!    arrived for 2 minutes (150 m and 2 minutes are ops settings, PRD D-6). The rider's app
//!    shows *Are you okay?* (`ride_check` on her view of the ride). The driver is never told.
//!    The same kind is not asked again within 5 minutes.
//! 3. **Escalate**: no answer within 60 s opens a `ridecheck_deviation` / `ridecheck_stop`
//!    incident for the safety desk with the car's position.
//!
//! Her answer is `POST /v1/safety/ridecheck/{id}/respond` (API); *Call for help* raises an SOS.

use fuber_common::validate::LatLng;
use fuber_db::{
    settings::RideCheckLimits,
    sqlx::{self, PgPool},
};
use fuber_domain::safety::{RIDECHECK_AGAIN_S, RIDECHECK_ANSWER_S, RIDECHECK_NEAR_DESTINATION_M, RIDECHECK_STOP_RADIUS_M};
use serde_json::json;
use uuid::Uuid;

use crate::Ctx;

/// A position older than this is not "the car now" for the route check.
const FRESH_S: f64 = 30.0;
/// Track samples are 10 s apart: allow one missing when looking back over the stop window.
const SAMPLE_SLACK_S: i64 = 10;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub routes_planned: usize,
    pub asked: usize,
    pub escalated: usize,
}

pub async fn tick(ctx: &Ctx) -> Result<Tick, sqlx::Error> {
    let routes_planned = plan_routes(ctx).await?;
    let escalated = escalate(&ctx.db).await?;
    let asked = ask(&ctx.db).await?;
    if asked + escalated > 0 {
        tracing::warn!(security_event = "ridecheck", asked, escalated, "RideCheck");
    }
    Ok(Tick { routes_planned, asked, escalated })
}

async fn plan_routes(ctx: &Ctx) -> Result<usize, sqlx::Error> {
    let rides: Vec<(Uuid, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT id, ST_Y(pickup::geometry), ST_X(pickup::geometry), ST_Y(dropoff::geometry), ST_X(dropoff::geometry)
         FROM rides WHERE state = 'IN_TRIP' AND planned_route IS NULL LIMIT 20",
    )
    .fetch_all(&ctx.db)
    .await?;
    let mut n = 0;
    for (id, plat, plon, dlat, dlon) in rides {
        let line = match ctx.routing.route_line(LatLng { lat: plat, lon: plon }, LatLng { lat: dlat, lon: dlon }).await {
            Ok(line) => line,
            Err(e) => {
                tracing::warn!(error = %e, ride_id = %id, "no planned route for RideCheck yet");
                continue;
            }
        };
        let wkt =
            format!("SRID=4326;LINESTRING({})", line.iter().map(|p| format!("{:.6} {:.6}", p.lon, p.lat)).collect::<Vec<_>>().join(","));
        n += sqlx::query("UPDATE rides SET planned_route = ST_GeogFromText($2) WHERE id = $1 AND planned_route IS NULL")
            .bind(id)
            .bind(wkt)
            .execute(&ctx.db)
            .await?
            .rows_affected() as usize;
    }
    Ok(n)
}

/// Unanswered checks past 60 s become incidents. Safe with several workers (row locks).
async fn escalate(db: &PgPool) -> Result<usize, sqlx::Error> {
    let mut tx = db.begin().await?;
    let due: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, ride_id, kind FROM ride_checks
         WHERE answered_at IS NULL AND incident_id IS NULL AND asked_at <= now() - make_interval(secs => $1)
         FOR UPDATE SKIP LOCKED",
    )
    .bind(RIDECHECK_ANSWER_S as f64)
    .fetch_all(&mut *tx)
    .await?;
    for (check_id, ride_id, kind) in &due {
        sqlx::query(
            "WITH i AS (
               INSERT INTO safety_incidents (ride_id, raised_by, kind, location)
               SELECT ride_id, NULL, ('ridecheck_' || kind)::incident_kind, location FROM ride_checks WHERE id = $1
               RETURNING id
             )
             UPDATE ride_checks SET incident_id = (SELECT id FROM i) WHERE id = $1",
        )
        .bind(check_id)
        .execute(&mut *tx)
        .await?;
        tracing::warn!(security_event = "ridecheck_unanswered", %ride_id, kind, "RideCheck unanswered; safety desk alerted");
        notify(&mut tx, *ride_id).await?;
    }
    tx.commit().await?;
    Ok(due.len())
}

/// What the last track samples of one trip say: the ride; the last two samples are off the
/// planned route (false without a route); no movement over the stop window away from the
/// destination; the trip has run for the stop window with no sample during it.
type Signals = (Uuid, bool, bool, bool);

async fn ask(db: &PgPool) -> Result<usize, sqlx::Error> {
    // 150 m and 2 minutes unless ops changed them (`safety.deviation_m`, `safety.unexpected_stop_s`).
    let limits = RideCheckLimits::load(db).await?;
    let trips: Vec<Signals> = sqlx::query_as(
        "WITH trips AS (
           SELECT r.id, r.started_at, r.planned_route, r.dropoff FROM rides r
           WHERE r.state = 'IN_TRIP'
             AND NOT EXISTS (SELECT 1 FROM ride_checks c WHERE c.ride_id = r.id AND c.answered_at IS NULL AND c.incident_id IS NULL)
         )
         SELECT t.id AS ride_id,
           coalesce((
             SELECT count(*) = 2 AND bool_and(ST_Distance(p.location, t.planned_route) > $1) AND max(p.at) > now() - make_interval(secs => $2)
             FROM (SELECT location, at FROM ride_track_points WHERE ride_id = t.id AND at >= t.started_at ORDER BY at DESC LIMIT 2) p
           ), false) AND t.planned_route IS NOT NULL AS off_route,
           coalesce(w.first_at <= now() - make_interval(secs => $3) AND w.spread <= $5 AND ST_Distance(l.location, t.dropoff) > $6, false)
             AS stopped,
           t.started_at <= now() - make_interval(secs => $3) AND coalesce(l.at < now() - make_interval(secs => $3), true) AS silent
         FROM trips t
         -- The latest sample of this trip.
         LEFT JOIN LATERAL (
           SELECT location, at FROM ride_track_points WHERE ride_id = t.id AND at >= t.started_at ORDER BY at DESC LIMIT 1
         ) l ON true
         -- The samples over the stop window: the oldest, and how far any is from the latest.
         LEFT JOIN LATERAL (
           SELECT min(p.at) AS first_at, max(ST_Distance(p.location, l.location)) AS spread FROM ride_track_points p
           WHERE p.ride_id = t.id AND p.at >= t.started_at AND p.at >= now() - make_interval(secs => $3 + $4)
         ) w ON true",
    )
    .bind(limits.off_route_m)
    .bind(FRESH_S)
    .bind(limits.stop_s as f64)
    .bind(SAMPLE_SLACK_S as f64)
    .bind(RIDECHECK_STOP_RADIUS_M)
    .bind(RIDECHECK_NEAR_DESTINATION_M)
    .fetch_all(db)
    .await?;

    let mut n = 0;
    for (ride_id, off_route, stopped, silent) in trips {
        let kind = if off_route {
            "deviation"
        } else if stopped || silent {
            "stop"
        } else {
            continue;
        };
        let mut tx = db.begin().await?;
        let id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO ride_checks (ride_id, kind, location)
             SELECT $1, $2, coalesce((SELECT location FROM ride_track_points WHERE ride_id = $1 ORDER BY at DESC LIMIT 1),
                                     (SELECT p.location FROM driver_presence p JOIN rides r ON r.driver_id = p.driver_id WHERE r.id = $1))
             WHERE NOT EXISTS (SELECT 1 FROM ride_checks WHERE ride_id = $1 AND kind = $2 AND asked_at > now() - make_interval(secs => $3))
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(ride_id)
        .bind(kind)
        .bind(RIDECHECK_AGAIN_S as f64)
        .fetch_optional(&mut *tx)
        .await?;
        if id.is_some() {
            notify(&mut tx, ride_id).await?;
            n += 1;
        }
        tx.commit().await?;
    }
    Ok(n)
}

/// Tells the rider's app (only hers) that the ride's RideCheck changed; the API pushes her view.
async fn notify(tx: &mut sqlx::PgConnection, ride_id: Uuid) -> Result<(), sqlx::Error> {
    let rider: Uuid = sqlx::query_scalar("SELECT rider_id FROM rides WHERE id = $1").bind(ride_id).fetch_one(&mut *tx).await?;
    let event = json!({ "kind": "ridecheck", "ride_id": ride_id, "rider_id": rider });
    sqlx::query("SELECT pg_notify('fuber_events', $1)").bind(event.to_string()).execute(&mut *tx).await?;
    Ok(())
}
