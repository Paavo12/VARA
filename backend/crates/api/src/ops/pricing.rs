//! Pricing and surge (PRD O-5), admins only.
//!
//! - **Rates are versioned**: a change adds a `pricing_tiers` row from a start time (now or
//!   later) and never edits one, so quotes already locked keep the rates they were signed with.
//! - **Service fee** stays within 15–18 % (stakeholder decision 2026-11-04, docs/06 §7).
//! - **Surge zones**: the multiplier (1.00–3.00) and demand level of an existing zone. Drawing
//!   new zones needs a map (later).
//!
//! Every change is audited.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use fuber_domain::Cents;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

/// The service fee may be set between these, in basis points (15–18 %).
const FEE_BP: std::ops::RangeInclusive<i32> = 1_500..=1_800;
/// Any single rate, in cents (N$0 – N$1 000).
const MAX_RATE_CENTS: i64 = 100_000;

#[derive(Debug, Serialize, ToSchema)]
pub struct TierRates {
    pub id: Uuid,
    /// `classic`, `comfort` or `softlife`.
    pub tier: String,
    pub base: Cents,
    pub per_km: Cents,
    pub per_min: Cents,
    pub minimum: Cents,
    /// Basis points: 1650 = 16.5 %.
    pub service_fee_bp: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub effective_from: OffsetDateTime,
    /// In force now (the latest started row of its tier).
    pub current: bool,
}

type RateRow = (Uuid, String, i64, i64, i64, i64, i32, OffsetDateTime, bool);

#[utoipa::path(
    get, path = "/v1/ops/pricing", operation_id = "pricing_list", tag = "ops", security(("staff" = [])),
    responses(
        (status = 200, description = "The rates in force for each ride type, and any that start later", body = Vec<TierRates>),
        (status = 403, description = "Admins only", body = ErrorBody),
    )
)]
pub async fn list(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<TierRates>>, ApiError> {
    staff.require_admin()?;
    let rows: Vec<RateRow> = sqlx::query_as(
        "WITH cur AS (SELECT DISTINCT ON (tier) id FROM pricing_tiers WHERE effective_from <= now() ORDER BY tier, effective_from DESC)
         SELECT p.id, p.tier::text, p.base_cents, p.per_km_cents, p.per_min_cents, p.minimum_cents, p.service_fee_bp, p.effective_from,
                p.id IN (SELECT id FROM cur)
         FROM pricing_tiers p
         WHERE p.id IN (SELECT id FROM cur) OR p.effective_from > now()
         ORDER BY p.tier, p.effective_from",
    )
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, tier, base, km, min, minimum, fee, effective_from, current)| TierRates {
                id,
                tier,
                base: Cents(base),
                per_km: Cents(km),
                per_min: Cents(min),
                minimum: Cents(minimum),
                service_fee_bp: fee,
                effective_from,
                current,
            })
            .collect(),
    ))
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RideType {
    Classic,
    Comfort,
    Softlife,
}

