//! Trip history (PRD R-2), newest first, 20 at a time. Page with `before` = the last item's
//! `requested_at`.
//!
//! The driver's list follows the same privacy rules as her live view (`views`): the exact pickup
//! only if she reached it, the destination only if the PIN was verified. A ride she cancelled
//! goes back to the pool without her, so it is not in her history at all.

use axum::{
    Json,
    extract::{Query, State},
};
use fuber_domain::{Cents, RideState};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{DriverUser, RiderUser},
    error::ApiError,
};

const PAGE: i64 = 20;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// Trips requested before this time (the last item's `requested_at`); omit for the newest.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[param(value_type = Option<String>, format = DateTime)]
    pub before: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RiderTrip {
    pub id: Uuid,
    pub code: String,
    #[schema(value_type = String, example = "RATED")]
    pub state: RideState,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    pub pickup_address: String,
    pub dropoff_address: String,
    pub fare: Cents,
    pub payment_method: String,
    pub driver_first_name: Option<String>,
    /// The stars she gave, if she rated.
    pub my_rating: Option<i16>,
}

type RiderTripRow =
    (Uuid, String, String, OffsetDateTime, Option<OffsetDateTime>, String, String, i64, String, Option<String>, Option<i16>);

#[utoipa::path(
    get, path = "/v1/rides", operation_id = "rider_trips", tag = "history", security(("bearer" = [])),
    params(HistoryQuery),
    responses((status = 200, description = "Her trips, newest first, 20 at a time", body = Vec<RiderTrip>))
)]
pub async fn rider_trips(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<RiderTrip>>, ApiError> {
    let rows: Vec<RiderTripRow> = sqlx::query_as(
        "SELECT r.id, r.code, r.state::text, r.requested_at, r.completed_at, r.pickup_address, r.dropoff_address,
                r.fare_cents, r.payment_method::text,
                CASE WHEN r.state IN ('COMPLETED', 'RATED', 'IN_TRIP', 'PIN_VERIFIED', 'DRIVER_ARRIVED', 'DRIVER_EN_ROUTE', 'DRIVER_ASSIGNED')
                     THEN u.display_name END,
                (SELECT stars FROM ratings g WHERE g.ride_id = r.id AND g.rater = 'rider')
         FROM rides r LEFT JOIN users u ON u.id = r.driver_id
         WHERE r.rider_id = $1 AND ($2::timestamptz IS NULL OR r.requested_at < $2)
         ORDER BY r.requested_at DESC LIMIT $3",
    )
    .bind(user.user_id)
    .bind(q.before)
    .bind(PAGE)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    code,
                    state,
                    requested_at,
                    completed_at,
                    pickup_address,
                    dropoff_address,
                    fare,
                    payment_method,
                    driver,
                    my_rating,
                )| {
                    RiderTrip {
                        id,
                        code,
                        state: RideState::parse(&state).unwrap_or(RideState::Cancelled),
                        requested_at,
                        completed_at,
                        pickup_address,
                        dropoff_address,
                        fare: Cents(fare),
                        payment_method,
                        driver_first_name: driver,
                        my_rating,
                    }
                },
            )
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DriverTrip {
    pub id: Uuid,
    pub code: String,
    #[schema(value_type = String, example = "RATED")]
    pub state: RideState,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    /// Only if she reached the pickup.
    pub pickup_address: Option<String>,
    /// Only if the PIN was verified.
    pub dropoff_address: Option<String>,
    pub earning: Cents,
    pub payment_method: String,
    pub payment_status: Option<String>,
    pub my_rating: Option<i16>,
}

type DriverTripRow =
    (Uuid, String, String, OffsetDateTime, Option<OffsetDateTime>, String, String, i64, String, Option<String>, Option<i16>, bool);

#[utoipa::path(
    get, path = "/v1/driver/trips", tag = "history", security(("bearer" = [])),
    params(HistoryQuery),
    responses((status = 200, description = "Her trips, newest first, 20 at a time", body = Vec<DriverTrip>))
)]
pub async fn driver_trips(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<DriverTrip>>, ApiError> {
    let rows: Vec<DriverTripRow> = sqlx::query_as(
        "SELECT r.id, r.code, r.state::text, r.requested_at, r.completed_at, r.pickup_address, r.dropoff_address,
                r.driver_earning_cents, r.payment_method::text,
                (SELECT status::text FROM payments p WHERE p.ride_id = r.id),
                (SELECT stars FROM ratings g WHERE g.ride_id = r.id AND g.rater = 'driver'),
                r.arrived_at IS NOT NULL
         FROM rides r
         WHERE r.driver_id = $1 AND ($2::timestamptz IS NULL OR r.requested_at < $2)
         ORDER BY r.requested_at DESC LIMIT $3",
    )
    .bind(user.user_id)
    .bind(q.before)
    .bind(PAGE)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    code,
                    state,
                    requested_at,
                    completed_at,
                    pickup,
                    dropoff,
                    earning,
                    payment_method,
                    payment_status,
                    my_rating,
                    arrived,
                )| {
                    let state = RideState::parse(&state).unwrap_or(RideState::Cancelled);
                    // A ride cancelled at the pickup (no-show) did reach the exact pickup; one the rider
                    // cancelled while she was still driving there did not.
                    let reached_pickup = state.driver_sees_exact_pickup() || (state == RideState::Cancelled && arrived);
                    DriverTrip {
                        id,
                        code,
                        state,
                        requested_at,
                        completed_at,
                        pickup_address: reached_pickup.then_some(pickup),
                        dropoff_address: state.driver_sees_destination().then_some(dropoff),
                        earning: Cents(earning),
                        payment_method,
                        payment_status,
                        my_rating,
                    }
                },
            )
            .collect(),
    ))
}
