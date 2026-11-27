//! Safety incidents' retention review (schema §12): 5 years after closing, an admin keeps an
//! incident another year (with a reason) or releases it, which lets its ride's data go and
//! removes its free text. Only admins decide; every decision is audited.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::{Value, json};
use uuid::Uuid;

use common::*;

/// A closed SOS on `ride`, closed `ago` (a Postgres interval), with a note, a location and a report.
async fn closed_incident(t: &T, ride: Uuid, rider: Uuid, ago: &str) -> Uuid {
    let db = &t.state.db;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO safety_incidents (ride_id, raised_by, kind, status, outcome, location, created_at, closed_at)
         VALUES ($1, $2, 'sos', 'closed', 'Spoke to her; she was safe at home.', 'POINT(17.0870 -22.5750)',
                 now() - $3::interval - interval '1 hour', now() - $3::interval)
         RETURNING id",
    )
    .bind(ride)
    .bind(rider)
    .bind(ago)
    .fetch_one(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO incident_notes (incident_id, staff_id, body) SELECT $1, id, 'Called her at 22:14.' FROM staff_users LIMIT 1")
        .bind(id)
        .execute(db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO issue_reports (ride_id, reporter_id, category, body, incident_id) VALUES ($1, $2, 'harassment', 'He followed me.', $3)",
    )
    .bind(ride)
    .bind(rider)
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn queue(t: &T, staff: &str) -> (StatusCode, Value) {
    t.call(Method::GET, "/v1/ops/incidents/retention", Some(staff), None).await
}

async fn decide(t: &T, staff: &str, id: Uuid, body: Value) -> StatusCode {
    t.post(&format!("/v1/ops/incidents/{id}/retention"), staff, body).await.0
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn admins_keep_or_release_incidents_five_years_after_they_close() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let safety = t.staff("ops_safety").await;
    let trip = t.completed().await;
    let ride: Uuid = trip.id.parse().unwrap();
    let db = &t.state.db;
    sqlx::query("UPDATE rides SET created_at = now() - interval '6 years' WHERE id = $1").bind(ride).execute(db).await.unwrap();
    let old = closed_incident(&t, ride, trip.rider.user_id, "5 years 1 day").await;
    let recent = closed_incident(&t, ride, trip.rider.user_id, "4 years").await;

    // Only admins see the queue, and only incidents past their 5 years.
    assert_eq!(queue(&t, &safety).await.0, StatusCode::FORBIDDEN);
    assert_eq!(decide(&t, &safety, old, json!({ "decision": "release" })).await, StatusCode::FORBIDDEN);
    let (status, v) = queue(&t, &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&v), [old.to_string()]);
    assert_eq!(v[0]["outcome"], "Spoke to her; she was safe at home.");
    assert_eq!(decide(&t, &admin, recent, json!({ "decision": "release" })).await, StatusCode::NOT_FOUND, "not due yet");

    // Keeping needs a reason, and brings it back after a year.
    assert_eq!(decide(&t, &admin, old, json!({ "decision": "keep" })).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(decide(&t, &admin, old, json!({ "decision": "keep", "reason": "Police case still open." })).await, StatusCode::NO_CONTENT);
    assert!(ids(&queue(&t, &admin).await.1).is_empty());
    sqlx::query("UPDATE safety_incidents SET retain_until = now() - interval '1 day' WHERE id = $1").bind(old).execute(db).await.unwrap();
    assert_eq!(ids(&queue(&t, &admin).await.1), [old.to_string()]);

    // The recent incident still holds the ride, so releasing the old one alone keeps it intact.
    assert_eq!(decide(&t, &admin, old, json!({ "decision": "release" })).await, StatusCode::NO_CONTENT);
    let (hold, outcome, located, notes, report): (bool, Option<String>, bool, i64, String) = sqlx::query_as(
        "SELECT i.hold_data, i.outcome, i.location IS NOT NULL,
                (SELECT count(*) FROM incident_notes WHERE incident_id = i.id),
                (SELECT body FROM issue_reports WHERE incident_id = i.id)
         FROM safety_incidents i WHERE i.id = $1",
    )
    .bind(old)
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!((hold, outcome, located, notes), (false, None, false, 0));
    assert_eq!(report, "(removed after the retention review)");
    assert!(ids(&queue(&t, &admin).await.1).is_empty());
    let address = || async {
        sqlx::query_scalar::<_, String>("SELECT pickup_address FROM rides WHERE id = $1").bind(ride).fetch_one(db).await.unwrap()
    };
    fuber_worker::cleanup(&t.ctx).await.unwrap();
    assert_ne!(address().await, "", "the recent incident still holds the ride");

    // Once nothing holds it, the clean-up anonymises the 6-year-old ride.
    sqlx::query("UPDATE safety_incidents SET hold_data = false WHERE id = $1").bind(recent).execute(db).await.unwrap();
    fuber_worker::cleanup(&t.ctx).await.unwrap();
    assert_eq!(address().await, "");

    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE target_type = 'incident' AND target_id = $1 ORDER BY id")
            .bind(old.to_string())
            .fetch_all(db)
            .await
            .unwrap();
    assert_eq!(actions, ["incident.retain", "incident.release"]);
}
