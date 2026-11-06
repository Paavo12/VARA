//! The trip, from the driver's side (PRD T-2 – T-7): arriving at the pickup, the PIN, the free
//! wait and no-show, starting, completing, and cancelling after accepting.
//!
//! - **Arrival** needs her last reported position within 50 m of the exact pickup (she has
//!   only seen the fuzzy circle until then). Location updates mark it automatically; `arrive`
//!   is the button for when they are slow.
//! - **PIN**: five wrong entries block the trip (`BLOCKED`), raise a `pin_lockout` incident
//!   for the safety desk and hold her from new requests until it is closed (see
//!   [`fuber_db::presence::lockout_hold`]). The correct one unlocks the destination.
//! - Every action locks the ride row and goes through the state machine, so a double tap or a
//!   rider cancelling at the same moment can never apply twice.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_common::validate::{LatLng, Pin4};
use fuber_db::rides::{self, RideError};
use fuber_domain::{
    Party, RideEvent, RideState, TransitionError,
    trip::{self, PinCheck},
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::DriverUser,
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::rides::ride_error,
    views::{self, DriverRide},
};

/// Her ride, or 404 (another driver's ride looks exactly like a missing one).
async fn own_trip(db: &PgPool, driver_id: Uuid, ride_id: Uuid) -> Result<(), ApiError> {
    let mine: Option<bool> = sqlx::query_scalar("SELECT driver_id IS NOT DISTINCT FROM $2 FROM rides WHERE id = $1")
        .bind(ride_id)
        .bind(driver_id)
        .fetch_optional(db)
        .await?;
    if mine == Some(true) { Ok(()) } else { Err(ApiError::not_found()) }
}

async fn view(s: &AppState, driver_id: Uuid, ride_id: Uuid) -> Result<Json<DriverRide>, ApiError> {
    views::driver_ride(&s.db, driver_id, ride_id).await?.map(Json).ok_or_else(ApiError::not_found)
}

/// The state-machine error for `event` on a ride in `state`.
fn not_now(state: RideState, event: RideEvent) -> ApiError {
    ride_error(RideError::Invalid(TransitionError { state, event }))
}

