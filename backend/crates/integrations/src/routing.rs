//! Road routing (TRD §2.1, §4.2): trip distance and duration for quotes, and driver ETAs to a
//! pickup for matching.
//!
//! - `Osrm`: the self-hosted OSRM server over the Namibia OpenStreetMap extract
//!   (`infra/osrm`). `route` prices a trip; `table` gives every candidate's ETA in one call.
//! - `Estimate`: straight-line distance × 1.3 at the demo's free-flow speed. Local development
//!   and tests only (refused by config elsewhere), so the stack runs without the OSRM data.
//!
//! Durations are free-flow; callers scale them by the demand level's traffic factor.

use std::time::Duration;

use fuber_common::validate::LatLng;
use serde::Deserialize;

const TIMEOUT: Duration = Duration::from_secs(3);
/// Straight line to road distance, for the estimate.
pub const ROAD_FACTOR: f64 = 1.3;
/// Free-flow speed for the estimate, as in the demo (`CFG.SPEED_KMH`).
pub const ESTIMATE_KMH: f64 = 34.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leg {
    pub distance_m: u32,
    /// Free-flow duration.
    pub duration_s: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum RoutingError {
    #[error("routing unavailable: {0}")]
    Unavailable(String),
    #[error("no road route between these points")]
    NoRoute,
}

pub enum Routing {
    Osrm { base_url: String, http: reqwest::Client },
    Estimate,
}

impl Routing {
    pub fn osrm(base_url: &str) -> Routing {
        let http = crate::http_client(TIMEOUT);
        Routing::Osrm { base_url: base_url.trim_end_matches('/').to_owned(), http }
    }

    /// `"osrm"` or `"estimate"`, reported with quotes so clients can show "approximate".
    pub fn source(&self) -> &'static str {
        match self {
            Routing::Osrm { .. } => "osrm",
            Routing::Estimate => "estimate",
        }
    }

    /// The fastest road route from `a` to `b`.
    pub async fn route(&self, a: LatLng, b: LatLng) -> Result<Leg, RoutingError> {
        match self {
            Routing::Estimate => Ok(estimate(a, b)),
            Routing::Osrm { base_url, http } => {
                let url = format!("{base_url}/route/v1/driving/{};{}?overview=false&alternatives=false&steps=false", coord(a), coord(b));
                let r: RouteResponse = get(http, &url).await?;
                let route = r.routes.into_iter().next().ok_or(RoutingError::NoRoute)?;
                Ok(Leg { distance_m: route.distance.round() as u32, duration_s: route.duration.round() as u32 })
            }
        }
    }

    /// The fastest road route from `a` to `b` as a line (RideCheck's planned route). The
    /// estimate is the straight line.
    pub async fn route_line(&self, a: LatLng, b: LatLng) -> Result<Vec<LatLng>, RoutingError> {
        match self {
            Routing::Estimate => Ok(vec![a, b]),
            Routing::Osrm { base_url, http } => {
                let url = format!(
                    "{base_url}/route/v1/driving/{};{}?overview=full&geometries=geojson&alternatives=false&steps=false",
                    coord(a),
                    coord(b)
                );
                let r: LineResponse = get(http, &url).await?;
                let route = r.routes.into_iter().next().ok_or(RoutingError::NoRoute)?;
                let line: Vec<LatLng> = route.geometry.coordinates.into_iter().map(|[lon, lat]| LatLng { lat, lon }).collect();
                if line.len() < 2 {
                    return Err(RoutingError::NoRoute);
                }
                Ok(line)
            }
        }
    }

    /// Road legs from each source to one destination (driver positions → pickup), in order.
    /// `None` where OSRM finds no route (e.g. a driver on a road cut off from the pickup).
    pub async fn to_one(&self, sources: &[LatLng], dest: LatLng) -> Result<Vec<Option<Leg>>, RoutingError> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        match self {
            Routing::Estimate => Ok(sources.iter().map(|s| Some(estimate(*s, dest))).collect()),
            Routing::Osrm { base_url, http } => {
                let coords: Vec<String> = sources.iter().chain(std::iter::once(&dest)).map(|p| coord(*p)).collect();
                let srcs: Vec<String> = (0..sources.len()).map(|i| i.to_string()).collect();
                let url = format!(
                    "{base_url}/table/v1/driving/{}?sources={}&destinations={}&annotations=duration,distance",
                    coords.join(";"),
                    srcs.join(";"),
                    sources.len()
                );
                let t: TableResponse = get(http, &url).await?;
                let (durations, distances) = (t.durations.unwrap_or_default(), t.distances.unwrap_or_default());
                Ok((0..sources.len())
                    .map(|i| {
                        let d = durations.get(i).and_then(|row| row.first().copied().flatten())?;
                        let m = distances.get(i).and_then(|row| row.first().copied().flatten())?;
                        Some(Leg { distance_m: m.round() as u32, duration_s: d.round() as u32 })
                    })
                    .collect())
            }
        }
    }
}

