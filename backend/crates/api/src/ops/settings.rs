//! Matching, booking and RideCheck parameters (PRD D-6, O-5), editable by admins without a
//! release. Only the keys below can be set, each within bounds that keep the service safe;
//! readers fall back to the launch defaults if a value is ever missing
//! (`fuber_db::settings`). Every change is audited with the old and new value.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use utoipa::ToSchema;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

/// Key, what it means, minimum, maximum.
const KEYS: &[(&str, &str, f64, f64)] = &[
    ("dispatch.radius_m", "Search radius for drivers (m)", 500.0, 15_000.0),
    ("dispatch.offer_ms", "Time a driver has to answer a request (ms)", 8_000.0, 60_000.0),
    ("dispatch.arrive_m", "Distance that counts as arrived at the pickup (m)", 20.0, 200.0),
    ("dispatch.location_stale_s", "Ignore drivers whose position is older than (s)", 15.0, 600.0),
    ("dispatch.max_candidates", "Nearest drivers considered per request", 1.0, 100.0),
    ("dispatch.fuzz_min_m", "Fuzzy pickup offset, minimum (m)", 50.0, 500.0),
    ("dispatch.fuzz_max_m", "Fuzzy pickup offset, maximum (m)", 100.0, 1_000.0),
    ("booking.quote_ttl_s", "A quote's locked price holds for (s)", 60.0, 1_800.0),
    ("booking.schedule_min_s", "Earliest scheduled pickup, from now (s)", 900.0, 86_400.0),
    ("booking.schedule_max_s", "Latest scheduled pickup, from now (s)", 86_400.0, 2_592_000.0),
    ("booking.schedule_lead_s", "Matching starts this long before a scheduled pickup (s)", 300.0, 3_600.0),
    ("safety.deviation_m", "RideCheck: distance off the planned route (m)", 100.0, 1_000.0),
    ("safety.unexpected_stop_s", "RideCheck: stopped for (s)", 60.0, 900.0),
    ("pricing.kids_surcharge_cents", "Kids Guardian: child seat surcharge (cents, all to the driver)", 0.0, 20_000.0),
];

/// Minimum key, maximum key, and their defaults: a minimum may not pass its maximum.
const PAIRS: &[(&str, &str, f64, f64)] = &[
    ("dispatch.fuzz_min_m", "dispatch.fuzz_max_m", 100.0, 200.0),
    ("booking.schedule_min_s", "booking.schedule_max_s", 1_800.0, 604_800.0),
];

#[derive(Debug, Serialize, ToSchema)]
pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    /// The stored value; absent when the default is used.
    pub value: Option<f64>,
    pub min: f64,
    pub max: f64,
    /// When staff last changed it; absent for a value never changed since it was installed.
    #[serde(with = "time::serde::rfc3339::option")]
    pub updated_at: Option<OffsetDateTime>,
}

#[utoipa::path(
    get, path = "/v1/ops/settings", operation_id = "settings_list", tag = "ops", security(("staff" = [])),
    responses(
        (status = 200, description = "Every editable parameter with its bounds", body = Vec<Setting>),
        (status = 403, description = "Admins only", body = ErrorBody),
    )
)]
pub async fn list(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<Setting>>, ApiError> {
    staff.require_admin()?;
    // Rows the migrations seeded carry the install time and no staff member: not a change.
    let rows: Vec<(String, serde_json::Value, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT key, value, CASE WHEN updated_by IS NOT NULL THEN updated_at END FROM settings").fetch_all(&s.db).await?;
    Ok(Json(
        KEYS.iter()
            .map(|&(key, label, min, max)| {
                let row = rows.iter().find(|r| r.0 == key);
                Setting { key, label, value: row.and_then(|r| r.1.as_f64()), min, max, updated_at: row.and_then(|r| r.2) }
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingValue {
    pub value: f64,
}

#[utoipa::path(
    put, path = "/v1/ops/settings/{key}", operation_id = "setting_update", tag = "ops", security(("staff" = [])),
    params(("key" = String, Path, description = "e.g. `dispatch.radius_m`")),
    request_body = SettingValue,
    responses(
        (status = 200, description = "Saved; matching and RideCheck use it from their next run", body = Setting),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 404, description = "Not an editable setting", body = ErrorBody),
        (status = 422, description = "Outside the allowed range", body = ErrorBody),
    )
)]
pub async fn set(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(key): Path<String>,
    ValidJson(body): ValidJson<SettingValue>,
) -> Result<Json<Setting>, ApiError> {
    staff.require_admin()?;
    let &(key, label, min, max) = KEYS.iter().find(|k| k.0 == key).ok_or_else(ApiError::not_found)?;
    if !body.value.is_finite() || body.value < min || body.value > max {
        return Err(ApiError::invalid(format!("{label} must be between {min} and {max}")));
    }
    let mut tx = s.db.begin().await?;
    for &(lo, hi, lo_default, hi_default) in PAIRS {
        let (other, default) = match key {
            k if k == lo => (hi, hi_default),
            k if k == hi => (lo, lo_default),
            _ => continue,
        };
        let stored: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = $1 FOR UPDATE").bind(other).fetch_optional(&mut *tx).await?;
        let other_value = stored.and_then(|v| v.as_f64()).unwrap_or(default);
        let (min_v, max_v) = if key == lo { (body.value, other_value) } else { (other_value, body.value) };
        if min_v > max_v {
            return Err(ApiError::invalid(format!("{lo} can't be more than {hi} ({min_v} > {max_v})")));
        }
    }
    let old: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = $1 FOR UPDATE").bind(key).fetch_optional(&mut *tx).await?;
    let updated_at: OffsetDateTime = sqlx::query_scalar(
        "INSERT INTO settings (key, value, updated_by, updated_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = now()
         RETURNING updated_at",
    )
    .bind(key)
    .bind(json!(body.value))
    .bind(staff.id)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: None,
            action: "setting.update",
            target_type: "setting",
            target_id: key.into(),
            detail: Some(json!({ "from": old, "to": body.value })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(Setting { key, label, value: Some(body.value), min, max, updated_at: Some(updated_at) }))
}
