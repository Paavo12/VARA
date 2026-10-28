//! The driver's side of dispatch (PRD D-1 – D-4): going online and offline, position updates,
//! and answering a request.
//!
//! Accepting races other things happening to the ride (the rider cancelling, the offer
//! expiring). It wins only if, in one transaction, its offer is still pending and unexpired,
//! the ride is still `SEARCHING`, and she has no other active ride (`one_active_ride_driver`).
//! Exactly one accept can succeed (TRD §4.2 step 6).

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_common::validate::LatLng;
use fuber_db::{
    presence::{self, NotEligible, Position},
    rides,
};
use fuber_domain::{Party, RideEvent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, DriverUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::rides::ride_error,
    views::{self, DriverRide, OfferCard},
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocationUpdate {
    pub at: LatLng,
    /// Degrees from north, 0–359.
    pub heading: Option<u16>,
    pub speed_kmh: Option<f32>,
}

impl LocationUpdate {
    pub fn position(&self) -> Result<Position, ApiError> {
        if self.heading.is_some_and(|h| h > 359) {
            return Err(ApiError::invalid("heading must be 0–359"));
        }
        if self.speed_kmh.is_some_and(|v| !(0.0..=250.0).contains(&v)) {
            return Err(ApiError::invalid("speed must be 0–250 km/h"));
        }
        Ok(Position { lat: self.at.lat, lon: self.at.lon, heading: self.heading.map(|h| h as i16), speed_kmh: self.speed_kmh })
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DriverStatus {
    pub online: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub online_since: Option<OffsetDateTime>,
    /// Ride types she receives requests for.
    pub tiers: Vec<String>,
    /// The request on her screen, if any (15 s to answer).
    pub offer: Option<OfferCard>,
    /// The trip she is on, if any.
    pub ride: Option<DriverRide>,
}

pub(crate) async fn status_of(s: &AppState, user: &AuthUser) -> Result<DriverStatus, ApiError> {
    let online = presence::status(&s.db, user.user_id, user.session_id).await?;
    Ok(DriverStatus {
        online: online.is_some(),
        online_since: online.as_ref().map(|o| o.online_since),
        tiers: online.map(|o| o.tiers).unwrap_or_default(),
        offer: views::pending_offer(&s.db, user.user_id).await?,
        ride: views::driver_current(&s.db, user.user_id).await?,
    })
}

#[utoipa::path(
    get, path = "/v1/driver/status", tag = "dispatch", security(("bearer" = [])),
    responses((status = 200, description = "Online state, pending request and current trip (refetch after reconnecting)", body = DriverStatus))
)]
pub async fn status(State(s): State<AppState>, DriverUser(user): DriverUser) -> Result<Json<DriverStatus>, ApiError> {
    Ok(Json(status_of(&s, &user).await?))
}

#[utoipa::path(
    post, path = "/v1/driver/online", tag = "dispatch", security(("bearer" = [])),
    request_body = LocationUpdate,
    responses(
        (status = 200, description = "Online: requests can arrive", body = DriverStatus),
        (status = 403, description = "`not_approved` (application, documents, vehicle or tier) or `account_suspended`", body = ErrorBody),
        (status = 422, description = "Position outside the service area", body = ErrorBody),
    )
)]
pub async fn go_online(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    ValidJson(req): ValidJson<LocationUpdate>,
) -> Result<Json<DriverStatus>, ApiError> {
    let at = req.position()?;
    let mut tx = s.db.begin().await?;
    match presence::go_online(&mut tx, user.user_id, user.session_id, at).await? {
        Ok(_) => {}
        Err(NotEligible::Suspended) => {
            return Err(ApiError::forbidden("account_suspended", "Your account is suspended. Please contact FUber support."));
        }
        Err(NotEligible::NotApproved) => {
            return Err(ApiError::forbidden(
                "not_approved",
                "You can't go online yet: your application, documents, vehicle and ride types must all be approved and up to date.",
            ));
        }
    }
    tx.commit().await?;
    tracing::info!(driver_id = %user.user_id, "driver online");
    Ok(Json(status_of(&s, &user).await?))
}

#[utoipa::path(
    post, path = "/v1/driver/offline", tag = "dispatch", security(("bearer" = [])),
    responses((status = 200, description = "Offline: no new requests. A trip in progress continues.", body = DriverStatus))
)]
pub async fn go_offline(State(s): State<AppState>, DriverUser(user): DriverUser) -> Result<Json<DriverStatus>, ApiError> {
    let mut tx = s.db.begin().await?;
    presence::go_offline(&mut tx, user.user_id, "went_offline").await?;
    tx.commit().await?;
    tracing::info!(driver_id = %user.user_id, "driver offline");
    Ok(Json(status_of(&s, &user).await?))
}

/// Records a position and tells the rider where her assigned driver is.
pub(crate) async fn record_location(s: &AppState, user: &AuthUser, at: Position) -> Result<bool, ApiError> {
    if !presence::update_location(&s.db, user.user_id, user.session_id, at).await? {
        return Ok(false);
    }
    // Only a rider waiting for this driver hears about her position.
    sqlx::query(
        "SELECT pg_notify('fuber_events', json_build_object('kind', 'location', 'ride_id', r.id, 'rider_id', r.rider_id,
                 'driver_id', r.driver_id, 'lat', $2::float8, 'lon', $3::float8, 'heading', $4::int)::text)
         FROM rides r WHERE r.driver_id = $1 AND r.state IN ('DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED')",
    )
    .bind(user.user_id)
    .bind(at.lat)
    .bind(at.lon)
    .bind(at.heading.map(i32::from))
    .execute(&s.db)
    .await?;
    Ok(true)
}

