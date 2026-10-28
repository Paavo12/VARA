//! Booking (PRD B-3 – B-7, P-1): quote, request, the rider's current ride, cancel, try again.
//!
//! A quote prices all three ride types over the OSRM road route at the pickup's demand level
//! and returns a signed token valid for 5 minutes (TRD §4.3). `POST /rides` must present it: the
//! token carries the route, surge and the exact rate rows used, and the fare is recomputed from
//! them with the same pure function, so the price charged is exactly the price shown.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use fuber_common::{
    crypto::{HasExpiry, random_bytes},
    validate::{LatLng, Line, Passengers, PersonName, PhoneE164},
};
use fuber_db::{
    presence::{self, Search},
    rides::{self, RideError},
    settings::Dispatch,
};
use fuber_domain::{
    Demand, Party, RideEvent, RideState, Tier, geo,
    pricing::{self, Fare, Rates},
};
use fuber_integrations::routing::RoutingError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RiderUser,
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    views::{self, RiderRide},
};

const QUOTE_ISSUER: &str = "fuber-quote";
const MIN_TRIP_M: f64 = 100.0;
const GUEST_PHONE_AAD: &str = "rides.guest_phone";

pub type Address = Line<120>;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteRequest {
    pub pickup: LatLng,
    #[schema(value_type = String, example = "Post Street Mall")]
    pub pickup_address: Address,
    pub dropoff: LatLng,
    #[schema(value_type = String, example = "Maerua Mall")]
    pub dropoff_address: Address,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuoteOption {
    pub tier: Tier,
    /// The exact price for this ride type, in cents (PRD B-3).
    pub fare: Fare,
    /// Seconds for the nearest available driver to reach the pickup; null when no driver of
    /// this type is online within the dispatch radius ("No driver online within 3 km").
    pub pickup_eta_s: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Quote {
    /// Present this with `POST /v1/rides` before `expires_at`.
    pub quote: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub distance_m: u32,
    /// Trip time at the current demand level.
    pub duration_s: u32,
    /// `osrm` (real roads) or `estimate` (local development).
    pub routing: &'static str,
    /// `normal`, `high` or `peak`.
    pub demand: &'static str,
    pub currency: &'static str,
    pub options: Vec<QuoteOption>,
}

/// What a quote token commits to. Short field names keep the token small.
#[derive(Debug, Serialize, Deserialize)]
struct QuoteClaims {
    iss: String,
    /// The rider it was issued to.
    sub: Uuid,
    exp: i64,
    /// Pickup and dropoff (lat, lon).
    a: (f64, f64),
    b: (f64, f64),
    pa: String,
    da: String,
    /// Road distance and pricing duration.
    m: u32,
    s: u32,
    /// Surge in basis points and demand level.
    sb: u32,
    d: Demand,
    /// Rate rows (`pricing_tiers.id`) for classic, comfort, softlife.
    t: [Uuid; 3],
}

impl HasExpiry for QuoteClaims {
    fn exp(&self) -> i64 {
        self.exp
    }
}

/// The surge zone covering the pickup, if any: (multiplier in basis points, demand level).
async fn surge_at(db: &PgPool, p: LatLng) -> Result<(u32, Demand), sqlx::Error> {
    let row: Option<(i32, String)> = sqlx::query_as(
        "SELECT (multiplier * 10000)::int, demand_level FROM surge_zones
         WHERE ST_Covers(area, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography)
         ORDER BY multiplier DESC LIMIT 1",
    )
    .bind(p.lat)
    .bind(p.lon)
    .fetch_optional(db)
    .await?;
    Ok(match row {
        Some((bp, level)) => (bp as u32, Demand::parse(&level).unwrap_or(Demand::Normal)),
        None => (10_000, Demand::Normal),
    })
}

/// The rates in force for each tier, with their row ids.
async fn current_rates(db: &PgPool) -> Result<Vec<(Tier, Uuid, Rates)>, sqlx::Error> {
    let rows: Vec<(String, Uuid, i64, i64, i64, i64, i32)> = sqlx::query_as(
        "SELECT DISTINCT ON (tier) tier::text, id, base_cents, per_km_cents, per_min_cents, minimum_cents, service_fee_bp
         FROM pricing_tiers WHERE effective_from <= now() ORDER BY tier, effective_from DESC",
    )
    .fetch_all(db)
    .await?;
    let mut out: Vec<(Tier, Uuid, Rates)> = rows
        .into_iter()
        .filter_map(|(tier, id, base, km, min, minimum, fee)| Some((Tier::parse(&tier)?, id, rates(base, km, min, minimum, fee))))
        .collect();
    out.sort_by_key(|(t, _, _)| Tier::ALL.iter().position(|x| x == t));
    Ok(out)
}

fn rates(base: i64, km: i64, min: i64, minimum: i64, fee: i32) -> Rates {
    use fuber_domain::Cents;
    Rates { base: Cents(base), per_km: Cents(km), per_min: Cents(min), minimum: Cents(minimum), service_fee_bp: fee as u32 }
}

async fn rates_by_id(db: &PgPool, id: Uuid) -> Result<Option<Rates>, sqlx::Error> {
    let row: Option<(i64, i64, i64, i64, i32)> =
        sqlx::query_as("SELECT base_cents, per_km_cents, per_min_cents, minimum_cents, service_fee_bp FROM pricing_tiers WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(b, k, m, mn, f)| rates(b, k, m, mn, f)))
}

fn routing_error(e: RoutingError) -> ApiError {
    match e {
        RoutingError::NoRoute => ApiError::invalid("we can't find a road route between these places"),
        RoutingError::Unavailable(cause) => {
            tracing::error!(%cause, "routing unavailable");
            ApiError::unavailable("routing_unavailable", "We can't price trips right now. Please try again in a minute.")
        }
    }
}

#[utoipa::path(
    post, path = "/v1/rides/quote", tag = "booking", security(("bearer" = [])),
    request_body = QuoteRequest,
    responses(
        (status = 200, description = "Locked prices for every ride type, valid for 5 minutes", body = Quote),
        (status = 422, description = "Outside the service area, too short, or no road route", body = ErrorBody),
        (status = 429, description = "30 quotes a minute", body = ErrorBody),
        (status = 503, description = "`routing_unavailable`", body = ErrorBody),
    )
)]
pub async fn quote(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    ValidJson(req): ValidJson<QuoteRequest>,
) -> Result<Json<Quote>, ApiError> {
    s.limiter.check("quote", &user.user_id.to_string(), s.limits.quote).await?;
    if req.pickup.distance_m(&req.dropoff) < MIN_TRIP_M {
        return Err(ApiError::invalid("pickup and destination are too close together"));
    }
    let cfg = Dispatch::load(&s.db).await?;
    let leg = s.routing.route(req.pickup, req.dropoff).await.map_err(routing_error)?;
    let (surge_bp, demand) = surge_at(&s.db, req.pickup).await?;
    // Prices use the demand level's base traffic, never live traffic, so the price doesn't
    // drift between this screen and Request (demo bug fix, 21 Oct).
    let duration_s = (leg.duration_s as f64 * demand.traffic()).round() as u32;
    let rates = current_rates(&s.db).await?;
    if rates.len() != 3 {
        return Err(ApiError::internal("pricing_tiers is missing a ride type"));
    }

    let mut options = Vec::with_capacity(3);
    for (tier, _, r) in &rates {
        options.push(QuoteOption {
            tier: *tier,
            fare: pricing::fare(r, leg.distance_m, duration_s, surge_bp),
            pickup_eta_s: nearest_eta(&s, &cfg, req.pickup, *tier, demand).await?,
        });
    }
    let expires_at = OffsetDateTime::now_utc() + Duration::seconds(cfg.quote_ttl_s);
    let claims = QuoteClaims {
        iss: QUOTE_ISSUER.into(),
        sub: user.user_id,
        exp: expires_at.unix_timestamp(),
        a: (req.pickup.lat, req.pickup.lon),
        b: (req.dropoff.lat, req.dropoff.lon),
        pa: req.pickup_address.into(),
        da: req.dropoff_address.into(),
        m: leg.distance_m,
        s: duration_s,
        sb: surge_bp,
        d: demand,
        t: [rates[0].1, rates[1].1, rates[2].1],
    };
    Ok(Json(Quote {
        quote: s.keys.tokens.sign(&claims),
        expires_at,
        distance_m: leg.distance_m,
        duration_s,
        routing: s.routing.source(),
        demand: demand.as_str(),
        currency: "NAD",
        options,
    }))
}

/// Road ETA of the nearest free driver of this type, or None if nobody is within the radius.
async fn nearest_eta(s: &AppState, cfg: &Dispatch, pickup: LatLng, tier: Tier, demand: Demand) -> Result<Option<i64>, ApiError> {
    let found = presence::candidates(
        &s.db,
        &Search {
            lat: pickup.lat,
            lon: pickup.lon,
            tier: tier.as_str(),
            wheelchair: false,
            radius_m: cfg.radius_m,
            stale_s: cfg.location_stale_s,
            exclude: &[],
            rider_id: None,
            limit: 3,
        },
    )
    .await?;
    let positions: Vec<LatLng> = found.iter().map(|c| LatLng { lat: c.lat, lon: c.lon }).collect();
    let legs = match s.routing.to_one(&positions, pickup).await {
        Ok(l) => l,
        // ETAs are a nicety on the options screen; prices still work without them.
        Err(e) => {
            tracing::warn!(error = %e, "pickup ETAs unavailable");
            return Ok(None);
        }
    };
    Ok(legs.into_iter().flatten().map(|l| (l.duration_s as f64 * demand.traffic()).round() as i64).min())
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Guest {
    #[schema(value_type = String, example = "Ndapandula Shikongo")]
    pub name: PersonName,
    /// Receives the driver, plate and PIN by SMS once a driver accepts (PRD B-5).
    #[schema(value_type = String, example = "+264811234567")]
    pub phone: PhoneE164,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    Cash,
    Card,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RideRequest {
    /// From `POST /v1/rides/quote`.
    pub quote: String,
    pub tier: Tier,
    #[schema(value_type = i64, minimum = 1, maximum = 4)]
    pub passengers: Passengers,
    /// Only wheelchair-accessible vehicles (PRD B-4).
    #[serde(default)]
    pub wheelchair: bool,
    /// Booking for someone else.
    pub guest: Option<Guest>,
    pub payment_method: PaymentMethod,
    /// Book ahead: 30 minutes to 7 days from now (PRD B-6).
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub scheduled_for: Option<OffsetDateTime>,
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|v| (8..=80).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .map(str::to_owned)
        .ok_or_else(|| ApiError::invalid("an Idempotency-Key header (8–80 letters, digits, - or _) is required"))
}

fn pin() -> String {
    let n = u32::from_le_bytes(random_bytes::<4>()) % 10_000;
    format!("{n:04}")
}

/// Uniform in [0, 1).
fn unit() -> f64 {
    (u64::from_le_bytes(random_bytes::<8>()) >> 11) as f64 / (1u64 << 53) as f64
}

#[utoipa::path(
    post, path = "/v1/rides", operation_id = "create_ride", tag = "booking", security(("bearer" = [])),
    request_body = RideRequest,
    params(("Idempotency-Key" = String, Header, description = "Random per booking attempt; a retry with the same key returns the same ride")),
    responses(
        (status = 201, description = "Requested (searching) or scheduled", body = RiderRide),
        (status = 200, description = "Same Idempotency-Key as an earlier request: that ride", body = RiderRide),
        (status = 403, description = "`not_verified`: identity verification must be approved first", body = ErrorBody),
        (status = 409, description = "`ride_in_progress`", body = ErrorBody),
        (status = 422, description = "Invalid or expired quote, bad schedule time, unsupported payment", body = ErrorBody),
        (status = 429, description = "5 a minute, 30 an hour", body = ErrorBody),
    )
)]
pub async fn create(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    headers: HeaderMap,
    ValidJson(req): ValidJson<RideRequest>,
) -> Result<(StatusCode, Json<RiderRide>), ApiError> {
    let key = idempotency_key(&headers)?;
    // A retry of a request that already went through is answered before the rate limit.
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM rides WHERE rider_id = $1 AND idempotency_key = $2")
        .bind(user.user_id)
        .bind(&key)
        .fetch_optional(&s.db)
        .await?
    {
        let ride = views::rider_ride(&s.db, user.user_id, id).await?.ok_or_else(ApiError::not_found)?;
        return Ok((StatusCode::OK, Json(ride)));
    }
    let uid = user.user_id.to_string();
    s.limiter.check("ride_request", &uid, s.limits.ride_request).await?;
    s.limiter.check("ride_request_hourly", &uid, s.limits.ride_request_hourly).await?;

    let (status, verification): (String, String) =
        sqlx::query_as("SELECT status::text, verification::text FROM users WHERE id = $1").bind(user.user_id).fetch_one(&s.db).await?;
    if verification != "approved" || status != "active" {
        return Err(ApiError::forbidden("not_verified", "Your identity must be verified before you can book a ride."));
    }
    let q: QuoteClaims = s
        .keys
        .tokens
        .verify(&req.quote, OffsetDateTime::now_utc().unix_timestamp())
        .ok()
        .filter(|q: &QuoteClaims| q.iss == QUOTE_ISSUER && q.sub == user.user_id)
        .ok_or_else(|| ApiError::invalid("this price has expired; please check the price again"))?;
    if matches!(req.payment_method, PaymentMethod::Card) {
        // Card payments (pre-authorisation, PRD P-2) arrive with the payment provider in Phase 4.
        return Err(ApiError::invalid("card payments are not available yet; please choose cash"));
    }

    let cfg = Dispatch::load(&s.db).await?;
    let now = OffsetDateTime::now_utc();
    if let Some(at) = req.scheduled_for {
        let ahead = (at - now).whole_seconds();
        if ahead < cfg.schedule_min_s || ahead > cfg.schedule_max_s {
            return Err(ApiError::invalid("a scheduled ride must be between 30 minutes and 7 days from now"));
        }
    }

    let tier_index = Tier::ALL.iter().position(|t| *t == req.tier).expect("every tier is in ALL");
    let pricing_tier_id = q.t[tier_index];
    let rates = rates_by_id(&s.db, pricing_tier_id)
        .await?
        .ok_or_else(|| ApiError::invalid("this price has expired; please check the price again"))?;
    let fare = pricing::fare(&rates, q.m, q.s, q.sb);
    let (fuzz_lat, fuzz_lon) = geo::fuzz(q.a.0, q.a.1, cfg.fuzz_min_m, cfg.fuzz_max_m, unit(), unit());
    let (guest_name, guest_phone) = match &req.guest {
        Some(g) => (Some(g.name.as_str().to_owned()), Some(s.keys.sealer.seal_str(GUEST_PHONE_AAD, g.phone.as_str()))),
        None => (None, None),
    };
    let initial = if req.scheduled_for.is_some() { RideState::Scheduled } else { RideState::Requested };

    let mut tx = s.db.begin().await?;
    let inserted: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO rides (rider_id, state, tier, passengers, wheelchair, guest_name, guest_phone,
                            pickup, pickup_address, pickup_fuzzed, dropoff, dropoff_address,
                            quote_distance_m, quote_duration_s, pricing_tier_id, surge_multiplier,
                            fare_cents, fare_breakdown, service_fee_cents, driver_earning_cents, payment_method,
                            pin, scheduled_for, idempotency_key, demand_level)
         VALUES ($1, $2::ride_state, $3::ride_tier, $4, $5, $6, $7,
                 ST_SetSRID(ST_MakePoint($9, $8), 4326)::geography, $10,
                 ST_SetSRID(ST_MakePoint($12, $11), 4326)::geography,
                 ST_SetSRID(ST_MakePoint($14, $13), 4326)::geography, $15,
                 $16, $17, $18, $19 / 10000.0, $20, $21, $22, $23, $24::payment_method, $25, $26, $27, $28)
         RETURNING id",
    )
    .bind(user.user_id)
    .bind(initial.as_str())
    .bind(req.tier.as_str())
    .bind(req.passengers.get() as i16)
    .bind(req.wheelchair)
    .bind(&guest_name)
    .bind(&guest_phone)
    .bind(q.a.0)
    .bind(q.a.1)
    .bind(&q.pa)
    .bind(fuzz_lat)
    .bind(fuzz_lon)
    .bind(q.b.0)
    .bind(q.b.1)
    .bind(&q.da)
    .bind(q.m as i32)
    .bind(q.s as i32)
    .bind(pricing_tier_id)
    .bind(q.sb as i32)
    .bind(fare.total.0)
    .bind(json!(fare))
    .bind(fare.service_fee.0)
    .bind(fare.driver_earning.0)
    .bind("cash")
    .bind(pin())
    .bind(req.scheduled_for)
    .bind(&key)
    .bind(q.d.as_str())
    .fetch_one(&mut *tx)
    .await;
    let ride_id = match inserted {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("one_active_ride_rider") => {
            return Err(ApiError::conflict("ride_in_progress", "You already have a ride in progress."));
        }
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("rides_idempotency") => {
            // The same request raced itself: answer with the ride the other one created.
            drop(tx);
            let id: Uuid = sqlx::query_scalar("SELECT id FROM rides WHERE rider_id = $1 AND idempotency_key = $2")
                .bind(user.user_id)
                .bind(&key)
                .fetch_one(&s.db)
                .await?;
            let ride = views::rider_ride(&s.db, user.user_id, id).await?.ok_or_else(ApiError::not_found)?;
            return Ok((StatusCode::OK, Json(ride)));
        }
        Err(e) => return Err(e.into()),
    };
    rides::record(&mut *tx, ride_id, None, initial, Party::Rider, Some(user.user_id), None).await?;
    if initial == RideState::Requested {
        rides::transition(&mut tx, ride_id, RideEvent::StartSearch, Party::System, None, None).await.map_err(ride_error)?;
        sqlx::query("UPDATE rides SET search_started_at = now() WHERE id = $1").bind(ride_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    tracing::info!(%ride_id, tier = req.tier.as_str(), scheduled = req.scheduled_for.is_some(), "ride requested");
    let ride = views::rider_ride(&s.db, user.user_id, ride_id).await?.ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(ride)))
}

