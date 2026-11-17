//! Weekly settlement tests (PRD P-6) against a real PostGIS database: a cash trip's service fee
//! becomes last week's statement, settling it by hand zeroes her balance and is audited, a
//! second run of the week changes nothing, only admins can settle, and drivers see their own
//! statements only.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use fuber_worker::settlement;
use serde_json::json;
use time::{Duration, OffsetDateTime, UtcOffset};

use common::*;

#[tokio::test]
async fn last_weeks_cash_fees_are_settled_by_hand_and_audited() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let (status, cash) = t.post(&format!("/v1/rides/{}/cash-received", trip.id), &trip.driver.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{cash}");
    let fee = cash["service_fee"].as_i64().unwrap();
    assert!(fee > 0);

    // The fee was booked last week.
    let today = OffsetDateTime::now_utc().to_offset(UtcOffset::from_hms(2, 0, 0).unwrap()).date();
    let week = settlement::last_week_start(today);
    sqlx::query("UPDATE driver_ledger SET created_at = ($2::date + 2)::timestamp AT TIME ZONE 'Africa/Windhoek' WHERE driver_id = $1")
        .bind(trip.driver.user_id)
        .bind(week)
        .execute(&t.state.db)
        .await
        .unwrap();
    assert_eq!(settlement::settle_week(&t.state.db, week).await.unwrap(), 1);
    assert_eq!(settlement::settle_week(&t.state.db, week).await.unwrap(), 1, "running again changes nothing");

    let (status, list) = t.get("/v1/driver/statements", &trip.driver.access).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["amount"], -fee);
    assert_eq!(list[0]["status"], "pending");
    assert_eq!(list[0]["period_start"], week.to_string());
    assert_eq!(list[0]["period_end"], (week + Duration::days(6)).to_string());

    // Only admins see and settle statements.
    let safety = t.staff("ops_safety").await;
    assert_eq!(t.call(Method::GET, "/v1/ops/settlements", Some(&safety), None).await.0, StatusCode::FORBIDDEN);
    let admin = t.staff("ops_admin").await;
    let (status, queue) = t.call(Method::GET, "/v1/ops/settlements", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let id = queue[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(queue[0]["driver_id"], trip.driver.user_id.to_string());

    let uri = format!("/v1/ops/settlements/{id}/settle");
    assert_eq!(t.call(Method::POST, &uri, Some(&admin), Some(json!({ "reference": "" }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, done) = t.call(Method::POST, &uri, Some(&admin), Some(json!({ "reference": "Deposit slip 4471" }))).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["status"], "received");
    let (status, e) = t.call(Method::POST, &uri, Some(&admin), Some(json!({ "reference": "again" }))).await;
    assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("already_settled")));

    // Her balance is back to zero, and the audit log has it.
    let (_, earnings) = t.get("/v1/driver/earnings", &trip.driver.access).await;
    assert_eq!(earnings["balance"], 0);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'settlement.settle' AND target_id = $1")
        .bind(&id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(audited, 1);

    // An entry from this week stays out of last week's statement.
    sqlx::query("INSERT INTO driver_ledger (driver_id, kind, amount_cents, note) VALUES ($1, 'adjustment', -500, 'test')")
        .bind(trip.driver.user_id)
        .execute(&t.state.db)
        .await
        .unwrap();
    assert_eq!(settlement::settle_week(&t.state.db, week).await.unwrap(), 1);
    let unsettled: i64 = sqlx::query_scalar("SELECT count(*) FROM driver_ledger WHERE driver_id = $1 AND payout_id IS NULL")
        .bind(trip.driver.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(unsettled, 1);
    // Riders can't see statements.
    assert_eq!(t.get("/v1/driver/statements", &trip.rider.access).await.0, StatusCode::FORBIDDEN);
}
