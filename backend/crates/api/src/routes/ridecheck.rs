//! RideCheck answers (PRD S-3, App Flow §6.2). The worker asks (`fuber_worker::ridecheck`);
//! the rider's view of the ride carries the open question (`views::RiderRide::ride_check`).
//!
//! - *Yes, I'm fine* closes it. If the safety desk was already alerted, the incident stays open
//!   for them to close, and they see her answer.
//! - *Call for help* is an SOS (`routes::safety`), with the same texts to her contacts.
//! - Answering twice returns the first answer; only the ride's rider may answer.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RiderUser,
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::safety::{self, Point, SosAlert},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Fine,
    Help,
}

impl Answer {
    fn as_str(self) -> &'static str {
        match self {
            Answer::Fine => "fine",
            Answer::Help => "help",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RideCheckResponse {
    pub answer: Answer,
    /// Where the phone is, for the SOS when she asks for help.
    pub location: Option<Point>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RideCheckResult {
    /// The answer on record (the first one, if she answered before).
    pub answer: Answer,
    /// The SOS raised by *Call for help*.
    pub sos: Option<SosAlert>,
}

#[utoipa::path(
    post, path = "/v1/safety/ridecheck/{id}/respond", tag = "safety", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "The `ride_check.id` from her view of the ride")),
    request_body = RideCheckResponse,
    responses(
        (status = 200, description = "Recorded; *help* also raised an SOS", body = RideCheckResult),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 404, description = "No such check on her rides", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
    )
)]
pub async fn respond(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<RideCheckResponse>,
) -> Result<Json<RideCheckResult>, ApiError> {
    let row: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT c.ride_id, c.answer FROM ride_checks c JOIN rides r ON r.id = c.ride_id WHERE c.id = $1 AND r.rider_id = $2",
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&s.db)
    .await?;
    let Some((ride_id, earlier)) = row else { return Err(ApiError::not_found()) };
    if let Some(earlier) = earlier {
        let answer = if earlier == "help" { Answer::Help } else { Answer::Fine };
        return Ok(Json(RideCheckResult { answer, sos: None }));
    }

    let updated = sqlx::query("UPDATE ride_checks SET answer = $2, answered_at = now() WHERE id = $1 AND answered_at IS NULL")
        .bind(id)
        .bind(req.answer.as_str())
        .execute(&s.db)
        .await?
        .rows_affected();
    // Lost a race with another tap: the first answer stands (and raised any SOS).
    if updated == 0 {
        let answer: String = sqlx::query_scalar("SELECT answer FROM ride_checks WHERE id = $1").bind(id).fetch_one(&s.db).await?;
        return Ok(Json(RideCheckResult { answer: if answer == "help" { Answer::Help } else { Answer::Fine }, sos: None }));
    }
    tracing::warn!(security_event = "ridecheck_answer", ride_id = %ride_id, answer = req.answer.as_str(), "RideCheck answered");
    let event = serde_json::json!({ "kind": "ridecheck", "ride_id": ride_id, "rider_id": user.user_id });
    sqlx::query("SELECT pg_notify('fuber_events', $1)").bind(event.to_string()).execute(&s.db).await?;

    let sos = match req.answer {
        Answer::Fine => None,
        Answer::Help => Some(safety::raise(&s, user.user_id, Some(ride_id), req.location).await?),
    };
    Ok(Json(RideCheckResult { answer: req.answer, sos }))
}
