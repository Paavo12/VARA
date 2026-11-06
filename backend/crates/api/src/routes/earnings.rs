//! Cash and earnings (PRD P-2, P-5, P-6).
//!
//! The driver ledger is positive when VARA owes the driver and negative when she owes VARA.
//! On a cash trip she keeps the whole fare, so confirming the cash books VARA's service fee
//! against her (`service_fee`, negative); card trips (with the payment provider) will book her
//! earning instead. Weekly settlement nets the balance.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use fuber_domain::Cents;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::DriverUser,
    error::{ApiError, ErrorBody},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct CashConfirmed {
    pub ride_id: Uuid,
    /// `cash_confirmed`.
    pub payment_status: String,
    /// VARA's service fee on this trip, added to what she owes.
    pub service_fee: Cents,
    /// Her balance after this trip: negative means she owes VARA.
    pub balance: Cents,
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/cash-received", tag = "earnings", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Cash recorded (tapping again changes nothing)", body = CashConfirmed),
        (status = 404, description = "Not her ride", body = ErrorBody),
        (status = 409, description = "`not_cash` or `not_completed`", body = ErrorBody),
    )
)]
pub async fn cash_received(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    Path(id): Path<Uuid>,
) -> Result<Json<CashConfirmed>, ApiError> {
    let mut tx = s.db.begin().await?;
    let ride: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT state::text, payment_method::text, service_fee_cents FROM rides WHERE id = $1 AND driver_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (state, method, fee) = ride.ok_or_else(ApiError::not_found)?;
    if method != "cash" {
        return Err(ApiError::conflict("not_cash", "This trip is paid by card."));
    }
    if state != "COMPLETED" && state != "RATED" {
        return Err(ApiError::conflict("not_completed", "Confirm the cash once the trip is complete."));
    }
    sqlx::query("UPDATE payments SET status = 'cash_confirmed', updated_at = now() WHERE ride_id = $1 AND status = 'pending'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO driver_ledger (driver_id, ride_id, kind, amount_cents, note)
         VALUES ($1, $2, 'service_fee', $3, 'cash trip: service fee owed')
         ON CONFLICT (ride_id, kind) WHERE ride_id IS NOT NULL DO NOTHING",
    )
    .bind(user.user_id)
    .bind(id)
    .bind(-fee)
    .execute(&mut *tx)
    .await?;
    let balance: i64 = sqlx::query_scalar("SELECT coalesce(sum(amount_cents), 0)::int8 FROM driver_ledger WHERE driver_id = $1")
        .bind(user.user_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(ride_id = %id, "cash confirmed");
    Ok(Json(CashConfirmed { ride_id: id, payment_status: "cash_confirmed".into(), service_fee: Cents(fee), balance: Cents(balance) }))
}

#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    #[default]
    Today,
    Week,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EarningsQuery {
    /// `today` (default) or `week` (since Monday), Windhoek time.
    #[param(inline)]
    #[serde(default)]
    pub period: Period,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EarningsTrip {
    pub ride_id: Uuid,
    pub code: String,
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,
    pub earning: Cents,
    pub payment_method: String,
    /// `pending` until she confirms the cash.
    pub payment_status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Earnings {
    /// `today` or `week`.
    pub period: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    pub since: OffsetDateTime,
    pub trips: i64,
    /// What she earned: fares minus VARA's service fee.
    pub earnings: Cents,
    /// Cash she collected from passengers.
    pub cash_collected: Cents,
    /// VARA's service fee on these trips.
    pub service_fees: Cents,
    /// Time online in the period.
    pub online_s: i64,
    /// All-time balance: positive means VARA pays her at settlement, negative means she owes VARA.
    pub balance: Cents,
    /// Completed trips in the period, newest first.
    pub recent: Vec<EarningsTrip>,
}

type TotalsRow = (OffsetDateTime, i64, i64, i64, i64, i64, i64);
type TripRow = (Uuid, String, OffsetDateTime, i64, String, Option<String>);

#[utoipa::path(
    get, path = "/v1/driver/earnings", tag = "earnings", security(("bearer" = [])),
    params(EarningsQuery),
    responses((status = 200, description = "Earnings dashboard (PRD P-6)", body = Earnings))
)]
pub async fn earnings(
    State(s): State<AppState>,
    DriverUser(user): DriverUser,
    Query(q): Query<EarningsQuery>,
) -> Result<Json<Earnings>, ApiError> {
    let unit = match q.period {
        Period::Today => "day",
        Period::Week => "week",
    };
    let (since, trips, earned, cash, fees, online_s, balance): TotalsRow = sqlx::query_as(
        "WITH p AS (SELECT date_trunc($2, now() AT TIME ZONE 'Africa/Windhoek') AT TIME ZONE 'Africa/Windhoek' AS since)
         SELECT p.since,
                count(r.id),
                coalesce(sum(r.driver_earning_cents), 0)::int8,
                coalesce(sum(r.fare_cents) FILTER (WHERE pay.status = 'cash_confirmed'), 0)::int8,
                coalesce(sum(r.service_fee_cents), 0)::int8,
                (SELECT coalesce(sum(extract(epoch FROM least(coalesce(sh.ended_at, now()), now()) - greatest(sh.started_at, p.since))), 0)::int8
                 FROM driver_shifts sh WHERE sh.driver_id = $1 AND coalesce(sh.ended_at, now()) > p.since),
                (SELECT coalesce(sum(amount_cents), 0)::int8 FROM driver_ledger WHERE driver_id = $1)
         FROM p
         LEFT JOIN rides r ON r.driver_id = $1 AND r.state IN ('COMPLETED', 'RATED') AND r.completed_at >= p.since
         LEFT JOIN payments pay ON pay.ride_id = r.id
         GROUP BY p.since",
    )
    .bind(user.user_id)
    .bind(unit)
    .fetch_one(&s.db)
    .await?;
    let recent: Vec<TripRow> = sqlx::query_as(
        "SELECT r.id, r.code, r.completed_at, r.driver_earning_cents, r.payment_method::text,
                (SELECT status::text FROM payments WHERE ride_id = r.id)
         FROM rides r WHERE r.driver_id = $1 AND r.state IN ('COMPLETED', 'RATED') AND r.completed_at >= $2
         ORDER BY r.completed_at DESC LIMIT 50",
    )
    .bind(user.user_id)
    .bind(since)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(Earnings {
        period: match q.period {
            Period::Today => "today",
            Period::Week => "week",
        },
        since,
        trips,
        earnings: Cents(earned),
        cash_collected: Cents(cash),
        service_fees: Cents(fees),
        online_s,
        balance: Cents(balance),
        recent: recent
            .into_iter()
            .map(|(ride_id, code, completed_at, earning, payment_method, payment_status)| EarningsTrip {
                ride_id,
                code,
                completed_at,
                earning: Cents(earning),
                payment_method,
                payment_status,
            })
            .collect(),
    }))
}