pub(crate) fn ride_error(e: RideError) -> ApiError {
    match e {
        RideError::NotFound => ApiError::not_found(),
        RideError::Invalid(t) => ApiError::conflict(
            "invalid_state",
            format!("This ride can't do that now (it is {}).", t.state.as_str().to_lowercase().replace('_', " ")),
        ),
        RideError::Db(e) => e.into(),
    }
}

// ---------------------------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/v1/rides/active", tag = "booking", security(("bearer" = [])),
    responses(
        (status = 200, description = "The rider's active or scheduled ride", body = RiderRide),
        (status = 204, description = "No active ride"),
    )
)]
pub async fn active(State(s): State<AppState>, RiderUser(user): RiderUser) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    Ok(match views::rider_current(&s.db, user.user_id).await? {
        Some(r) => Json(r).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

#[utoipa::path(
    get, path = "/v1/rides/{id}", operation_id = "get_ride", tag = "booking", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 200, body = RiderRide), (status = 404, description = "Not found, or not her ride", body = ErrorBody))
)]
pub async fn get(State(s): State<AppState>, RiderUser(user): RiderUser, Path(id): Path<Uuid>) -> Result<Json<RiderRide>, ApiError> {
    views::rider_ride(&s.db, user.user_id, id).await?.map(Json).ok_or_else(ApiError::not_found)
}

