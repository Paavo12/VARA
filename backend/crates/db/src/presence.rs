//! Drivers online (PRD D-1) and the candidate search used by matching (TRD §4.2 steps 1–2).
//! The only code that touches `driver_presence`.

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::rides::DRIVER_ACTIVE;

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
    /// Going online, the reported position was implausible: she is placed at her last
    /// accepted one until her phone reports a reachable position.
    pub fix_ignored: bool,
}

/// can_go_online, account status, active vehicle, approved tiers, vehicle wheelchair access.
type EligibilityRow = (bool, String, Option<Uuid>, Vec<String>, Option<bool>);

/// A reported position is believed only if a car could have got there from her last accepted
/// one: at most `FIX_SLACK_M` (GPS error) plus `FIX_MAX_SPEED_MPS` (150 km/h) for every second
/// since. The exact pickup unlocks within 50 m of it, so a phone that claims to jump about can't
/// sweep the approximate pickup circle faster than she could drive it (security review 2026-11-24).
pub const FIX_SLACK_M: f64 = 100.0;
pub const FIX_MAX_SPEED_MPS: f64 = 150.0 / 3.6;

/// `$2` is the reported point; true if it is reachable from `last_fix` (or there is none yet).
const REACHABLE: &str =
    "(last_fix IS NULL OR ST_Distance(last_fix, ST_GeogFromText($2)) <= $3 + $4 * greatest(extract(epoch FROM now() - last_fix_at), 0))";

/// What happened to a reported position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    Recorded,
    /// Online, but no car could have got there since her last position: not stored.
    Implausible,
    /// This session is not the one online (offline, or signed in elsewhere).
    Offline,
}

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
    // Her last accepted position survives going offline, so going offline and back on can't jump.
    let (reachable, last_lat, last_lon): (bool, Option<f64>, Option<f64>) = sqlx::query_as(&format!(
        "SELECT {REACHABLE}, ST_Y(last_fix::geometry), ST_X(last_fix::geometry) FROM driver_profiles WHERE user_id = $1"
    ))
    .bind(driver_id)
    .bind(point(at))
    .bind(FIX_SLACK_M)
    .bind(FIX_MAX_SPEED_MPS)
    .fetch_one(&mut *conn)
    .await?;
    let at = match (reachable, last_lat, last_lon) {
        (true, _, _) => {
            sqlx::query("UPDATE driver_profiles SET last_fix = ST_GeogFromText($2), last_fix_at = now() WHERE user_id = $1")
                .bind(driver_id)
                .bind(point(at))
                .execute(&mut *conn)
                .await?;
            at
        }
        (false, Some(lat), Some(lon)) => Position { lat, lon, heading: None, speed_kmh: None },
        (false, _, _) => at,
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
    Ok(Ok(Online { online_since: since, tiers, wheelchair, fix_ignored: !reachable }))
}

/// Records a new position if this session is the one online and the position is reachable
/// from her last one (see `FIX_MAX_SPEED_MPS`). One statement, so it can't race itself.
pub async fn update_location<'e>(db: impl PgExecutor<'e>, driver_id: Uuid, session_id: Uuid, at: Position) -> Result<Fix, sqlx::Error> {
    let (online, recorded): (bool, bool) = sqlx::query_as(&format!(
        "WITH here AS (SELECT 1 FROM driver_presence WHERE driver_id = $1 AND session_id = $5),
         fix AS (
           UPDATE driver_profiles SET last_fix = ST_GeogFromText($2), last_fix_at = now()
           WHERE user_id = $1 AND EXISTS (SELECT 1 FROM here) AND {REACHABLE}
           RETURNING 1
         ),
         moved AS (
           UPDATE driver_presence SET location = ST_GeogFromText($2), heading = $6, speed_kmh = $7, updated_at = now()
           WHERE driver_id = $1 AND session_id = $5 AND EXISTS (SELECT 1 FROM fix)
           RETURNING 1
         )
         SELECT EXISTS (SELECT 1 FROM here), EXISTS (SELECT 1 FROM moved)"
    ))
    .bind(driver_id)
    .bind(point(at))
    .bind(FIX_SLACK_M)
    .bind(FIX_MAX_SPEED_MPS)
    .bind(session_id)
    .bind(at.heading)
    .bind(at.speed_kmh)
    .fetch_one(db)
    .await?;
    Ok(match (online, recorded) {
        (false, _) => Fix::Offline,
        (true, true) => Fix::Recorded,
        (true, false) => Fix::Implausible,
    })
}

/// The position as a PostGIS geography literal (WGS 84 is geography's default SRID).
fn point(at: Position) -> String {
    format!("POINT({} {})", at.lon, at.lat)
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
    Ok(row.map(|(online_since, tiers, wheelchair)| Online { online_since, tiers, wheelchair, fix_ignored: false }))
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
        held = held_sql("p.driver_id"),
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

/// The hold rule, shared by matching and the driver's status so both always agree: any PIN
/// lockout on her trips stays a hold, however old, until the safety desk closes it (PRD T-3).
fn held_sql(driver: &str) -> String {
    format!(
        "SELECT 1 FROM safety_incidents i JOIN rides r ON r.id = i.ride_id
         WHERE r.driver_id = {driver} AND i.kind = 'pin_lockout' AND i.status <> 'closed'"
    )
}

/// Whether she is held from new requests after a PIN lockout.
pub async fn lockout_hold<'e>(db: impl PgExecutor<'e>, driver_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(&format!("SELECT EXISTS ({})", held_sql("$1"))).bind(driver_id).fetch_one(db).await
}
