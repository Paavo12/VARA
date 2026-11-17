//! Report an issue tests (PRD S-5) against a real PostGIS database: safety reports reach the
//! desk, other reports wait for support, the 30-day window, validation, wrong-user access and
//! the daily limit.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_api::rate_limit::{RateLimits, Rule};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

impl T {
    async fn report(&self, s: &Session, ride_id: &str, body: Value) -> (StatusCode, Value) {
        self.post(&format!("/v1/rides/{ride_id}/report"), &s.access, body).await
    }
}

#[tokio::test]
async fn a_safety_report_reaches_the_desk_and_a_lost_item_goes_to_support() {
    let t = setup!(relaxed());
    let trip = t.completed().await;

    let text = "He kept asking for my number.\nPlease look into it.";
    let (status, r) = t.report(&trip.rider, &trip.id, json!({ "category": "harassment", "text": text })).await;
    assert_eq!(status, StatusCode::CREATED, "{r}");
    assert_eq!(r["safety"], true);
    assert!(r["message"].as_str().unwrap().contains("safety team"));

    let desk = t.staff("ops_safety").await;
    let (_, queue) = t.call(Method::GET, "/v1/ops/incidents", Some(&desk), None).await;
    let incident = queue.as_array().unwrap().iter().find(|i| i["kind"] == "report").expect("a report incident");
    assert_eq!(incident["ride_id"], trip.id.as_str());
    assert_eq!(incident["raised_by"]["user_id"], trip.rider.user_id.to_string());
    assert_eq!(incident["report"]["category"], "harassment");
    assert_eq!(incident["report"]["text"], text);

    // The driver reports a forgotten bag: support only, no incident.
    let (status, r) = t.report(&trip.driver, &trip.id, json!({ "category": "lost_item", "text": "A blue bag on the back seat." })).await;
    assert_eq!(status, StatusCode::CREATED, "{r}");
    assert_eq!(r["safety"], false);
    let (incidents, reports): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM safety_incidents WHERE ride_id = $1::uuid AND kind = 'report'),
                (SELECT count(*) FROM issue_reports WHERE ride_id = $1::uuid)",
    )
    .bind(&trip.id)
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    assert_eq!((incidents, reports), (1, 2));
}

#[tokio::test]
async fn only_her_own_recent_rides_can_be_reported() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let stranger = t.rider().await;
    let body = json!({ "category": "other", "text": "Something" });

    assert_eq!(t.report(&stranger, &trip.id, body.clone()).await.0, StatusCode::NOT_FOUND);
    let uri = format!("/v1/rides/{}/report", trip.id);
    assert_eq!(t.call(Method::POST, &uri, None, Some(body.clone())).await.0, StatusCode::UNAUTHORIZED);
    for bad in [
        json!({ "category": "rude", "text": "x" }),
        json!({ "category": "other", "text": "   " }),
        json!({ "category": "other", "text": "x".repeat(2001) }),
        json!({ "category": "other", "text": "x", "photo": "y" }),
        json!({ "category": "other", "text": "bell\u{7}" }),
    ] {
        assert_eq!(t.report(&trip.rider, &trip.id, bad.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }

    sqlx::query("UPDATE rides SET requested_at = now() - interval '31 days' WHERE id = $1::uuid")
        .bind(&trip.id)
        .execute(&t.state.db)
        .await
        .unwrap();
    assert_eq!(t.report(&trip.rider, &trip.id, body).await.0, StatusCode::NOT_FOUND, "more than 30 days ago");
}

#[tokio::test]
async fn reports_are_limited_per_person_per_day() {
    let t = setup!(RateLimits { reports: Rule { limit: 2, window_s: 86_400 }, ..relaxed() });
    let trip = t.completed().await;
    let body = json!({ "category": "fare", "text": "Charged twice?" });
    for _ in 0..2 {
        assert_eq!(t.report(&trip.rider, &trip.id, body.clone()).await.0, StatusCode::CREATED);
    }
    assert_eq!(t.report(&trip.rider, &trip.id, body).await.0, StatusCode::TOO_MANY_REQUESTS);
}
