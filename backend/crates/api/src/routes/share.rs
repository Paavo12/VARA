//! Share trip (PRD S-2, App Flow §6.3): the rider or the driver makes a live link to her trip
//! and sends it with the phone's share sheet or to her emergency contacts. Whoever opens
//! `vara.example/t/<token>` sees the car moving, the driver's first name, car and plate, the
//! destination area and the ETA, with a button to call the police.
//!
//! - **The token is the key**: 256 random bits, only its hash is stored, and the page needs no
//!   sign-in. Looking one up is rate-limited per IP (TRD §7.1).
//! - **What the page never shows**: the exact pickup or destination, the PIN, anyone's phone
//!   number, the rider's name when the driver shared, or the car once the ride has ended.
//! - **Expiry**: 30 minutes after the ride ends. A link the driver made dies at once if she
//!   cancels and the ride goes to someone else; a rider's link follows her ride.
//! - SOS texts carry the same kind of link (`routes::safety`).

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use fuber_common::{crypto::random_token, validate::LatLng};
use fuber_db::{rides::DRIVER_ACTIVE, safety};
use fuber_domain::safety::{POLICE_PHONE, SHARE_AFTER_END_S, share_text};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::places,
};

/// Length of a link token: 32 random bytes, base64url without padding.
const TOKEN_LEN: usize = 43;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ShareRequest {
    /// Also text the link to her emergency contacts (*Send to my emergency contacts*).
    #[serde(default)]
    pub send_to_contacts: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TripShare {
    /// The link to send, e.g. `https://vara.example/t/<token>`.
    pub url: String,
    /// The text for the share sheet.
    pub text: String,
    /// Emergency contacts being texted the link (0 unless asked for).
    pub contacts_texted: usize,
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/share", operation_id = "share_trip", tag = "safety", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = ShareRequest,
    responses(
        (status = 200, description = "A new live link to her trip; each call makes another", body = TripShare),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "The ride has no driver yet or has ended (`not_shareable`)", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
        (status = 429, description = "Too many links", body = ErrorBody),
    )
)]
pub async fn create(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<ShareRequest>,
) -> Result<Json<TripShare>, ApiError> {
    s.limiter.check("trip_share_create", &user.user_id.to_string(), s.limits.trip_share_create).await?;
    let (column, party) = match user.app {
        App::Rider => ("rider_id", "rider"),
        App::Driver => ("driver_id", "driver"),
    };
    let row: Option<(String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT r.state::text, u.display_name FROM rides r JOIN users u ON u.id = r.{column} WHERE r.id = $1 AND r.{column} = $2"
    ))
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&s.db)
    .await?;
    let Some((state, name)) = row else { return Err(ApiError::not_found()) };
    // From the moment a driver accepts until drop-off: before that there is no car to follow.
    if !DRIVER_ACTIVE.contains(&state.as_str()) {
        return Err(ApiError::conflict("not_shareable", "You can share your trip once a driver has accepted it."));
    }

    let token = random_token();
    let mut conn = s.db.acquire().await?;
    safety::create_share(&mut conn, id, &safety::share_hash(&s.keys.hasher, &token), party).await?;
    let url = format!("{}/{token}", s.config.trip_link_base);
    let text = share_text(name.as_deref().unwrap_or("Someone"), &url);

    let mut contacts = vec![];
    if req.send_to_contacts {
        contacts = safety::contact_phones(&mut conn, &s.keys.sealer, user.user_id).await?;
    }
    drop(conn);
    tracing::info!(security_event = "trip_shared", user_id = %user.user_id, ride_id = %id, contacts = contacts.len(), "trip link created");

    let n = contacts.len();
    if n > 0 {
        let (state, body) = (s.clone(), text.clone());
        tokio::spawn(async move {
            for to in contacts {
                if let Err(e) = state.sms.send(&to, &body).await {
                    tracing::warn!(error = %e, ride_id = %id, "trip link text failed");
                }
            }
        });
    }
    Ok(Json(TripShare { url, text, contacts_texted: n }))
}