#[utoipa::path(
    put, path = "/v1/driver/location", tag = "dispatch", security(("bearer" = [])),
    request_body = LocationUpdate,
    responses(
        (status = 204, description = "Recorded. The app normally sends this over the WebSocket (`location.update`)."),
        (status = 409, description = "`offline`: go online first", body = ErrorBody),
        (status = 429, description = "40 a minute", body = ErrorBody),
    )
)]
pub async fn location(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    ValidJson(req): ValidJson<LocationUpdate>,
) -> Result<StatusCode, ApiError> {
    s.limiter.check("location", &user.user_id.to_string(), s.limits.location).await?;
    if record_location(&s, &user, req.position()?).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::conflict("offline", "You're offline. Go online to receive requests."))
    }
}

#[utoipa::path(
    post, path = "/v1/offers/{id}/accept", tag = "dispatch", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "The ride is hers: drive to the pickup area", body = DriverRide),
        (status = 404, description = "Not her offer", body = ErrorBody),
        (status = 409, description = "`offer_gone`: expired, withdrawn, or the rider cancelled", body = ErrorBody),
    )
)]
pub async fn accept(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<DriverRide>, ApiError> {
    let gone = || ApiError::conflict("offer_gone", "This request is no longer available.");
    let mut tx = s.db.begin().await?;
    // SKIP LOCKED: if another accept of this offer is already in flight (a double tap), it
    // decides the outcome; this one answers at once instead of holding a connection in a queue.
    let offer: Option<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT ride_id, response::text, expires_at > now() FROM ride_offers WHERE id = $1 AND driver_id = $2 FOR UPDATE SKIP LOCKED",
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((ride_id, response, fresh)) = offer else {
        let hers: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM ride_offers WHERE id = $1 AND driver_id = $2)")
            .bind(id)
            .bind(user.user_id)
            .fetch_one(&mut *tx)
            .await?;
        return Err(if hers { gone() } else { ApiError::not_found() });
    };
    if response != "pending" || !fresh {
        return Err(gone());
    }
    // The ride must still be searching (not cancelled by the rider a moment ago).
    let state = rides::lock(&mut tx, ride_id).await.map_err(ride_error)?;
    if state != fuber_domain::RideState::Searching {
        return Err(gone());
    }
    sqlx::query("UPDATE ride_offers SET response = 'accepted', responded_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    let assigned = sqlx::query(
        "UPDATE rides SET driver_id = $2, assigned_at = now(),
                vehicle_id = (SELECT active_vehicle_id FROM driver_profiles WHERE user_id = $2)
         WHERE id = $1",
    )
    .bind(ride_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await;
    match assigned {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("one_active_ride_driver") => {
            return Err(ApiError::conflict("offer_gone", "You already have a trip in progress."));
        }
        Err(e) => return Err(e.into()),
    }
    rides::transition(&mut tx, ride_id, RideEvent::DriverAccepted, Party::Driver, Some(user.user_id), Some(json!({ "offer_id": id })))
        .await
        .map_err(ride_error)?;
    // Phase 3 computes the live route here; until then she drives to the fuzzy area at once.
    rides::transition(&mut tx, ride_id, RideEvent::RouteComputed, Party::System, None, None).await.map_err(ride_error)?;
    rides::record_answer(&mut *tx, user.user_id, true).await?;
    let guest: bool =
        sqlx::query_scalar("SELECT guest_phone IS NOT NULL FROM rides WHERE id = $1").bind(ride_id).fetch_one(&mut *tx).await?;
    if guest {
        fuber_db::jobs::enqueue(
            &mut *tx,
            fuber_db::jobs::kind::GUEST_SMS,
            json!({ "ride_id": ride_id }),
            None,
            Some(&format!("guest-sms:{ride_id}")),
        )
        .await?;
    }
    match tx.commit().await {
        Ok(()) => {}
        // Two accepts of the same offer at once: the loser's commit fails on the unique index.
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => return Err(gone()),
        Err(e) => return Err(e.into()),
    }
    tracing::info!(%ride_id, driver_id = %user.user_id, "offer accepted");
    views::driver_ride(&s.db, user.user_id, ride_id).await?.map(Json).ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    post, path = "/v1/offers/{id}/decline", tag = "dispatch", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "Declined; the request goes to the next driver"),
        (status = 404, description = "Not her offer", body = ErrorBody),
        (status = 409, description = "`offer_gone`", body = ErrorBody),
    )
)]
pub async fn decline(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let mut tx = s.db.begin().await?;
    let row: Option<String> = sqlx::query_scalar("SELECT response::text FROM ride_offers WHERE id = $1 AND driver_id = $2 FOR UPDATE")
        .bind(id)
        .bind(user.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    match row.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("pending") => {}
        Some(_) => return Err(ApiError::conflict("offer_gone", "This request is no longer available.")),
    }
    sqlx::query("UPDATE ride_offers SET response = 'declined', responded_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
    rides::record_answer(&mut *tx, user.user_id, false).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
