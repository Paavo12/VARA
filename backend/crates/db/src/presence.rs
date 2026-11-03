//! Drivers online (PRD D-1) and the candidate search used by matching (TRD §4.2 steps 1–2).
//! The only code that touches `driver_presence`.

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{rides::DRIVER_ACTIVE, settings::LockoutRules};

/// Why a driver can't go online. Messages are shown in the driver app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotEligible {
    /// Application not approved, documents missing or expired, or no approved vehicle/tier.
    NotApproved,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    pub heading: Option<i16>,
    pub speed_kmh: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Online {
    pub online_since: time::OffsetDateTime,
    pub tiers: Vec<String>,
    pub wheelchair: bool,
}

/// can_go_online, account status, active vehicle, approved tiers, vehicle wheelchair access.
type EligibilityRow = (bool, String, Option<Uuid>, Vec<String>, Option<bool>);

/// Puts the driver online from this session, or moves an existing online row to it (a new
/// sign-in on another phone takes over). Opens a shift if none is open.
pub async fn go_online(
    conn: &mut PgConnection,
    driver_id: Uuid,
    session_id: Uuid,
    at: Position,
) -> Result<Result<Online, NotEligible>, sqlx::Error> {
    let row: Option<EligibilityRow> = sqlx::query_as(
        "SELECT dp.can_go_online, u.status::text, dp.active_vehicle_id,
                coalesce((SELECT array_agg(t.tier::text ORDER BY t.tier) FROM driver_tiers t WHERE t.driver_id = dp.user_id), '{}'),
                (SELECT v.wheelchair_accessible FROM vehicles v WHERE v.id = dp.active_vehicle_id AND v.approved)
         FROM driver_profiles dp JOIN users u ON u.id = dp.user_id WHERE dp.user_id = $1 FOR UPDATE OF dp",
    )
    .bind(driver_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((can, status, vehicle, tiers, wheelchair)) = row else { return Ok(Err(NotEligible::NotApproved)) };
    if status == "suspended" {
        return Ok(Err(NotEligible::Suspended));
    }
    let (Some(vehicle), Some(wheelchair)) = (vehicle, wheelchair) else { return Ok(Err(NotEligible::NotApproved)) };
    if !can || status != "active" || tiers.is_empty() {
        return Ok(Err(NotEligible::NotApproved));
    }
    let shift: Uuid = match sqlx::query_scalar("SELECT id FROM driver_shifts WHERE driver_id = $1 AND ended_at IS NULL")
        .bind(driver_id)
        .fetch_optional(&mut *conn)
        .await?
    {
        Some(id) => id,
        None => {
            sqlx::query_scalar("INSERT INTO driver_shifts (driver_id, vehicle_id, tier) VALUES ($1, $2, $3::ride_tier) RETURNING id")
                .bind(driver_id)
                .bind(vehicle)
                .bind(&tiers[0])
                .fetch_one(&mut *conn)
                .await?
        }
    };
    let since: time::OffsetDateTime = sqlx::query_scalar(
        "INSERT INTO driver_presence (driver_id, session_id, shift_id, vehicle_id, tiers, wheelchair, location, heading, speed_kmh)
         VALUES ($1, $2, $3, $4, $5::ride_tier[], $6, ST_SetSRID(ST_MakePoint($8, $7), 4326)::geography, $9, $10)
         ON CONFLICT (driver_id) DO UPDATE SET session_id = EXCLUDED.session_id, vehicle_id = EXCLUDED.vehicle_id,
           tiers = EXCLUDED.tiers, wheelchair = EXCLUDED.wheelchair, location = EXCLUDED.location,
           heading = EXCLUDED.heading, speed_kmh = EXCLUDED.speed_kmh, updated_at = now()
         RETURNING online_since",
    )
    .bind(driver_id)
    .bind(session_id)
    .bind(shift)
    .bind(vehicle)
    .bind(&tiers)
    .bind(wheelchair)
    .bind(at.lat)
    .bind(at.lon)
    .bind(at.heading)
    .bind(at.speed_kmh)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Ok(Online { online_since: since, tiers, wheelchair }))
}

