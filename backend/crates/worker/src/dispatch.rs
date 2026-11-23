//! Matching (PRD D-1 – D-5, TRD §4.2). Runs every second in the worker:
//!
//! 1. Offers past their 15 s window expire (they count against the driver's acceptance rate).
//! 2. Scheduled rides whose dispatch time has come start searching (PRD B-6).
//! 3. Each searching ride without a pending offer is offered to the best eligible driver,
//!    re-ranked with fresh positions every time. With nobody left, the ride becomes
//!    `NO_DRIVERS` and the rider sees *No drivers accepted → Try again*.
//!
//! Several workers can run at once: rides are claimed with `FOR UPDATE SKIP LOCKED`, and the
//! unique indexes on `ride_offers` stop a ride or a driver ever holding two pending offers.
//! Accepting is the API's job (it races drivers against the ride's state, not the worker).

use fuber_common::validate::LatLng;
use fuber_db::{
    presence::{self, Search},
    rides::{self, RideError},
    settings::Dispatch,
    sqlx,
};
use fuber_domain::{Cents, Demand, Party, RideEvent, matching, pricing};
use uuid::Uuid;

use crate::{Ctx, JobError};

/// Most rides handled per tick, so one tick never runs long.
const BATCH: i64 = 50;

impl From<RideError> for JobError {
    fn from(e: RideError) -> Self {
        match e {
            RideError::Db(e) => JobError::Db(e),
            other => JobError::Other(other.to_string()),
        }
    }
}

/// What happened in one tick, for logs and tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub expired: usize,
    pub started: usize,
    pub offered: usize,
    pub no_drivers: usize,
}

pub async fn tick(ctx: &Ctx) -> Result<Tick, JobError> {
    let cfg = Dispatch::load(&ctx.db).await?;
    let mut t = Tick { expired: expire_offers(ctx).await?, started: start_scheduled(ctx, &cfg).await?, ..Tick::default() };
    let mut conn = ctx.db.acquire().await?;
    presence::sweep_stale(&mut conn, cfg.location_stale_s).await?;
    drop(conn);

    let waiting: Vec<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM rides r WHERE r.state = 'SEARCHING'
           AND NOT EXISTS (SELECT 1 FROM ride_offers o WHERE o.ride_id = r.id AND o.response = 'pending')
         ORDER BY r.search_started_at LIMIT $1",
    )
    .bind(BATCH)
    .fetch_all(&ctx.db)
    .await?;
    for ride_id in waiting {
        match offer_next(ctx, &cfg, ride_id).await {
            Ok(Some(true)) => t.offered += 1,
            Ok(Some(false)) => t.no_drivers += 1,
            Ok(None) => {}
            // One ride's failure (e.g. routing down) must not stop the others.
            Err(e) => tracing::warn!(%ride_id, error = %e, "matching failed; will retry"),
        }
    }
    Ok(t)
}

async fn expire_offers(ctx: &Ctx) -> Result<usize, JobError> {
    let drivers: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE ride_offers SET response = 'expired', responded_at = now()
         WHERE response = 'pending' AND expires_at <= now() RETURNING driver_id",
    )
    .fetch_all(&ctx.db)
    .await?;
    for d in &drivers {
        rides::record_answer(&ctx.db, *d, false).await?;
    }
    Ok(drivers.len())
}