/// Straight line × 1.3 at free-flow speed.
pub fn estimate(a: LatLng, b: LatLng) -> Leg {
    let distance_m = (a.distance_m(&b) * ROAD_FACTOR).round();
    let duration_s = (distance_m / (ESTIMATE_KMH / 3.6)).round();
    Leg { distance_m: distance_m as u32, duration_s: duration_s as u32 }
}

/// OSRM takes `lon,lat`.
fn coord(p: LatLng) -> String {
    format!("{:.6},{:.6}", p.lon, p.lat)
}

async fn get<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T, RoutingError> {
    let res = http.get(url).send().await.map_err(|e| RoutingError::Unavailable(e.without_url().to_string()))?;
    let status = res.status();
    let body: OsrmBody<T> = res.json().await.map_err(|e| RoutingError::Unavailable(e.without_url().to_string()))?;
    match body.code.as_str() {
        "Ok" => body.data.ok_or_else(|| RoutingError::Unavailable("empty response".into())),
        "NoRoute" | "NoSegment" => Err(RoutingError::NoRoute),
        code => Err(RoutingError::Unavailable(format!("{status} {code}"))),
    }
}

#[derive(Deserialize)]
struct OsrmBody<T> {
    code: String,
    #[serde(flatten)]
    data: Option<T>,
}

#[derive(Deserialize)]
struct RouteResponse {
    routes: Vec<Route>,
}

#[derive(Deserialize)]
struct Route {
    distance: f64,
    duration: f64,
}

#[derive(Deserialize)]
struct LineResponse {
    routes: Vec<LineRoute>,
}

#[derive(Deserialize)]
struct LineRoute {
    geometry: Geometry,
}

/// GeoJSON LineString: `[lon, lat]` pairs.
#[derive(Deserialize)]
struct Geometry {
    coordinates: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct TableResponse {
    durations: Option<Vec<Vec<Option<f64>>>>,
    distances: Option<Vec<Vec<Option<f64>>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_matches_phase_0_fare_estimate() {
        let post_st = LatLng::new(-22.56665, 17.08126).unwrap();
        let maerua = LatLng::new(-22.58359, 17.09373).unwrap();
        let leg = estimate(post_st, maerua);
        let straight = post_st.distance_m(&maerua);
        assert_eq!(leg.distance_m, (straight * 1.3).round() as u32);
        // 34 km/h ≈ 9.44 m/s
        assert!((leg.duration_s as f64 - leg.distance_m as f64 / 9.444).abs() < 2.0);
    }

    #[tokio::test]
    async fn estimate_table_keeps_order() {
        let pickup = LatLng::new(-22.56665, 17.08126).unwrap();
        let near = LatLng::new(-22.5670, 17.0820).unwrap();
        let far = LatLng::new(-22.6000, 17.0900).unwrap();
        let legs = Routing::Estimate.to_one(&[far, near], pickup).await.unwrap();
        assert!(legs[0].unwrap().distance_m > legs[1].unwrap().distance_m);
    }
}
