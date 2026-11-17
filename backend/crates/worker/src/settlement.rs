//! Weekly settlement (PRD P-6): on Monday, the week before (Monday to Sunday, Windhoek time)
//! is closed for every driver with unsettled ledger entries. Each gets one `payouts` row with
//! the net amount (negative: she owes VARA the service fees on her cash trips), and those
//! entries are linked to it. Running it again for the same week changes nothing.
//!
//! Ops settles the rows by hand until a payout provider exists (`ops::settlements`).

use fuber_db::sqlx::{self, PgPool};
use time::{Date, Duration, Weekday};

/// The Monday that starts the last full week before `today`.
pub fn last_week_start(today: Date) -> Date {
    let this_monday = today - Duration::days(i64::from(today.weekday().number_days_from_monday()));
    this_monday - Duration::days(7)
}

/// Closes the week starting `week_start` (a Monday). Returns how many statements were made.
pub async fn settle_week(db: &PgPool, week_start: Date) -> Result<u64, sqlx::Error> {
    debug_assert_eq!(week_start.weekday(), Weekday::Monday);
    let week_end = week_start + Duration::days(6);
    let made = sqlx::query(
        "WITH cutoff AS (SELECT (($2::date + 1)::timestamp AT TIME ZONE 'Africa/Windhoek') AS at),
         sums AS (
           SELECT l.driver_id, sum(l.amount_cents)::int8 AS amount FROM driver_ledger l, cutoff
           WHERE l.payout_id IS NULL AND l.created_at < cutoff.at
           GROUP BY l.driver_id HAVING sum(l.amount_cents) <> 0
         ),
         p AS (
           INSERT INTO payouts (driver_id, period_start, period_end, amount_cents, status)
           SELECT driver_id, $1, $2, amount, 'pending' FROM sums
           ON CONFLICT (driver_id, period_start) DO NOTHING
           RETURNING id, driver_id
         )
         UPDATE driver_ledger l SET payout_id = p.id FROM p, cutoff
         WHERE l.driver_id = p.driver_id AND l.payout_id IS NULL AND l.created_at < cutoff.at",
    )
    .bind(week_start)
    .bind(week_end)
    .execute(db)
    .await?
    .rows_affected();
    let statements: i64 = sqlx::query_scalar("SELECT count(*) FROM payouts WHERE period_start = $1").bind(week_start).fetch_one(db).await?;
    if made > 0 {
        tracing::info!(%week_start, statements, entries = made, "week settled");
    }
    Ok(statements as u64)
}

#[cfg(test)]
mod tests {
    use time::Month;

    use super::*;

    fn nov(day: u8) -> Date {
        Date::from_calendar_date(2026, Month::November, day).unwrap()
    }

    #[test]
    fn last_week_is_the_full_week_before() {
        // From Monday 16 to Sunday 22 November, the last full week starts on Monday 9.
        assert_eq!(last_week_start(nov(16)), nov(9));
        assert_eq!(last_week_start(nov(22)), nov(9));
        assert_eq!(last_week_start(nov(23)), nov(16));
    }
}