impl RideType {
    fn as_str(self) -> &'static str {
        match self {
            RideType::Classic => "classic",
            RideType::Comfort => "comfort",
            RideType::Softlife => "softlife",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewRates {
    pub tier: RideType,
    pub base_cents: i64,
    pub per_km_cents: i64,
    pub per_min_cents: i64,
    pub minimum_cents: i64,
    /// 1500–1800 (15–18 %).
    pub service_fee_bp: i32,
    /// When the rates start; now when absent. Never in the past.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub effective_from: Option<OffsetDateTime>,
}

#[utoipa::path(
    post, path = "/v1/ops/pricing", operation_id = "pricing_add", tag = "ops", security(("staff" = [])),
    request_body = NewRates,
    responses(
        (status = 201, description = "New rates added; quotes use them from `effective_from`", body = TierRates),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 409, description = "Rates already start at that moment for this ride type (`duplicate_start`)", body = ErrorBody),
        (status = 422, description = "Outside the allowed ranges", body = ErrorBody),
    )
)]
pub async fn add(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    ValidJson(r): ValidJson<NewRates>,
) -> Result<(StatusCode, Json<TierRates>), ApiError> {
    staff.require_admin()?;
    for (name, v) in [("base", r.base_cents), ("per km", r.per_km_cents), ("per minute", r.per_min_cents), ("minimum", r.minimum_cents)] {
        if !(0..=MAX_RATE_CENTS).contains(&v) {
            return Err(ApiError::invalid(format!("The {name} rate must be between 0 and {MAX_RATE_CENTS} cents")));
        }
    }
    if r.minimum_cents == 0 || r.per_km_cents == 0 {
        return Err(ApiError::invalid("The minimum fare and the per km rate must be more than 0"));
    }
    if !FEE_BP.contains(&r.service_fee_bp) {
        return Err(ApiError::invalid("The service fee must be between 1500 and 1800 basis points (15–18 %)"));
    }
    let now = OffsetDateTime::now_utc();
    let from = r.effective_from.unwrap_or(now);
    if from < now - time::Duration::minutes(1) {
        return Err(ApiError::invalid("Rates can't start in the past"));
    }
    let mut tx = s.db.begin().await?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO pricing_tiers (tier, base_cents, per_km_cents, per_min_cents, minimum_cents, service_fee_bp, effective_from, created_by)
         VALUES ($1::ride_tier, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (tier, effective_from) DO NOTHING RETURNING id",
    )
    .bind(r.tier.as_str())
    .bind(r.base_cents)
    .bind(r.per_km_cents)
    .bind(r.per_min_cents)
    .bind(r.minimum_cents)
    .bind(r.service_fee_bp)
    .bind(from)
    .bind(staff.id)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or_else(|| ApiError::conflict("duplicate_start", "Rates for this ride type already start at that moment."))?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: None,
            action: "pricing.add",
            target_type: "pricing_tier",
            target_id: id.to_string(),
            detail: Some(json!({
                "tier": r.tier.as_str(), "base_cents": r.base_cents, "per_km_cents": r.per_km_cents, "per_min_cents": r.per_min_cents,
                "minimum_cents": r.minimum_cents, "service_fee_bp": r.service_fee_bp, "effective_from": from.unix_timestamp(),
            })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(TierRates {
            id,
            tier: r.tier.as_str().into(),
            base: Cents(r.base_cents),
            per_km: Cents(r.per_km_cents),
            per_min: Cents(r.per_min_cents),
            minimum: Cents(r.minimum_cents),
            service_fee_bp: r.service_fee_bp,
            effective_from: from,
            current: from <= now,
        }),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SurgeZone {
    pub id: Uuid,
    pub name: String,
    /// 1.00–3.00.
    pub multiplier: f64,
    /// `normal`, `high` or `peak` (sets the traffic factor on trip times).
    pub demand_level: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[utoipa::path(
    get, path = "/v1/ops/surge-zones", operation_id = "surge_zones", tag = "ops", security(("staff" = [])),
    responses(
        (status = 200, description = "Surge zones by name", body = Vec<SurgeZone>),
        (status = 403, description = "Admins only", body = ErrorBody),
    )
)]
pub async fn zones(State(s): State<AppState>, staff: Staff) -> Result<Json<Vec<SurgeZone>>, ApiError> {
    staff.require_admin()?;
    let rows: Vec<(Uuid, String, f64, String, OffsetDateTime)> =
        sqlx::query_as("SELECT id, name, multiplier::float8, demand_level, updated_at FROM surge_zones ORDER BY name")
            .fetch_all(&s.db)
            .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, multiplier, demand_level, updated_at)| SurgeZone { id, name, multiplier, demand_level, updated_at })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SurgeUpdate {
    /// 1.00–3.00, two decimals.
    pub multiplier: f64,
    /// `normal`, `high` or `peak`.
    pub demand_level: String,
}

#[utoipa::path(
    put, path = "/v1/ops/surge-zones/{id}", operation_id = "surge_zone_update", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = SurgeUpdate,
    responses(
        (status = 200, description = "Saved; new quotes in the zone use it at once", body = SurgeZone),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 404, description = "No such zone", body = ErrorBody),
        (status = 422, description = "Outside the allowed range", body = ErrorBody),
    )
)]
pub async fn update_zone(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(u): ValidJson<SurgeUpdate>,
) -> Result<Json<SurgeZone>, ApiError> {
    staff.require_admin()?;
    if !u.multiplier.is_finite() || !(1.0..=3.0).contains(&u.multiplier) {
        return Err(ApiError::invalid("The multiplier must be between 1.00 and 3.00"));
    }
    if !["normal", "high", "peak"].contains(&u.demand_level.as_str()) {
        return Err(ApiError::invalid("The demand level must be normal, high or peak"));
    }
    let multiplier = (u.multiplier * 100.0).round() / 100.0;
    let mut tx = s.db.begin().await?;
    let old: Option<(f64, String)> = sqlx::query_as("SELECT multiplier::float8, demand_level FROM surge_zones WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let (old_multiplier, old_level) = old.ok_or_else(ApiError::not_found)?;
    let (name, updated_at): (String, OffsetDateTime) = sqlx::query_as(
        "UPDATE surge_zones SET multiplier = $2, demand_level = $3, updated_by = $4, updated_at = now() WHERE id = $1 RETURNING name, updated_at",
    )
    .bind(id)
    .bind(multiplier)
    .bind(&u.demand_level)
    .bind(staff.id)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: None,
            action: "surge.update",
            target_type: "surge_zone",
            target_id: id.to_string(),
            detail: Some(json!({ "from": [old_multiplier, old_level], "to": [multiplier, u.demand_level] })),
            ip: Some(ip),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(SurgeZone { id, name, multiplier, demand_level: u.demand_level, updated_at }))
}