/// Records a new position. False if this session is not the one online (offline, or signed in
/// elsewhere), in which case the app should show "offline".
pub async fn update_location<'e>(db: impl PgExecutor<'e>, driver_id: Uuid, session_id: Uuid, at: Position) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE driver_presence SET location = ST_SetSRID(ST_MakePoint($4, $3), 4326)::geography, heading = $5, speed_kmh = $6, updated_at = now()
         WHERE driver_id = $1 AND session_id = $2",
    )
    .bind(driver_id)
    .bind(session_id)
    .bind(at.lat)
    .bind(at.lon)
    .bind(at.heading)
    .bind(at.speed_kmh)
    .execute(db)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// Takes the driver offline: withdraws a pending offer (matching moves on) and ends the shift.
/// A driver on a trip stays assigned to it; she just receives no new requests.
pub async fn go_offline(conn: &mut PgConnection, driver_id: Uuid, reason: &str) -> Result<bool, sqlx::Error> {
    let gone = sqlx::query("DELETE FROM driver_presence WHERE driver_id = $1").bind(driver_id).execute(&mut *conn).await?.rows_affected();
    sqlx::query("UPDATE ride_offers SET response = 'withdrawn', responded_at = now() WHERE driver_id = $1 AND response = 'pending'")
        .bind(driver_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE driver_shifts SET ended_at = now(), end_reason = $2 WHERE driver_id = $1 AND ended_at IS NULL")
        .bind(driver_id)
        .bind(reason)
        .execute(&mut *conn)
        .await?;
    Ok(gone == 1)
}

/// The driver's online row, if this session holds it.
pub async fn status<'e>(db: impl PgExecutor<'e>, driver_id: Uuid, session_id: Uuid) -> Result<Option<Online>, sqlx::Error> {
    let row: Option<(time::OffsetDateTime, Vec<String>, bool)> =
        sqlx::query_as("SELECT online_since, tiers::text[], wheelchair FROM driver_presence WHERE driver_id = $1 AND session_id = $2")
            .bind(driver_id)
            .bind(session_id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(online_since, tiers, wheelchair)| Online { online_since, tiers, wheelchair }))
}

/// Takes offline every driver whose session has ended, or who has sent no position for five
/// times the matching staleness (phone off, app killed). Matching already ignores her after
/// `stale_s`; this ends the shift. Returns how many.
pub async fn sweep_stale(conn: &mut PgConnection, stale_s: i64) -> Result<u64, sqlx::Error> {
    let gone: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT p.driver_id, (s.id IS NULL) FROM driver_presence p
         LEFT JOIN sessions s ON s.id = p.session_id AND s.revoked_at IS NULL AND s.expires_at > now()
         WHERE s.id IS NULL OR p.updated_at < now() - make_interval(secs => $1)
         FOR UPDATE OF p SKIP LOCKED",
    )
    .bind(stale_s as f64 * 5.0)
    .fetch_all(&mut *conn)
    .await?;
    for (driver, signed_out) in &gone {
        go_offline(conn, *driver, if *signed_out { "signed_out" } else { "stale" }).await?;
    }
    Ok(gone.len() as u64)
}

/// What matching needs to know about one eligible driver.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub driver_id: Uuid,
    pub lat: f64,
    pub lon: f64,
    pub straight_m: f64,
    pub acceptance: f64,
    pub priority_points: i32,
    pub favourite: bool,
}

pub struct Search<'a> {
    pub lat: f64,
    pub lon: f64,
    pub tier: &'a str,
    pub wheelchair: bool,
    pub radius_m: f64,
    pub stale_s: i64,
    /// Drivers already offered this ride in this round.
    pub exclude: &'a [Uuid],
    /// For the favourite-driver bonus.
    pub rider_id: Option<Uuid>,
    pub limit: i64,
    pub lockout: LockoutRules,
}