/// Where the shared trip is, in words a contact understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShareStatus {
    /// The driver is on her way to the pickup.
    DriverOnTheWay,
    /// The driver is at the pickup.
    DriverArrived,
    /// On the way to the destination.
    InTrip,
    /// The ride's driver cancelled and VARA is finding another (rider links only).
    FindingDriver,
    /// The safety team is handling the trip (five wrong PINs).
    WithSafetyTeam,
    /// Dropped off, or the ride was cancelled.
    Ended,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SharedCar {
    pub make: String,
    pub model: String,
    pub colour: String,
    pub plate: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SharedPosition {
    pub lat: f64,
    pub lon: f64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

/// The public trip page's data. Nothing here identifies the rider beyond her first name.
#[derive(Debug, Serialize, ToSchema)]
pub struct SharedTrip {
    pub status: ShareStatus,
    /// First name of whoever shared the link.
    pub shared_by: String,
    /// `rider` or `driver`.
    pub shared_by_party: String,
    /// The driver's first name.
    pub driver_first_name: Option<String>,
    pub car: Option<SharedCar>,
    /// The car's last reported position; absent once the trip has ended.
    pub position: Option<SharedPosition>,
    /// "Near Maerua Mall" or "Klein Windhoek area", never the address.
    pub destination_area: String,
    /// Expected arrival at the pickup (before it) or at the destination (in the trip).
    #[serde(with = "time::serde::rfc3339::option")]
    pub eta: Option<OffsetDateTime>,
    /// When the link stops working, once the ride has ended.
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    #[schema(example = "10111")]
    pub police_phone: &'static str,
}

#[derive(sqlx::FromRow)]
struct ShareRow {
    state: String,
    created_by: String,
    /// The ride's driver is the one the link was made with.
    same_driver: bool,
    sharer: Option<String>,
    driver_name: Option<String>,
    make: Option<String>,
    model: Option<String>,
    colour: Option<String>,
    plate: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    located_at: Option<OffsetDateTime>,
    dropoff_lat: f64,
    dropoff_lon: f64,
    eta: Option<OffsetDateTime>,
    ended_at: Option<OffsetDateTime>,
    expired: bool,
}

#[utoipa::path(
    get, path = "/v1/trip-shares/{token}", operation_id = "shared_trip", tag = "safety",
    params(("token" = String, Path, description = "The token from the link")),
    responses(
        (status = 200, description = "The shared trip, live. Poll every 10 s", body = SharedTrip),
        (status = 404, description = "No such link, or it was stopped", body = ErrorBody),
        (status = 410, description = "The link expired 30 minutes after the trip ended (`link_expired`)", body = ErrorBody),
        (status = 429, description = "Too many lookups from this address", body = ErrorBody),
    )
)]
pub async fn view(State(s): State<AppState>, Path(token): Path<String>) -> Result<Response, ApiError> {
    if token.len() != TOKEN_LEN || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(ApiError::not_found());
    }
    let hash = safety::share_hash(&s.keys.hasher, &token);
    let row: Option<ShareRow> = sqlx::query_as(
        "WITH s AS (
           SELECT ts.ride_id, ts.created_by, ts.driver_id, ts.expires_at FROM trip_shares ts
           WHERE ts.token_hash = $1 AND ts.revoked_at IS NULL
         ), ride AS (
           SELECT r.*, CASE WHEN r.state IN ('COMPLETED', 'RATED', 'CANCELLED')
                            THEN coalesce(r.completed_at, (SELECT max(e.at) FROM ride_events e WHERE e.ride_id = r.id), r.updated_at)
                       END AS ended_at
           FROM rides r JOIN s ON s.ride_id = r.id
         )
         SELECT r.state::text AS state, s.created_by::text AS created_by,
                r.driver_id IS NOT DISTINCT FROM s.driver_id AS same_driver,
                sharer.display_name AS sharer, du.display_name AS driver_name,
                v.make, v.model, v.colour, v.plate,
                ST_Y(p.location::geometry) AS lat, ST_X(p.location::geometry) AS lon, p.updated_at AS located_at,
                ST_Y(r.dropoff::geometry) AS dropoff_lat, ST_X(r.dropoff::geometry) AS dropoff_lon,
                CASE WHEN r.state = 'IN_TRIP' THEN r.started_at + make_interval(secs => r.quote_duration_s)
                     WHEN r.state IN ('DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE') THEN
                       (SELECT r.assigned_at + make_interval(secs => o.eta_s) FROM ride_offers o
                        WHERE o.ride_id = r.id AND o.driver_id = r.driver_id AND o.response = 'accepted'
                        ORDER BY o.offered_at DESC LIMIT 1)
                END AS eta,
                r.ended_at,
                coalesce(r.ended_at + make_interval(secs => $2) <= now(), false) OR coalesce(s.expires_at <= now(), false) AS expired
         FROM s JOIN ride r ON true
         JOIN users sharer ON sharer.id = CASE WHEN s.created_by = 'driver' THEN s.driver_id ELSE r.rider_id END
         LEFT JOIN users du ON du.id = r.driver_id
         LEFT JOIN vehicles v ON v.id = r.vehicle_id
         LEFT JOIN driver_presence p ON p.driver_id = r.driver_id",
    )
    .bind(&hash)
    .bind(SHARE_AFTER_END_S as f64)
    .fetch_optional(&s.db)
    .await?;
    let Some(row) = row else { return Err(ApiError::not_found()) };
    // A driver's link is hers alone: once the ride has moved to someone else it is gone.
    if row.created_by == "driver" && !row.same_driver {
        return Err(ApiError::not_found());
    }
    if row.expired {
        return Err(ApiError::new(StatusCode::GONE, "link_expired", "This trip has ended and the link has expired."));
    }

    let status = match row.state.as_str() {
        "DRIVER_ASSIGNED" | "DRIVER_EN_ROUTE" => ShareStatus::DriverOnTheWay,
        "DRIVER_ARRIVED" | "PIN_VERIFIED" => ShareStatus::DriverArrived,
        "IN_TRIP" => ShareStatus::InTrip,
        "BLOCKED" => ShareStatus::WithSafetyTeam,
        "REQUESTED" | "SEARCHING" | "NO_DRIVERS" | "SCHEDULED" => ShareStatus::FindingDriver,
        _ => ShareStatus::Ended,
    };
    let live = matches!(status, ShareStatus::DriverOnTheWay | ShareStatus::DriverArrived | ShareStatus::InTrip);
    let has_driver = live || status == ShareStatus::WithSafetyTeam;
    let destination_area = places::approx_area(&s.db, LatLng { lat: row.dropoff_lat, lon: row.dropoff_lon }).await?;
    let car = match (row.make, row.model, row.colour, row.plate) {
        (Some(make), Some(model), Some(colour), Some(plate)) if has_driver => Some(SharedCar { make, model, colour, plate }),
        _ => None,
    };
    let trip = SharedTrip {
        status,
        shared_by: row.sharer.unwrap_or_else(|| "Someone".into()),
        shared_by_party: row.created_by,
        driver_first_name: row.driver_name.filter(|_| has_driver),
        car,
        position: match (live, row.lat, row.lon, row.located_at) {
            (true, Some(lat), Some(lon), Some(at)) => Some(SharedPosition { lat, lon, at }),
            _ => None,
        },
        destination_area,
        eta: row.eta.filter(|_| live),
        expires_at: row.ended_at.map(|t| t + time::Duration::seconds(SHARE_AFTER_END_S)),
        police_phone: POLICE_PHONE,
    };
    let mut res = Json(trip).into_response();
    // The page polls; nothing in between may keep a copy of where she is.
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}
