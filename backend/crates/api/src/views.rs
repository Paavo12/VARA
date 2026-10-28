//! What each party sees of a ride (TRD §4.4). Riders and drivers get separate types, built
//! from separate queries, so a driver payload can't carry what she may not see yet:
//!
//! - never the PIN;
//! - the exact pickup only from `DRIVER_ARRIVED` (PRD T-2), a fuzzy area before;
//! - the destination only from `PIN_VERIFIED` (PRD T-3).
//!
//! The HTTP endpoints and the WebSocket push use these same functions.

use fuber_common::validate::LatLng;
use fuber_domain::{Cents, RideState, Tier, pricing::Fare};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::routes::places;

/// Radius of the fuzzy pickup circle drawn on the driver's map (TRD §4.4).
pub const FUZZY_RADIUS_M: u32 = 250;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Stop {
    pub address: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Area {
    /// "Near Maerua Mall" or "Klein Windhoek area".
    pub label: String,
    /// Centre of the circle: the pickup moved 100–200 m in a random direction.
    pub lat: f64,
    pub lon: f64,
    pub radius_m: u32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Vehicle {
    pub make: String,
    pub model: String,
    pub colour: String,
    pub year: i16,
    pub plate: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AssignedDriver {
    pub first_name: String,
    pub rating: Option<f64>,
    pub rating_count: i32,
    pub vehicle: Vehicle,
    /// Last reported position.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Estimated seconds until she reaches the pickup.
    pub eta_s: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RiderRide {
    pub id: Uuid,
    /// Short reference such as `FU-4102`.
    pub code: String,
    #[schema(value_type = String, example = "SEARCHING")]
    pub state: RideState,
    pub tier: Tier,
    pub passengers: i16,
    pub wheelchair: bool,
    /// Set when booked for someone else.
    pub guest_name: Option<String>,
    pub pickup: Stop,
    pub dropoff: Stop,
    /// The locked price (PRD P-1), in cents.
    pub fare: Fare,
    /// `cash` or `card`.
    pub payment_method: String,
    /// Say it to the driver at pickup. Never sent to the driver.
    pub pin: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub scheduled_for: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
    /// When the current search started (searching rides).
    #[serde(with = "time::serde::rfc3339::option")]
    pub search_started_at: Option<OffsetDateTime>,
    pub driver: Option<AssignedDriver>,
    /// `rider`, `driver`, `system` or `staff` for cancelled rides.
    pub cancelled_by: Option<String>,
}

/// Who the driver is picking up (PRD D-4, B-5).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Passenger {
    /// First name of the rider, or the guest's name.
    pub name: String,
    /// The booking rider's rating; absent for a guest.
    pub rating: Option<f64>,
    /// For a guest: "booked by Ndeshi (verified)". The demo showed a guest as "Verified" with
    /// the booker's rating, which was wrong.
    pub booked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DriverRide {
    pub id: Uuid,
    pub code: String,
    #[schema(value_type = String, example = "DRIVER_EN_ROUTE")]
    pub state: RideState,
    pub tier: Tier,
    pub passengers: i16,
    pub wheelchair: bool,
    pub passenger: Passenger,
    pub pickup_area: Area,
    /// Only from `DRIVER_ARRIVED`.
    pub pickup: Option<Stop>,
    /// Only from `PIN_VERIFIED`.
    pub dropoff: Option<Stop>,
    /// What she earns for this trip, in cents.
    pub earning: Cents,
    pub payment_method: String,
}

/// The request card (PRD D-4). No destination, no exact pickup.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OfferCard {
    pub offer_id: Uuid,
    pub ride_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    /// Seconds left to answer, as of sending.
    pub seconds_left: i64,
    pub tier: Tier,
    pub passengers: i16,
    pub wheelchair: bool,
    pub passenger: Passenger,
    pub pickup_area: Area,
    /// Road distance and time from her position to the pickup.
    pub distance_m: i32,
    pub eta_s: i32,
    pub earning: Cents,
    pub payment_method: String,
}

type RiderRow = (
    Uuid,
    String,
    String,
    String,
    i16,
    bool,
    Option<String>,
    (String, f64, f64),
    (String, f64, f64),
    serde_json::Value,
    String,
    String,
    (Option<OffsetDateTime>, OffsetDateTime, Option<OffsetDateTime>),
    Option<Uuid>,
    Option<String>,
);

/// The rider's view of one of her rides.
pub async fn rider_ride(db: &PgPool, rider_id: Uuid, ride_id: Uuid) -> Result<Option<RiderRide>, sqlx::Error> {
    let row: Option<RiderRow> = sqlx::query_as(
        "SELECT r.id, r.code, r.state::text, r.tier::text, r.passengers, r.wheelchair, r.guest_name,
                ROW(r.pickup_address, ST_Y(r.pickup::geometry), ST_X(r.pickup::geometry)),
                ROW(r.dropoff_address, ST_Y(r.dropoff::geometry), ST_X(r.dropoff::geometry)),
                r.fare_breakdown, r.payment_method::text, r.pin,
                ROW(r.scheduled_for, r.requested_at, r.search_started_at),
                r.driver_id, r.cancelled_by::text
         FROM rides r WHERE r.id = $1 AND r.rider_id = $2",
    )
    .bind(ride_id)
    .bind(rider_id)
    .fetch_optional(db)
    .await?;
    let Some((
        id,
        code,
        state,
        tier,
        passengers,
        wheelchair,
        guest_name,
        pickup,
        dropoff,
        fare,
        payment,
        pin,
        times,
        driver_id,
        cancelled_by,
    )) = row
    else {
        return Ok(None);
    };
    let state = RideState::parse(&state).unwrap_or(RideState::Cancelled);
    let driver = match driver_id {
        Some(d) if state.locks_driver() || matches!(state, RideState::Completed | RideState::Rated) => assigned_driver(db, id, d).await?,
        _ => None,
    };
    Ok(Some(RiderRide {
        id,
        code,
        state,
        tier: Tier::parse(&tier).unwrap_or(Tier::Classic),
        passengers,
        wheelchair,
        guest_name,
        pickup: Stop { address: pickup.0, lat: pickup.1, lon: pickup.2 },
        dropoff: Stop { address: dropoff.0, lat: dropoff.1, lon: dropoff.2 },
        fare: serde_json::from_value(fare).map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        payment_method: payment,
        pin,
        scheduled_for: times.0,
        requested_at: times.1,
        search_started_at: if state == RideState::Searching { times.2 } else { None },
        driver,
        cancelled_by,
    }))
}

type DriverCardRow = (Option<String>, Option<f64>, i32, String, String, String, i16, String, Option<f64>, Option<f64>, Option<i32>);

async fn assigned_driver(db: &PgPool, ride_id: Uuid, driver_id: Uuid) -> Result<Option<AssignedDriver>, sqlx::Error> {
    let row: Option<DriverCardRow> = sqlx::query_as(
        "SELECT u.display_name, dp.rating_avg::float8, dp.rating_count, v.make, v.model, v.colour, v.year, v.plate,
                ST_Y(p.location::geometry), ST_X(p.location::geometry),
                -- Until live routes (Phase 3): the accepted offer's ETA, counted down since assignment.
                (SELECT greatest(0, o.eta_s - extract(epoch FROM now() - r.assigned_at)::int)
                 FROM ride_offers o WHERE o.ride_id = r.id AND o.driver_id = r.driver_id AND o.response = 'accepted'
                 ORDER BY o.offered_at DESC LIMIT 1)
         FROM rides r
         JOIN users u ON u.id = r.driver_id
         JOIN driver_profiles dp ON dp.user_id = r.driver_id
         JOIN vehicles v ON v.id = r.vehicle_id
         LEFT JOIN driver_presence p ON p.driver_id = r.driver_id
         WHERE r.id = $1 AND r.driver_id = $2",
    )
    .bind(ride_id)
    .bind(driver_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(name, rating, rating_count, make, model, colour, year, plate, lat, lon, eta_s)| AssignedDriver {
        first_name: name.unwrap_or_else(|| "Your driver".into()),
        rating,
        rating_count,
        vehicle: Vehicle { make, model, colour, year, plate },
        lat,
        lon,
        eta_s,
    }))
}

type PassengerRow = (Option<String>, Option<String>, Option<f64>, String);

async fn passenger(db: &PgPool, ride_id: Uuid) -> Result<Passenger, sqlx::Error> {
    let (guest, booker, rating, verification): PassengerRow = sqlx::query_as(
        "SELECT r.guest_name, u.display_name, rp.rating_avg::float8, u.verification::text
         FROM rides r JOIN users u ON u.id = r.rider_id JOIN rider_profiles rp ON rp.user_id = r.rider_id WHERE r.id = $1",
    )
    .bind(ride_id)
    .fetch_one(db)
    .await?;
    let booker = booker.unwrap_or_else(|| "a FUber rider".into());
    Ok(match guest {
        Some(name) => Passenger {
            name,
            rating: None,
            booked_by: Some(if verification == "approved" {
                format!("booked by {booker} (verified)")
            } else {
                format!("booked by {booker}")
            }),
        },
        None => Passenger { name: booker, rating, booked_by: None },
    })
}

async fn area(db: &PgPool, lat: f64, lon: f64) -> Result<Area, sqlx::Error> {
    Ok(Area { label: places::approx_area(db, LatLng { lat, lon }).await?, lat, lon, radius_m: FUZZY_RADIUS_M })
}

type DriverRow = (Uuid, String, String, String, i16, bool, (f64, f64), (String, f64, f64), (String, f64, f64), i64, String);

/// The driver's view of the ride she is assigned to.
pub async fn driver_ride(db: &PgPool, driver_id: Uuid, ride_id: Uuid) -> Result<Option<DriverRide>, sqlx::Error> {
    let row: Option<DriverRow> = sqlx::query_as(
        "SELECT r.id, r.code, r.state::text, r.tier::text, r.passengers, r.wheelchair,
                ROW(ST_Y(r.pickup_fuzzed::geometry), ST_X(r.pickup_fuzzed::geometry)),
                ROW(r.pickup_address, ST_Y(r.pickup::geometry), ST_X(r.pickup::geometry)),
                ROW(r.dropoff_address, ST_Y(r.dropoff::geometry), ST_X(r.dropoff::geometry)),
                r.driver_earning_cents, r.payment_method::text
         FROM rides r WHERE r.id = $1 AND r.driver_id = $2",
    )
    .bind(ride_id)
    .bind(driver_id)
    .fetch_optional(db)
    .await?;
    let Some((id, code, state, tier, passengers, wheelchair, fuzzed, pickup, dropoff, earning, payment)) = row else { return Ok(None) };
    let state = RideState::parse(&state).unwrap_or(RideState::Cancelled);
    Ok(Some(DriverRide {
        id,
        code,
        state,
        tier: Tier::parse(&tier).unwrap_or(Tier::Classic),
        passengers,
        wheelchair,
        passenger: passenger(db, id).await?,
        pickup_area: area(db, fuzzed.0, fuzzed.1).await?,
        pickup: state.driver_sees_exact_pickup().then_some(Stop { address: pickup.0, lat: pickup.1, lon: pickup.2 }),
        dropoff: state.driver_sees_destination().then_some(Stop { address: dropoff.0, lat: dropoff.1, lon: dropoff.2 }),
        earning: Cents(earning),
        payment_method: payment,
    }))
}

type OfferRow = (Uuid, Uuid, OffsetDateTime, i64, String, i16, bool, (f64, f64), i32, i32, i64, String);

/// The driver's pending request card, if she has one.
pub async fn pending_offer(db: &PgPool, driver_id: Uuid) -> Result<Option<OfferCard>, sqlx::Error> {
    let row: Option<OfferRow> = sqlx::query_as(
        "SELECT o.id, o.ride_id, o.expires_at, ceil(extract(epoch FROM o.expires_at - now()))::int8,
                r.tier::text, r.passengers, r.wheelchair,
                ROW(ST_Y(r.pickup_fuzzed::geometry), ST_X(r.pickup_fuzzed::geometry)),
                o.distance_m, o.eta_s, o.earning_cents, r.payment_method::text
         FROM ride_offers o JOIN rides r ON r.id = o.ride_id
         WHERE o.driver_id = $1 AND o.response = 'pending' AND o.expires_at > now() AND r.state = 'SEARCHING'",
    )
    .bind(driver_id)
    .fetch_optional(db)
    .await?;
    let Some((offer_id, ride_id, expires_at, left, tier, passengers, wheelchair, fuzzed, distance_m, eta_s, earning, payment)) = row else {
        return Ok(None);
    };
    Ok(Some(OfferCard {
        offer_id,
        ride_id,
        expires_at,
        seconds_left: left.max(0),
        tier: Tier::parse(&tier).unwrap_or(Tier::Classic),
        passengers,
        wheelchair,
        passenger: passenger(db, ride_id).await?,
        pickup_area: area(db, fuzzed.0, fuzzed.1).await?,
        distance_m,
        eta_s,
        earning: Cents(earning),
        payment_method: payment,
    }))
}

/// The rider's current ride: active, scheduled, or ended in the last few minutes (so the app can
/// show "cancelled"/"completed" after a reconnect).
pub async fn rider_current(db: &PgPool, rider_id: Uuid) -> Result<Option<RiderRide>, sqlx::Error> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM rides WHERE rider_id = $1
           AND (state::text = ANY($2) OR state = 'SCHEDULED')
         ORDER BY (state = 'SCHEDULED'), requested_at DESC LIMIT 1",
    )
    .bind(rider_id)
    .bind(&fuber_db::rides::RIDER_ACTIVE[..])
    .fetch_optional(db)
    .await?;
    match id {
        Some(id) => rider_ride(db, rider_id, id).await,
        None => Ok(None),
    }
}

/// The driver's current trip.
pub async fn driver_current(db: &PgPool, driver_id: Uuid) -> Result<Option<DriverRide>, sqlx::Error> {
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM rides WHERE driver_id = $1 AND state::text = ANY($2) LIMIT 1")
        .bind(driver_id)
        .bind(&fuber_db::rides::DRIVER_ACTIVE[..])
        .fetch_optional(db)
        .await?;
    match id {
        Some(id) => driver_ride(db, driver_id, id).await,
        None => Ok(None),
    }
}
