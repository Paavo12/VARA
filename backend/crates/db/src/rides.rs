//! Ride state changes (TRD §4.1). Every change goes through [`transition`]: it locks the ride,
//! asks the domain state machine whether the event is allowed, bumps `version` and appends to
//! `ride_events` in the caller's transaction. The `ride_events` trigger announces the change
//! to the WebSocket hubs when the transaction commits.

use fuber_domain::{Party, RideEvent, RideState, TransitionError, priority::Priority};
use serde_json::Value;
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

/// States in which a ride occupies its rider (matches the `one_active_ride_rider` index).
pub const RIDER_ACTIVE: [&str; 8] =
    ["REQUESTED", "SEARCHING", "NO_DRIVERS", "DRIVER_ASSIGNED", "DRIVER_EN_ROUTE", "DRIVER_ARRIVED", "PIN_VERIFIED", "IN_TRIP"];
/// States in which a ride occupies its driver (matches the `one_active_ride_driver` index).
pub const DRIVER_ACTIVE: [&str; 5] = ["DRIVER_ASSIGNED", "DRIVER_EN_ROUTE", "DRIVER_ARRIVED", "PIN_VERIFIED", "IN_TRIP"];

#[derive(Debug, thiserror::Error)]
pub enum RideError {
    #[error("ride not found")]
    NotFound,
    #[error(transparent)]
    Invalid(#[from] TransitionError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

pub fn party_str(p: Party) -> &'static str {
    match p {
        Party::Rider => "rider",
        Party::Driver => "driver",
        Party::System => "system",
        Party::Staff => "staff",
    }
}

/// Locks the ride and returns its state.
pub async fn lock(conn: &mut PgConnection, ride_id: Uuid) -> Result<RideState, RideError> {
    let state: Option<String> =
        sqlx::query_scalar("SELECT state::text FROM rides WHERE id = $1 FOR UPDATE").bind(ride_id).fetch_optional(&mut *conn).await?;
    state.as_deref().and_then(RideState::parse).ok_or(RideError::NotFound)
}

/// Applies `event` to the ride and records it. Other columns (driver, timestamps) are set by
/// the caller in the same transaction.
pub async fn transition(
    conn: &mut PgConnection,
    ride_id: Uuid,
    event: RideEvent,
    actor: Party,
    actor_id: Option<Uuid>,
    detail: Option<Value>,
) -> Result<RideState, RideError> {
    let from = lock(conn, ride_id).await?;
    let to = from.apply(event)?;
    sqlx::query("UPDATE rides SET state = $2::ride_state, version = version + 1, updated_at = now() WHERE id = $1")
        .bind(ride_id)
        .bind(to.as_str())
        .execute(&mut *conn)
        .await?;
    record(conn, ride_id, Some(from), to, actor, actor_id, detail).await?;
    Ok(to)
}

/// Appends a row to the ride's history (and so notifies both parties on commit).
pub async fn record<'e>(
    db: impl PgExecutor<'e>,
    ride_id: Uuid,
    from: Option<RideState>,
    to: RideState,
    actor: Party,
    actor_id: Option<Uuid>,
    detail: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ride_events (ride_id, from_state, to_state, actor, actor_id, detail)
         VALUES ($1, $2::ride_state, $3::ride_state, $4::party, $5, $6)",
    )
    .bind(ride_id)
    .bind(from.map(RideState::as_str))
    .bind(to.as_str())
    .bind(party_str(actor))
    .bind(actor_id)
    .bind(detail)
    .execute(db)
    .await
    .map(|_| ())
}

/// Takes priority points for a cancellation or no-show (PRD D-5) and records the entry.
pub async fn penalise(conn: &mut PgConnection, user_id: Uuid, role: Party, ride_id: Uuid, reason: &str) -> Result<i32, sqlx::Error> {
    let (select, update) = match role {
        Party::Driver => (
            "SELECT priority_points, cancel_count FROM driver_profiles WHERE user_id = $1 FOR UPDATE",
            "UPDATE driver_profiles SET priority_points = $2, cancel_count = $3 WHERE user_id = $1",
        ),
        _ => (
            "SELECT priority_points, cancel_count FROM rider_profiles WHERE user_id = $1 FOR UPDATE",
            "UPDATE rider_profiles SET priority_points = $2, cancel_count = $3 WHERE user_id = $1",
        ),
    };
    let (points, cancels): (i32, i32) = sqlx::query_as(select).bind(user_id).fetch_one(&mut *conn).await?;
    let mut p = Priority { points, cancels };
    let lost = p.penalise();
    sqlx::query(update).bind(user_id).bind(p.points).bind(p.cancels).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO priority_ledger (user_id, role, ride_id, delta, reason, balance) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(user_id)
        .bind(party_str(role))
        .bind(ride_id)
        .bind(-lost)
        .bind(reason)
        .bind(p.points)
        .execute(&mut *conn)
        .await?;
    Ok(lost)
}

/// Adds the completed-trip priority points (PRD D-5) and records the entry.
pub async fn reward(conn: &mut PgConnection, user_id: Uuid, role: Party, ride_id: Uuid) -> Result<(), sqlx::Error> {
    let table = if role == Party::Driver { "driver_profiles" } else { "rider_profiles" };
    let (points, cancels): (i32, i32) =
        sqlx::query_as(&format!("SELECT priority_points, cancel_count FROM {table} WHERE user_id = $1 FOR UPDATE"))
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await?;
    let mut p = Priority { points, cancels };
    p.reward_trip();
    sqlx::query(&format!("UPDATE {table} SET priority_points = $2 WHERE user_id = $1"))
        .bind(user_id)
        .bind(p.points)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO priority_ledger (user_id, role, ride_id, delta, reason, balance) VALUES ($1, $2, $3, $4, 'trip_completed', $5)",
    )
    .bind(user_id)
    .bind(party_str(role))
    .bind(ride_id)
    .bind(p.points - points)
    .bind(p.points)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Rolling acceptance rate used by matching: each answer moves it 5 % towards 1 (accepted) or
/// 0 (declined or let expire).
pub async fn record_answer<'e>(db: impl PgExecutor<'e>, driver_id: Uuid, accepted: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE driver_profiles SET acceptance_rate = round(acceptance_rate * 0.95 + $2::numeric * 0.05, 3) WHERE user_id = $1")
        .bind(driver_id)
        .bind(i32::from(accepted))
        .execute(db)
        .await
        .map(|_| ())
}