/// The ride's rider, or 404 (someone else's ride looks exactly like a missing one).
async fn own_ride(db: &PgPool, rider_id: Uuid, ride_id: Uuid) -> Result<(), ApiError> {
    let mine: Option<bool> =
        sqlx::query_scalar("SELECT rider_id = $2 FROM rides WHERE id = $1").bind(ride_id).bind(rider_id).fetch_optional(db).await?;
    if mine == Some(true) { Ok(()) } else { Err(ApiError::not_found()) }
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/cancel", tag = "booking", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Cancelled. Free, but every cancellation costs priority (PRD D-5)", body = RiderRide),
        (status = 404, body = ErrorBody),
        (status = 409, description = "`invalid_state`: the trip has started", body = ErrorBody),
    )
)]
pub async fn cancel(State(s): State<AppState>, RiderUser(user): RiderUser, Path(id): Path<Uuid>) -> Result<Json<RiderRide>, ApiError> {
    own_ride(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    let from = rides::lock(&mut tx, id).await.map_err(ride_error)?;
    let assigned = from.locks_driver();
    rides::transition(&mut tx, id, RideEvent::Cancel { by: Party::Rider }, Party::Rider, Some(user.user_id), None)
        .await
        .map_err(ride_error)?;
    sqlx::query("UPDATE rides SET cancelled_by = 'rider' WHERE id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE ride_offers SET response = 'withdrawn', responded_at = now() WHERE ride_id = $1 AND response = 'pending'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // Cancelling is free; instead every cancellation costs the rider priority points (demo
    // `cancelByRider`). A scheduled ride cancelled before dispatch costs nothing.
    if from != RideState::Scheduled {
        let lost = rides::penalise(&mut tx, user.user_id, Party::Rider, id, "cancel").await?;
        tracing::info!(ride_id = %id, assigned, lost, "rider cancelled");
    }
    tx.commit().await?;
    views::rider_ride(&s.db, user.user_id, id).await?.map(Json).ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/retry", tag = "booking", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Searching again; drivers who passed before may be asked again", body = RiderRide),
        (status = 404, body = ErrorBody),
        (status = 409, description = "`invalid_state`: only after *No drivers accepted*", body = ErrorBody),
    )
)]
pub async fn retry(State(s): State<AppState>, RiderUser(user): RiderUser, Path(id): Path<Uuid>) -> Result<Json<RiderRide>, ApiError> {
    own_ride(&s.db, user.user_id, id).await?;
    let mut tx = s.db.begin().await?;
    rides::transition(&mut tx, id, RideEvent::RetrySearch, Party::Rider, Some(user.user_id), None).await.map_err(ride_error)?;
    sqlx::query("UPDATE rides SET search_round = search_round + 1, search_started_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    views::rider_ride(&s.db, user.user_id, id).await?.map(Json).ok_or_else(ApiError::not_found)
}