/// Marks the driver arrived if her last position is within 50 m of the pickup. Returns the
/// distance when she is further (None when her position is unknown).
pub(crate) async fn try_arrive(conn: &mut PgConnection, driver_id: Uuid, ride_id: Uuid) -> Result<Result<(), Option<f64>>, RideError> {
    let state = rides::lock(conn, ride_id).await?;
    if state != RideState::DriverEnRoute {
        return Err(RideError::Invalid(TransitionError { state, event: RideEvent::DriverArrived }));
    }
    let distance: Option<f64> = sqlx::query_scalar(
        "SELECT ST_Distance(p.location, r.pickup) FROM rides r JOIN driver_presence p ON p.driver_id = r.driver_id
         WHERE r.id = $1 AND r.driver_id = $2",
    )
    .bind(ride_id)
    .bind(driver_id)
    .fetch_optional(&mut *conn)
    .await?;
    match distance {
        Some(d) if d <= trip::ARRIVE_RADIUS_M => {
            sqlx::query("UPDATE rides SET arrived_at = now() WHERE id = $1").bind(ride_id).execute(&mut *conn).await?;
            rides::transition(
                conn,
                ride_id,
                RideEvent::DriverArrived,
                Party::Driver,
                Some(driver_id),
                Some(json!({ "distance_m": d.round() })),
            )
            .await?;
            Ok(Ok(()))
        }
        other => Ok(Err(other)),
    }
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/arrive", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Arrived: the exact pickup is unlocked; ask for the PIN", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`not_at_pickup` (more than 50 m away, or no recent position) or `invalid_state`", body = ErrorBody),
    )
)]
pub async fn arrive(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    if let Err(distance) = try_arrive(&mut tx, user.user_id, id).await.map_err(ride_error)? {
        tracing::info!(ride_id = %id, distance = ?distance.map(f64::round), "arrive refused: not at pickup");
        return Err(ApiError::conflict(
            "not_at_pickup",
            "You're not at the pickup yet. Drive into the circle: the exact pickup appears within 50 m of it.",
        ));
    }
    tx.commit().await?;
    view(&s, user.user_id, id).await
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PinEntry {
    /// The 4 digits the passenger shows or says.
    #[schema(value_type = String, example = "0427")]
    pub pin: Pin4,
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/pin", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = PinEntry,
    responses(
        (status = 200, description = "Correct: the destination is unlocked", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`pin_locked`: that was the 5th wrong PIN; the trip is blocked and the safety desk alerted. Or `invalid_state`", body = ErrorBody),
        (status = 422, description = "`wrong_pin` (the message says how many attempts are left) or not 4 digits", body = ErrorBody),
    )
)]
pub async fn pin(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<PinEntry>,
) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    let state = rides::lock(&mut tx, id).await.map_err(ride_error)?;
    if state != RideState::DriverArrived {
        return Err(not_now(state, RideEvent::PinCorrect));
    }
    let (expected, wrong): (String, i16) =
        sqlx::query_as("SELECT pin, pin_attempts FROM rides WHERE id = $1").bind(id).fetch_one(&mut *tx).await?;
    let outcome = trip::check_pin(&expected, req.pin.as_str(), wrong);
    if outcome != PinCheck::Correct {
        sqlx::query("UPDATE rides SET pin_attempts = pin_attempts + 1 WHERE id = $1").bind(id).execute(&mut *tx).await?;
    }
    match outcome {
        PinCheck::Correct => {
            rides::transition(&mut tx, id, RideEvent::PinCorrect, Party::Driver, Some(user.user_id), None).await.map_err(ride_error)?;
            tx.commit().await?;
            tracing::info!(ride_id = %id, "PIN verified");
            view(&s, user.user_id, id).await
        }
        PinCheck::Wrong { attempts_left } => {
            tx.commit().await?;
            tracing::warn!(security_event = "pin_wrong", ride_id = %id, driver_id = %user.user_id, attempts_left, "wrong trip PIN");
            let left = if attempts_left == 1 { "1 attempt".to_owned() } else { format!("{attempts_left} attempts") };
            Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "wrong_pin",
                format!("That PIN is wrong: {left} left. Ask the passenger to check the PIN in her app."),
            ))
        }
        PinCheck::LockedOut => {
            rides::transition(&mut tx, id, RideEvent::PinLockout, Party::System, None, Some(json!({ "driver_id": user.user_id })))
                .await
                .map_err(ride_error)?;
            sqlx::query(
                "INSERT INTO safety_incidents (ride_id, kind, location)
                 SELECT $1, 'pin_lockout', (SELECT location FROM driver_presence WHERE driver_id = $2)",
            )
            .bind(id)
            .bind(user.user_id)
            .execute(&mut *tx)
            .await?;
            // No priority points are taken: the wrong PINs may not be her fault.
            tx.commit().await?;
            tracing::warn!(security_event = "pin_lockout", ride_id = %id, driver_id = %user.user_id, "trip blocked after 5 wrong PINs");
            Err(ApiError::conflict(
                "pin_locked",
                "Too many wrong PINs. This trip is blocked and VARA's safety team has been alerted. Don't start the trip; they will call you. You'll get no new requests until they have reviewed it.",
            ))
        }
    }
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/wait", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "The 5-minute free wait is running (starting it again changes nothing)", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`invalid_state`: only at the pickup, before the PIN", body = ErrorBody),
    )
)]
pub async fn wait(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    let state = rides::lock(&mut tx, id).await.map_err(ride_error)?;
    if state != RideState::DriverArrived {
        return Err(not_now(state, RideEvent::RiderNoShow));
    }
    let started = sqlx::query("UPDATE rides SET wait_started_at = now() WHERE id = $1 AND wait_started_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if started.rows_affected() == 1 {
        // Not a state change, but the rider should see "Your driver is waiting" at once.
        rides::record(&mut *tx, id, Some(state), state, Party::Driver, Some(user.user_id), Some(json!({ "wait_started": true }))).await?;
    }
    tx.commit().await?;
    view(&s, user.user_id, id).await
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/no-show", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Cancelled as a rider no-show: free, and it counts against the rider", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`wait_not_over` (start the wait, then 5 minutes) or `invalid_state`", body = ErrorBody),
    )
)]
pub async fn no_show(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    let state = rides::lock(&mut tx, id).await.map_err(ride_error)?;
    let (rider_id, waited): (Uuid, Option<bool>) =
        sqlx::query_as("SELECT rider_id, wait_started_at <= now() - make_interval(secs => $2) FROM rides WHERE id = $1")
            .bind(id)
            .bind(trip::FREE_WAIT_S as f64)
            .fetch_one(&mut *tx)
            .await?;
    if state == RideState::DriverArrived && waited != Some(true) {
        return Err(ApiError::conflict("wait_not_over", "You can cancel as a no-show once the 5-minute free wait is over."));
    }
    rides::transition(&mut tx, id, RideEvent::RiderNoShow, Party::Driver, Some(user.user_id), None).await.map_err(ride_error)?;
    sqlx::query("UPDATE rides SET cancelled_by = 'driver', cancel_reason = 'rider_no_show' WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let lost = rides::penalise(&mut tx, rider_id, Party::Rider, id, "no_show").await?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, lost, "rider no-show");
    view(&s, user.user_id, id).await
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/start", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "In trip: drive to the destination", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`invalid_state`: the PIN must be verified first", body = ErrorBody),
    )
)]
pub async fn start(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    rides::transition(&mut tx, id, RideEvent::TripStarted, Party::Driver, Some(user.user_id), None).await.map_err(ride_error)?;
    sqlx::query("UPDATE rides SET started_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, "trip started");
    view(&s, user.user_id, id).await
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/complete", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Completed. Cash: collect the locked fare", body = DriverRide),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`invalid_state`: the trip hasn't started", body = ErrorBody),
    )
)]
pub async fn complete(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    // How far from the destination she ended it, for the trip record (passengers may ask to be
    // dropped early, so it is not a condition).
    let (rider_id, from_dropoff): (Uuid, Option<f64>) = sqlx::query_as(
        "SELECT r.rider_id, ST_Distance(p.location, r.dropoff) FROM rides r LEFT JOIN driver_presence p ON p.driver_id = r.driver_id
         WHERE r.id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    rides::transition(
        &mut tx,
        id,
        RideEvent::TripCompleted,
        Party::Driver,
        Some(user.user_id),
        Some(json!({ "from_dropoff_m": from_dropoff.map(f64::round) })),
    )
    .await
    .map_err(ride_error)?;
    sqlx::query("UPDATE rides SET completed_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    // The locked fare is now due (PRD P-1, P-2). Cash: the driver confirms receipt next.
    sqlx::query(
        "INSERT INTO payments (ride_id, method, amount_cents, status)
         SELECT id, payment_method, fare_cents, 'pending' FROM rides WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    rides::reward(&mut tx, rider_id, Party::Rider, id).await?;
    rides::reward(&mut tx, user.user_id, Party::Driver, id).await?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, "trip completed");
    view(&s, user.user_id, id).await
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DriverCancelReason {
    RiderAskedMe,
    CantReachPickup,
    FeelUnsafe,
    VehicleProblem,
    Other,
}

impl DriverCancelReason {
    fn as_str(self) -> &'static str {
        match self {
            DriverCancelReason::RiderAskedMe => "rider_asked_me",
            DriverCancelReason::CantReachPickup => "cant_reach_pickup",
            DriverCancelReason::FeelUnsafe => "feel_unsafe",
            DriverCancelReason::VehicleProblem => "vehicle_problem",
            DriverCancelReason::Other => "other",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DriverCancel {
    pub reason: DriverCancelReason,
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/driver-cancel", tag = "trip", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = DriverCancel,
    responses(
        (status = 204, description = "Released: the request goes back to the pool without her. Free, but it costs priority (PRD T-7)"),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`invalid_state`: not after the PIN", body = ErrorBody),
    )
)]
pub async fn driver_cancel(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<DriverCancel>,
) -> Result<StatusCode, ApiError> {
    own_trip(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    let state = rides::lock(&mut tx, id).await.map_err(ride_error)?;
    let event = RideEvent::Cancel { by: Party::Driver };
    state.apply(event).map_err(|e| ride_error(RideError::Invalid(e)))?;
    // Back to the pool as if never assigned. Her offer for this round stays `accepted`, so
    // matching won't ask her again (PRD T-7); the rider keeps her PIN.
    sqlx::query(
        "UPDATE rides SET driver_id = NULL, vehicle_id = NULL, assigned_at = NULL, arrived_at = NULL, wait_started_at = NULL,
                pin_attempts = 0, search_started_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    rides::transition(
        &mut tx,
        id,
        event,
        Party::Driver,
        Some(user.user_id),
        Some(json!({ "previous_driver_id": user.user_id, "reason": req.reason.as_str() })),
    )
    .await
    .map_err(ride_error)?;
    let lost = rides::penalise(&mut tx, user.user_id, Party::Driver, id, "cancel").await?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, reason = req.reason.as_str(), lost, "driver cancelled; back to searching");
    Ok(StatusCode::NO_CONTENT)
}

/// The rider's live view of where her driver is, and the trip's GPS track (10 s samples,
/// kept 90 days: schema §12). Called for every position the driver reports.
pub(crate) async fn on_location(
    db: &PgPool,
    driver_id: Uuid,
    at: LatLng,
    heading: Option<i16>,
    speed_kmh: Option<f32>,
) -> Result<(), ApiError> {
    let ride: Option<(Uuid, Uuid, String, bool)> = sqlx::query_as(
        "SELECT id, rider_id, state::text, ST_DWithin(pickup, ST_SetSRID(ST_MakePoint($3, $2), 4326)::geography, $4)
         FROM rides WHERE driver_id = $1 AND state::text = ANY($5)",
    )
    .bind(driver_id)
    .bind(at.lat)
    .bind(at.lon)
    .bind(trip::ARRIVE_RADIUS_M)
    .bind(&rides::DRIVER_ACTIVE[..])
    .fetch_optional(db)
    .await?;
    let Some((ride_id, rider_id, state, near_pickup)) = ride else { return Ok(()) };
    let event = json!({ "kind": "location", "ride_id": ride_id, "rider_id": rider_id, "driver_id": driver_id,
                        "lat": at.lat, "lon": at.lon, "heading": heading });
    sqlx::query("SELECT pg_notify('fuber_events', $1)").bind(event.to_string()).execute(db).await?;
    match state.as_str() {
        "DRIVER_EN_ROUTE" if near_pickup => {
            let mut tx = db.begin().await?;
            match try_arrive(&mut tx, driver_id, ride_id).await {
                Ok(Ok(())) => tx.commit().await?,
                // Raced with a cancel or the arrive button: nothing to do.
                Ok(Err(_)) | Err(RideError::Invalid(_) | RideError::NotFound) => {}
                Err(RideError::Db(e)) => return Err(e.into()),
            }
        }
        "IN_TRIP" => {
            sqlx::query(
                "INSERT INTO ride_track_points (ride_id, at, location, speed_kmh, heading)
                 SELECT $1, now(), ST_SetSRID(ST_MakePoint($3, $2), 4326)::geography, $4, $5
                 WHERE NOT EXISTS (SELECT 1 FROM ride_track_points WHERE ride_id = $1 AND at > now() - interval '10 seconds')",
            )
            .bind(ride_id)
            .bind(at.lat)
            .bind(at.lon)
            .bind(speed_kmh)
            .bind(heading)
            .execute(db)
            .await?;
        }
        _ => {}
    }
    Ok(())
}
