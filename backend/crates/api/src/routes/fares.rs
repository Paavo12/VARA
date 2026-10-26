//! `POST /v1/fares/estimate` — prices all three ride types between two points.
//!
//! Phase 0 estimates road distance as straight-line × 1.3 and always uses normal demand.
//! Phase 2 replaces this with OSRM routes and surge zones and turns it into the signed,
//! locked `POST /v1/rides/quote` (TRD §4.3).

use axum::{Json, extract::State};
use fuber_common::validate::LatLng;
use fuber_domain::{
    Demand, Tier,
    pricing::{self, Fare, Rates},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, ErrorBody},
    extract::ValidJson,
};

const ROAD_FACTOR: f64 = 1.3;
const MIN_TRIP_M: f64 = 100.0;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EstimateRequest {
    pub pickup: LatLng,
    pub dropoff: LatLng,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TierEstimate {
    pub tier: Tier,
    pub fare: Fare,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EstimateResponse {
    pub distance_m: u32,
    pub duration_s: u32,
    pub distance_source: &'static str,
    /// Amounts are integer cents of NAD.
    pub currency: &'static str,
    pub estimates: Vec<TierEstimate>,
}

#[utoipa::path(
    post,
    path = "/v1/fares/estimate",
    tag = "fares",
    request_body = EstimateRequest,
    responses(
        (status = 200, description = "Fares for every ride type, in integer cents of NAD", body = EstimateResponse),
        (status = 422, description = "Invalid request (unknown field, outside service area, too short)", body = ErrorBody),
        (status = 429, description = "Rate limited; see Retry-After", body = ErrorBody),
    )
)]
pub async fn estimate(State(_): State<AppState>, ValidJson(req): ValidJson<EstimateRequest>) -> Result<Json<EstimateResponse>, ApiError> {
    let straight = req.pickup.distance_m(&req.dropoff);
    if straight < MIN_TRIP_M {
        return Err(ApiError::invalid("pickup and destination are too close together"));
    }
    let demand = Demand::Normal;
    let distance_m = (straight * ROAD_FACTOR).round() as u32;
    let duration_s = pricing::pricing_duration_s(distance_m, demand.traffic());
    let estimates = Tier::ALL
        .into_iter()
        .map(|tier| TierEstimate { tier, fare: pricing::fare(&Rates::default_for(tier), distance_m, duration_s, demand.surge_bp()) })
        .collect();
    Ok(Json(EstimateResponse { distance_m, duration_s, distance_source: "straight_line_x1.3", currency: "NAD", estimates }))
}