/// Eligible drivers near the pickup, nearest first (PRD D-1): online from a live session,
/// reporting recently, approved and allowed online, the right tier and wheelchair access, no
/// active trip, no request on screen, not held after a PIN lockout, within the radius in a
/// straight line.
pub async fn candidates<'e>(db: impl PgExecutor<'e>, q: &Search<'_>) -> Result<Vec<Candidate>, sqlx::Error> {
    let rows: Vec<(Uuid, f64, f64, f64, f64, i32, bool)> = sqlx::query_as(&format!(
        "WITH pickup AS (SELECT ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography AS g)
         SELECT p.driver_id, ST_Y(p.location::geometry), ST_X(p.location::geometry), ST_Distance(p.location, pickup.g),
                dp.acceptance_rate::float8, dp.priority_points,
                EXISTS (SELECT 1 FROM favourite_drivers f WHERE f.rider_id = $8 AND f.driver_id = p.driver_id)
         FROM driver_presence p
         CROSS JOIN pickup
         JOIN driver_profiles dp ON dp.user_id = p.driver_id
         JOIN users u ON u.id = p.driver_id
         JOIN sessions s ON s.id = p.session_id AND s.revoked_at IS NULL AND s.expires_at > now()
         WHERE ST_DWithin(p.location, pickup.g, $4)
           AND $3::ride_tier = ANY(p.tiers)
           AND (NOT $5 OR p.wheelchair)
           AND p.updated_at > now() - make_interval(secs => $6)
           AND dp.can_go_online AND u.status = 'active'
           AND p.driver_id <> ALL($7)
           AND NOT EXISTS (SELECT 1 FROM rides r WHERE r.driver_id = p.driver_id AND r.state::text = ANY($9))
           AND NOT EXISTS (SELECT 1 FROM ride_offers o WHERE o.driver_id = p.driver_id AND o.response = 'pending')
           AND NOT EXISTS ({held})
         ORDER BY ST_Distance(p.location, pickup.g)
         LIMIT $10",
        held = held_sql("p.driver_id", "$11", "$12", "$13"),
    ))
    .bind(q.lat)
    .bind(q.lon)
    .bind(q.tier)
    .bind(q.radius_m)
    .bind(q.wheelchair)
    .bind(q.stale_s as f64)
    .bind(q.exclude)
    .bind(q.rider_id)
    .bind(&DRIVER_ACTIVE[..])
    .bind(q.limit)
    .bind(q.lockout.hold_s as f64)
    .bind(q.lockout.repeat)
    .bind(q.lockout.window_days as i32)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(driver_id, lat, lon, straight_m, acceptance, priority_points, favourite)| Candidate {
            driver_id,
            lat,
            lon,
            straight_m,
            acceptance,
            priority_points,
            favourite,
        })
        .collect())
}

/// A driver held from new requests after a PIN lockout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    /// When requests start again; None while she waits for the safety desk (repeat lockouts).
    pub until: Option<time::OffsetDateTime>,
}

/// The hold rule, shared by matching and the driver's status so both always agree: her latest
/// lockout is still open (the safety desk closing it releases her early) and either it was
/// less than `hold` ago, or it makes `repeat` lockouts within `days`.
fn held_sql(driver: &str, hold: &str, repeat: &str, days: &str) -> String {
    format!(
        "SELECT l.until, l.review FROM (
           SELECT i.created_at + make_interval(secs => {hold}) AS until, count(*) OVER () >= {repeat} AS review,
                  i.status <> 'closed' AS open
           FROM safety_incidents i JOIN rides r ON r.id = i.ride_id
           WHERE r.driver_id = {driver} AND i.kind = 'pin_lockout' AND i.created_at > now() - make_interval(days => {days})
           ORDER BY i.created_at DESC LIMIT 1
         ) l WHERE l.open AND (l.review OR l.until > now())"
    )
}

/// Whether, and until when, she is held after a PIN lockout.
pub async fn lockout_hold<'e>(db: impl PgExecutor<'e>, driver_id: Uuid, rules: &LockoutRules) -> Result<Option<Hold>, sqlx::Error> {
    let row: Option<(time::OffsetDateTime, bool)> = sqlx::query_as(&held_sql("$1", "$2", "$3", "$4"))
        .bind(driver_id)
        .bind(rules.hold_s as f64)
        .bind(rules.repeat)
        .bind(rules.window_days as i32)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|(until, review)| Hold { until: (!review).then_some(until) }))
}
