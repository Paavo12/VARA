//! Places (PRD B-1, B-2): the rider's "Where to?" search and naming the pin on the map.
//!
//! Search matches the demo: every word must appear, ignoring case, accents and punctuation,
//! and a leading house number is dropped ("5 sam nujoma" finds Sam Nujoma Drive). Landmarks
//! come first, then suburbs, then streets; within each, names starting with the query first.

use axum::{
    Json,
    extract::{Query, State},
};
use fuber_common::validate::LatLng;
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::RiderUser,
    error::{ApiError, ErrorBody},
};

const MAX_RESULTS: i64 = 20;
const MAX_QUERY_CHARS: usize = 80;
/// The pin is named after a street this close, else a landmark this close, else its suburb.
const STREET_NEAR_M: f64 = 150.0;
const LANDMARK_NEAR_M: f64 = 300.0;

#[derive(Debug, Serialize, ToSchema)]
pub struct Place {
    pub id: Uuid,
    /// `landmark`, `suburb` or `street`.
    pub kind: String,
    pub name: String,
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    /// Up to 80 characters. Empty returns the landmarks (quick-place chips).
    #[serde(default)]
    pub q: String,
}

/// Lower case, accents removed, anything that is not a letter or digit collapsed to a space:
/// the same folding as `fold_text` in the database and `fold` in the demo.
pub fn fold(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_owned()
}

/// The search words: folded, without a leading house number such as `5` or `12a`.
fn words(q: &str) -> Vec<String> {
    let folded = fold(q);
    let mut w: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
    // The demo's /^\d+[a-z]?\s+/: digits, at most one letter, then more words.
    let house_number = |s: &str| {
        let letters = s.trim_start_matches(|c: char| c.is_ascii_digit());
        letters.len() < s.len() && letters.len() <= 1
    };
    if w.len() > 1 && house_number(w[0]) {
        w.remove(0);
    }
    w.into_iter().take(8).map(str::to_owned).collect()
}

type PlaceRow = (Uuid, String, String, String, f64, f64);

#[utoipa::path(
    get, path = "/v1/places", tag = "booking", security(("bearer" = [])),
    params(SearchQuery),
    responses(
        (status = 200, description = "Up to 20 places, best first", body = Vec<Place>),
        (status = 422, description = "Query too long", body = ErrorBody),
    )
)]
pub async fn search(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<Place>>, ApiError> {
    s.limiter.check("places", &user.user_id.to_string(), s.limits.places).await?;
    if q.q.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::invalid("search is limited to 80 characters"));
    }
    let words = words(&q.q);
    let rows: Vec<PlaceRow> = if words.is_empty() {
        sqlx::query_as(
            "SELECT id, kind::text, name, detail, ST_Y(location::geometry), ST_X(location::geometry)
             FROM places WHERE active AND kind = 'landmark' ORDER BY name LIMIT $1",
        )
        .bind(MAX_RESULTS)
        .fetch_all(&s.db)
        .await?
    } else {
        // Each word is a substring match; LIKE metacharacters can't occur after folding.
        let patterns: Vec<String> = words.iter().map(|w| format!("%{w}%")).collect();
        let prefix = format!("{}%", words.join(" "));
        sqlx::query_as(
            "SELECT id, kind::text, name, detail, ST_Y(location::geometry), ST_X(location::geometry)
             FROM places WHERE active AND search LIKE ALL($1)
             ORDER BY rank, (search LIKE $2) DESC, name LIMIT $3",
        )
        .bind(&patterns)
        .bind(&prefix)
        .bind(MAX_RESULTS)
        .fetch_all(&s.db)
        .await?
    };
    Ok(Json(rows.into_iter().map(|(id, kind, name, detail, lat, lon)| Place { id, kind, name, detail, lat, lon }).collect()))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReverseQuery {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PinName {
    /// What to show for the pin, e.g. "Sam Nujoma Drive" or "Near Maerua Mall".
    pub address: String,
    /// The suburb, e.g. "Klein Windhoek".
    pub area: String,
    pub lat: f64,
    pub lon: f64,
}

#[utoipa::path(
    get, path = "/v1/places/reverse", tag = "booking", security(("bearer" = [])),
    params(ReverseQuery),
    responses(
        (status = 200, description = "A name for a point on the map (the pickup pin)", body = PinName),
        (status = 422, description = "Outside the service area", body = ErrorBody),
    )
)]
pub async fn reverse(
    State(s): State<AppState>,
    RiderUser(user): RiderUser,
    Query(q): Query<ReverseQuery>,
) -> Result<Json<PinName>, ApiError> {
    s.limiter.check("places", &user.user_id.to_string(), s.limits.places).await?;
    let p = LatLng::new(q.lat, q.lon).map_err(|e| ApiError::invalid(e.0))?;
    let area = suburb(&s.db, p).await?;
    let street: Option<String> = nearest(&s.db, p, "street", STREET_NEAR_M).await?;
    let address = match street {
        Some(name) => name,
        None => match nearest(&s.db, p, "landmark", LANDMARK_NEAR_M).await? {
            Some(name) => format!("Near {name}"),
            None => format!("Pinned location · {area}"),
        },
    };
    Ok(Json(PinName { address, area, lat: p.lat, lon: p.lon }))
}

async fn nearest<'e>(db: impl PgExecutor<'e>, p: LatLng, kind: &str, within_m: f64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT name FROM places
         WHERE active AND kind = $3::place_kind AND ST_DWithin(location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography, $4)
         ORDER BY location <-> ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography LIMIT 1",
    )
    .bind(p.lat)
    .bind(p.lon)
    .bind(kind)
    .bind(within_m)
    .fetch_optional(db)
    .await
}

/// The suburb whose centre is nearest (as in the demo).
pub async fn suburb<'e>(db: impl PgExecutor<'e>, p: LatLng) -> Result<String, sqlx::Error> {
    let name: Option<String> = sqlx::query_scalar(
        "SELECT name FROM places WHERE active AND kind = 'suburb'
         ORDER BY location <-> ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography LIMIT 1",
    )
    .bind(p.lat)
    .bind(p.lon)
    .fetch_optional(db)
    .await?;
    Ok(name.unwrap_or_else(|| "Windhoek".into()))
}

/// The pickup area a driver sees before arrival (PRD D-4): "Near <landmark>" within 1 km of the
/// fuzzed point, else "<suburb> area", as in the demo's `approxArea`.
pub async fn approx_area(db: &sqlx::PgPool, fuzzed: LatLng) -> Result<String, sqlx::Error> {
    match nearest(db, fuzzed, "landmark", 1000.0).await? {
        Some(name) => Ok(format!("Near {name}")),
        None => Ok(format!("{} area", suburb(db, fuzzed).await?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_matches_the_demo() {
        assert_eq!(fold("Dr. Kwame Nkrumah Avenue"), "dr kwame nkrumah avenue");
        assert_eq!(fold("  Café  Zoo!! "), "cafe zoo");
        assert_eq!(fold("Joe's Beerhouse"), "joe s beerhouse");
    }

    #[test]
    fn leading_house_number_is_ignored() {
        assert_eq!(words("5 sam nujoma"), ["sam", "nujoma"]);
        assert_eq!(words("12a Klein Windhoek"), ["klein", "windhoek"]);
        // A number on its own, or a road name that is a number, is kept.
        assert_eq!(words("2020"), ["2020"]);
        assert_eq!(words("Nelson MANDELA"), ["nelson", "mandela"]);
    }
}
