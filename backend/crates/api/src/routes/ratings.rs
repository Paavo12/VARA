//! Ratings and favourites (PRD R-1, App Flow §8).
//!
//! Both sides rate 1–5 stars after a completed trip; below 3 a reason is required. The rider may
//! add the driver to her favourites (matching gives favourites a bonus). The driver may skip
//! rating. The ride becomes `RATED` once the rider has rated and the driver has rated or skipped.
//! Each rating updates the other person's average.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_common::validate::{PlainText, Stars};
use fuber_db::rides::{self, RideError};
use fuber_domain::{Party, RideEvent};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser, DriverUser, RiderUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::rides::ride_error,
};

/// Why a trip got fewer than 3 stars. The rider rates the driver, the driver rates the rider;
/// some reasons apply to one side only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RatingReason {
    /// Rider about driver.
    UnsafeDriving,
    /// Rider about driver.
    CarCondition,
    /// Rider about driver.
    Route,
    /// Driver about rider.
    Damage,
    Late,
    Rude,
    FeltUnsafe,
    Other,
}

impl RatingReason {
    fn as_str(self) -> &'static str {
        match self {
            RatingReason::UnsafeDriving => "unsafe_driving",
            RatingReason::CarCondition => "car_condition",
            RatingReason::Route => "route",
            RatingReason::Damage => "damage",
            RatingReason::Late => "late",
            RatingReason::Rude => "rude",
            RatingReason::FeltUnsafe => "felt_unsafe",
            RatingReason::Other => "other",
        }
    }

    fn allowed_for(self, rater: App) -> bool {
        match self {
            RatingReason::UnsafeDriving | RatingReason::CarCondition | RatingReason::Route => rater == App::Rider,
            RatingReason::Damage => rater == App::Driver,
            _ => true,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RatingRequest {
    #[schema(value_type = i64, minimum = 1, maximum = 5)]
    pub stars: Stars,
    /// Required below 3 stars.
    #[serde(default)]
    pub reasons: Vec<RatingReason>,
    #[schema(value_type = Option<String>)]
    pub comment: Option<PlainText<500>>,
    /// Rider only: add this driver to her favourites.
    #[serde(default)]
    pub favourite: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Rated {
    pub ride_id: Uuid,
    /// `COMPLETED`, or `RATED` once both sides are done.
    pub state: String,
}

/// Rated is reached when the rider has rated and the driver has rated or skipped.
async fn finish_if_done(conn: &mut PgConnection, ride_id: Uuid) -> Result<String, RideError> {
    let (state, done): (String, bool) = sqlx::query_as(
        "SELECT state::text,
                EXISTS (SELECT 1 FROM ratings WHERE ride_id = r.id AND rater = 'rider')
                AND (r.driver_rating_skipped OR EXISTS (SELECT 1 FROM ratings WHERE ride_id = r.id AND rater = 'driver'))
         FROM rides r WHERE id = $1",
    )
    .bind(ride_id)
    .fetch_one(&mut *conn)
    .await?;
    if done && state == "COMPLETED" {
        rides::transition(conn, ride_id, RideEvent::RatingsDone, Party::System, None, None).await?;
        return Ok("RATED".into());
    }
    Ok(state)
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/rating", tag = "ratings", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = RatingRequest,
    responses(
        (status = 200, description = "Rated", body = Rated),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`already_rated` or `not_completed`", body = ErrorBody),
        (status = 422, description = "No reason below 3 stars, a reason for the other side, or `favourite` from the driver app", body = ErrorBody),
    )
)]
pub async fn rate(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<RatingRequest>,
) -> Result<Json<Rated>, ApiError> {
    let stars = req.stars.get();
    if stars < 3 && req.reasons.is_empty() {
        return Err(ApiError::invalid("tell us what went wrong: choose at least one reason"));
    }
    if req.reasons.iter().any(|r| !r.allowed_for(user.app)) {
        return Err(ApiError::invalid("one of the reasons doesn't apply to this side of the trip"));
    }
    if req.favourite && user.app != App::Rider {
        return Err(ApiError::invalid("only passengers can add favourites"));
    }
    let mut tx = s.db.begin().await?;
    let own_column = if user.app == App::Rider { "rider_id" } else { "driver_id" };
    let ride: Option<(String, Uuid, Option<Uuid>)> =
        sqlx::query_as(&format!("SELECT state::text, rider_id, driver_id FROM rides WHERE id = $1 AND {own_column} = $2 FOR UPDATE"))
            .bind(id)
            .bind(user.user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (state, rider_id, driver_id) = ride.ok_or_else(ApiError::not_found)?;
    if state != "COMPLETED" && state != "RATED" {
        return Err(ApiError::conflict("not_completed", "You can rate a trip once it's complete."));
    }
    let driver_id = driver_id.ok_or_else(ApiError::not_found)?;
    let (rater, rated_table, rated_id) = match user.app {
        App::Rider => ("rider", "driver_profiles", driver_id),
        App::Driver => ("driver", "rider_profiles", rider_id),
    };
    let reasons: Vec<&str> = req.reasons.iter().map(|r| r.as_str()).collect();
    let inserted = sqlx::query(
        "INSERT INTO ratings (ride_id, rater, stars, reasons, comment) VALUES ($1, $2::party, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(rater)
    .bind(stars as i16)
    .bind(&reasons)
    .bind(req.comment.as_ref().map(|c| c.as_str()))
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::conflict("already_rated", "You've already rated this trip."));
    }
    // Running average, two decimals, as shown in the apps ("★ 4.92").
    sqlx::query(&format!(
        "UPDATE {rated_table} SET rating_avg = round((coalesce(rating_avg, 0) * rating_count + $2) / (rating_count + 1), 2),
                                 rating_count = rating_count + 1
         WHERE user_id = $1"
    ))
    .bind(rated_id)
    .bind(stars as i32)
    .execute(&mut *tx)
    .await?;
    if req.favourite {
        sqlx::query("INSERT INTO favourite_drivers (rider_id, driver_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(rider_id)
            .bind(driver_id)
            .execute(&mut *tx)
            .await?;
    }
    let state = finish_if_done(&mut tx, id).await.map_err(ride_error)?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, rater, stars, "trip rated");
    Ok(Json(Rated { ride_id: id, state }))
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/rating/skip", tag = "ratings", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Skipped (the driver's \"Back to requests\")", body = Rated),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`not_completed`", body = ErrorBody),
    )
)]
pub async fn skip(State(s): State<AppState>, DriverUser(user): DriverUser, Path(id): Path<Uuid>) -> Result<Json<Rated>, ApiError> {
    let mut tx = s.db.begin().await?;
    let state: Option<String> = sqlx::query_scalar("SELECT state::text FROM rides WHERE id = $1 AND driver_id = $2 FOR UPDATE")
        .bind(id)
        .bind(user.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    let state = state.ok_or_else(ApiError::not_found)?;
    if state != "COMPLETED" && state != "RATED" {
        return Err(ApiError::conflict("not_completed", "You can rate a trip once it's complete."));
    }
    sqlx::query("UPDATE rides SET driver_rating_skipped = true WHERE id = $1").bind(id).execute(&mut *tx).await?;
    let state = finish_if_done(&mut tx, id).await.map_err(ride_error)?;
    tx.commit().await?;
    Ok(Json(Rated { ride_id: id, state }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FavouriteDriver {
    pub driver_id: Uuid,
    pub first_name: String,
    pub rating: Option<f64>,
    #[serde(with = "time::serde::rfc3339")]
    pub added_at: OffsetDateTime,
}

#[utoipa::path(
    get, path = "/v1/me/favourite-drivers", tag = "ratings", security(("bearer" = [])),
    responses((status = 200, description = "Her favourite drivers; matching prefers them", body = Vec<FavouriteDriver>))
)]
pub async fn favourites(State(s): State<AppState>, RiderUser(user): RiderUser) -> Result<Json<Vec<FavouriteDriver>>, ApiError> {
    let rows: Vec<(Uuid, Option<String>, Option<f64>, OffsetDateTime)> = sqlx::query_as(
        "SELECT f.driver_id, u.display_name, dp.rating_avg::float8, f.created_at
         FROM favourite_drivers f JOIN users u ON u.id = f.driver_id JOIN driver_profiles dp ON dp.user_id = f.driver_id
         WHERE f.rider_id = $1 AND u.status <> 'deleted' ORDER BY f.created_at DESC",
    )
    .bind(user.user_id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(driver_id, name, rating, added_at)| FavouriteDriver {
                driver_id,
                first_name: name.unwrap_or_else(|| "A VARA driver".into()),
                rating,
                added_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    delete, path = "/v1/me/favourite-drivers/{driver_id}", tag = "ratings", security(("bearer" = [])),
    params(("driver_id" = Uuid, Path)),
    responses((status = 204, description = "Removed (also when she wasn't a favourite)"))
)]
pub async fn remove_favourite(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    Path(driver_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM favourite_drivers WHERE rider_id = $1 AND driver_id = $2")
        .bind(user.user_id)
        .bind(driver_id)
        .execute(&s.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