async fn start_scheduled(ctx: &Ctx, cfg: &Dispatch) -> Result<usize, JobError> {
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM rides WHERE state = 'SCHEDULED' AND scheduled_for <= now() + make_interval(secs => $1) LIMIT $2",
    )
    .bind(cfg.schedule_lead_s as f64)
    .bind(BATCH)
    .fetch_all(&ctx.db)
    .await?;
    let mut n = 0;
    for id in due {
        let mut tx = ctx.db.begin().await?;
        match rides::transition(&mut tx, id, RideEvent::StartSearch, Party::System, None, None).await {
            Ok(_) => {
                sqlx::query("UPDATE rides SET search_started_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
                tx.commit().await?;
                n += 1;
            }
            // Cancelled meanwhile.
            Err(RideError::Invalid(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(n)
}

type RideRow = (f64, f64, String, bool, Uuid, i16, String, i64, i32, i32);

/// Offers the ride to the best driver not yet asked this round. `Some(true)` = offered,
/// `Some(false)` = nobody left (now `NO_DRIVERS`), `None` = another worker has it or it is no
/// longer searching.
pub async fn offer_next(ctx: &Ctx, cfg: &Dispatch, ride_id: Uuid) -> Result<Option<bool>, JobError> {
    let mut tx = ctx.db.begin().await?;
    let row: Option<RideRow> = sqlx::query_as(
        "SELECT ST_Y(r.pickup::geometry), ST_X(r.pickup::geometry), r.tier::text, r.wheelchair, r.rider_id, r.search_round,
                r.demand_level, r.fare_cents, p.service_fee_bp, rp.priority_points
         FROM rides r JOIN rider_profiles rp ON rp.user_id = r.rider_id JOIN pricing_tiers p ON p.id = r.pricing_tier_id
         WHERE r.id = $1 AND r.state = 'SEARCHING'
           AND NOT EXISTS (SELECT 1 FROM ride_offers o WHERE o.ride_id = r.id AND o.response = 'pending')
         FOR UPDATE OF r SKIP LOCKED",
    )
    .bind(ride_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((lat, lon, tier, wheelchair, rider_id, round, demand, fare, fee_bp, rider_points)) = row else { return Ok(None) };
    let demand = Demand::parse(&demand).unwrap_or(Demand::Normal);
    // Drivers asked this round, plus any who accepted this ride in an earlier round and then
    // cancelled: she has seen the exact pickup, and a new driver's PIN attempts start again.
    let offers: Vec<(Uuid, bool)> =
        sqlx::query_as("SELECT driver_id, round = $2 FROM ride_offers WHERE ride_id = $1 AND (round = $2 OR response = 'accepted')")
            .bind(ride_id)
            .bind(round)
            .fetch_all(&mut *tx)
            .await?;
    let asked = offers.iter().filter(|(_, this_round)| *this_round).count();
    let exclude: Vec<Uuid> = offers.iter().map(|(d, _)| *d).collect();
    let found = presence::candidates(
        &mut *tx,
        &Search {
            lat,
            lon,
            tier: &tier,
            wheelchair,
            radius_m: cfg.radius_m,
            stale_s: cfg.location_stale_s,
            exclude: &exclude,
            rider_id: Some(rider_id),
            limit: cfg.max_candidates,
        },
    )
    .await?;

    // Road ETAs for every candidate in one routing call (TRD §4.2 step 3).
    let pickup = LatLng { lat, lon };
    let positions: Vec<LatLng> = found.iter().map(|c| LatLng { lat: c.lat, lon: c.lon }).collect();
    let legs = ctx.routing.to_one(&positions, pickup).await.map_err(|e| JobError::Other(e.to_string()))?;
    let traffic = demand.traffic();
    let mut ranked_in = Vec::new();
    for (c, leg) in found.iter().zip(&legs) {
        let Some(leg) = leg else { continue };
        let eta_s = (leg.duration_s as f64 * traffic).round();
        ranked_in.push(matching::Candidate {
            driver_id: (c.driver_id, eta_s as i32, leg.distance_m as i32),
            straight_m: c.straight_m,
            eta_min: eta_s / 60.0,
            acceptance: c.acceptance,
            favourite: c.favourite,
            priority_points: c.priority_points,
        });
    }
    let ranked = matching::rank(ranked_in, demand, rider_points);
    let Some(&((driver_id, eta_s, distance_m), score)) = ranked.offer_order.first() else {
        rides::transition(&mut tx, ride_id, RideEvent::NoDriverLeft, Party::System, None, Some(serde_json::json!({ "asked": asked })))
            .await?;
        tx.commit().await?;
        tracing::info!(%ride_id, asked, "no drivers left");
        return Ok(Some(false));
    };
    // What she would earn: a Map Expert pays the flat fee, and everyone pays the launch
    // promotion's fee while it runs (accept books the same split).
    let (map_expert, launch_promo): (bool, bool) = sqlx::query_as(
        "SELECT coalesce((SELECT map_expert_until > now() FROM driver_profiles WHERE user_id = $1), false), launch_promo_active()",
    )
    .bind(driver_id)
    .fetch_one(&mut *tx)
    .await?;
    let (_, earning) = pricing::split(Cents(fare), fee_bp as u32, pricing::FeeTerms { map_expert, launch_promo });
    let inserted = sqlx::query(
        "INSERT INTO ride_offers (ride_id, driver_id, rank, score, eta_s, distance_m, expires_at, round, earning_cents)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7), $8, $9)",
    )
    .bind(ride_id)
    .bind(driver_id)
    .bind(asked as i16 + 1)
    .bind(score.clamp(-99.0, 99.0))
    .bind(eta_s)
    .bind(distance_m)
    .bind(cfg.offer_ms as f64 / 1000.0)
    .bind(round)
    .bind(earning.0)
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(_) => {}
        // Another worker just gave this driver a request: try again next tick.
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    tx.commit().await?;
    tracing::info!(%ride_id, %driver_id, rank = asked + 1, eta_s, held = ranked.held.len(), "offer sent");
    Ok(Some(true))
}
