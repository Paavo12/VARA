//! Ops support tests (PRD O-2, O-6) against a real PostGIS database: finding a user by phone or
//! name, the audited detail view, suspending (signed out and offline at once) and reinstating,
//! priority adjustments within 0–150, the queue of reported issues, and role checks.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

impl T {
    async fn ops(&self, token: &str, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call(method, uri, Some(token), body).await
    }
}

#[tokio::test]
async fn support_finds_a_user_and_suspends_and_reinstates_her() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let support = t.staff("ops_support").await;
    let phone: String =
        sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = $1").bind(trip.driver.user_id).fetch_one(&t.state.db).await.unwrap();

    // By phone, written the local way, and by part of a name.
    let local = format!("0{}", &phone[4..]);
    let (status, found) = t.ops(&support, Method::GET, &format!("/v1/ops/users?q={local}"), None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found[0]["user_id"], trip.driver.user_id.to_string());
    assert_eq!(found[0]["is_driver"], true);
    let (_, found) = t.ops(&support, Method::GET, "/v1/ops/users?q=hilma", None).await;
    assert!(found.as_array().unwrap().iter().any(|u| u["user_id"] == trip.driver.user_id.to_string()));
    assert_eq!(t.ops(&support, Method::GET, "/v1/ops/users?q=x", None).await.0, StatusCode::UNPROCESSABLE_ENTITY);

    // The detail view has her trips and is audited.
    let uri = format!("/v1/ops/users/{}", trip.driver.user_id);
    let (status, detail) = t.ops(&support, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["trips"][0]["ride_id"], trip.id.as_str());
    assert_eq!(detail["trips"][0]["as_party"], "driver");
    let views: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'user.view' AND user_id = $1")
        .bind(trip.driver.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(views, 1);

    // Suspended: her next request is refused and she is no longer online (she still was).
    assert_eq!(t.driver_status(&trip.driver).await["online"], true);
    let reason = json!({ "reason": "Under review after a report" });
    let (status, e) = t.ops(&support, Method::POST, &format!("{uri}/suspend"), Some(reason.clone())).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");
    let (status, e) = t.get("/v1/driver/status", &trip.driver.access).await;
    assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("account_suspended")));
    let online: i64 = sqlx::query_scalar("SELECT count(*) FROM driver_presence WHERE driver_id = $1")
        .bind(trip.driver.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(online, 0);
    assert_eq!(t.ops(&support, Method::POST, &format!("{uri}/suspend"), Some(reason.clone())).await.0, StatusCode::CONFLICT);

    // Reinstated: active again.
    assert_eq!(t.ops(&support, Method::POST, &format!("{uri}/reinstate"), Some(reason.clone())).await.0, StatusCode::NO_CONTENT);
    assert_eq!(t.get("/v1/driver/status", &trip.driver.access).await.0, StatusCode::OK);
    assert_eq!(t.ops(&support, Method::POST, &format!("{uri}/reinstate"), Some(reason)).await.0, StatusCode::CONFLICT);
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log WHERE user_id = $1 AND action LIKE 'user.%' ORDER BY id")
        .bind(trip.driver.user_id)
        .fetch_all(&t.state.db)
        .await
        .unwrap();
    assert_eq!(actions, ["user.view", "user.suspend", "user.reinstate"]);
}

#[tokio::test]
async fn priority_points_are_adjusted_within_bounds_and_recorded() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let support = t.staff("ops_support").await;
    let uri = format!("/v1/ops/users/{}/priority", rider.user_id);

    let (status, r) = t.ops(&support, Method::POST, &uri, Some(json!({ "role": "rider", "delta": 30, "reason": "Goodwill" }))).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["points"], 130);
    let (_, r) = t.ops(&support, Method::POST, &uri, Some(json!({ "role": "rider", "delta": 50, "reason": "Goodwill" }))).await;
    assert_eq!(r["points"], 150, "capped at 150");
    assert_eq!(t.points("rider_profiles", &rider).await, 150);
    let (delta, reason): (i32, String) =
        sqlx::query_as("SELECT delta, reason FROM priority_ledger WHERE user_id = $1 ORDER BY id DESC LIMIT 1")
            .bind(rider.user_id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((delta, reason.as_str()), (20, "staff_adjustment"));

    for bad in [
        json!({ "role": "rider", "delta": 51, "reason": "x" }),
        json!({ "role": "rider", "delta": 0, "reason": "x" }),
        json!({ "role": "rider", "delta": 5, "reason": "" }),
        json!({ "role": "staff", "delta": 5, "reason": "x" }),
    ] {
        assert_eq!(t.ops(&support, Method::POST, &uri, Some(bad.clone())).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    // She is not a driver.
    assert_eq!(
        t.ops(&support, Method::POST, &uri, Some(json!({ "role": "driver", "delta": 5, "reason": "x" }))).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn support_works_through_reported_issues() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let (status, r) = t
        .post(&format!("/v1/rides/{}/report", trip.id), &trip.rider.access, json!({ "category": "lost_item", "text": "My phone charger" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{r}");
    let support = t.staff("ops_support").await;

    let (status, queue) = t.ops(&support, Method::GET, "/v1/ops/reports", None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue[0]["category"], "lost_item");
    assert_eq!(queue[0]["text"], "My phone charger");
    assert_eq!(queue[0]["reporter_id"], trip.rider.user_id.to_string());
    let id = queue[0]["id"].as_str().unwrap().to_owned();

    let uri = format!("/v1/ops/reports/{id}/status");
    assert_eq!(t.ops(&support, Method::POST, &uri, Some(json!({ "status": "closed" }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, e) = t.ops(&support, Method::POST, &uri, Some(json!({ "status": "resolved", "note": "Driver dropped it off" }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");
    let (_, open) = t.ops(&support, Method::GET, "/v1/ops/reports", None).await;
    assert_eq!(open.as_array().unwrap().len(), 0);
    let (_, resolved) = t.ops(&support, Method::GET, "/v1/ops/reports?status=resolved", None).await;
    assert_eq!(resolved[0]["id"], id.as_str());
}

#[tokio::test]
async fn only_support_and_admins_use_the_support_tools() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let id = rider.user_id;
    let reviewer = t.staff("ops_reviewer").await;
    let safety = t.staff("ops_safety").await;
    let admin = t.staff("ops_admin").await;
    let reason = json!({ "reason": "x" });
    for token in [&reviewer, &safety] {
        assert_eq!(t.ops(token, Method::GET, "/v1/ops/users?q=Ndeshi", None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(t.ops(token, Method::GET, &format!("/v1/ops/users/{id}"), None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(t.ops(token, Method::POST, &format!("/v1/ops/users/{id}/suspend"), Some(reason.clone())).await.0, StatusCode::FORBIDDEN);
        assert_eq!(t.ops(token, Method::GET, "/v1/ops/reports", None).await.0, StatusCode::FORBIDDEN);
    }
    assert_eq!(t.ops(&admin, Method::GET, &format!("/v1/ops/users/{id}"), None).await.0, StatusCode::OK);
    // Riders' own tokens are not staff tokens.
    assert_eq!(t.call(Method::GET, "/v1/ops/reports", Some(&rider.access), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.ops(&admin, Method::GET, &format!("/v1/ops/users/{}", uuid::Uuid::new_v4()), None).await.0, StatusCode::NOT_FOUND);
}
