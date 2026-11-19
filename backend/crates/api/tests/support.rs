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

#[tokio::test]
async fn an_admin_makes_a_driver_a_map_expert_and_her_comfort_trips_pay_15_percent() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let support = t.staff("ops_support").await;
    let rider = t.rider().await;
    let driver = t.online_driver("Hilma Nangolo", "comfort", NEAR).await;
    let uri = format!("/v1/ops/users/{}/map-expert", driver.user_id);
    let until =
        (time::OffsetDateTime::now_utc() + time::Duration::days(90)).format(&time::format_description::well_known::Rfc3339).unwrap();
    let grant = json!({ "until": until, "reason": "Verifies wayfinding in Katutura" });

    // Admins only, and a date within the next year.
    assert_eq!(t.ops(&support, Method::POST, &uri, Some(grant.clone())).await.0, StatusCode::FORBIDDEN);
    let past = json!({ "until": "2020-01-01T00:00:00Z", "reason": "Old" });
    assert_eq!(t.ops(&admin, Method::POST, &uri, Some(past)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let rider_uri = format!("/v1/ops/users/{}/map-expert", rider.user_id);
    assert_eq!(t.ops(&admin, Method::POST, &rider_uri, Some(grant.clone())).await.0, StatusCode::NOT_FOUND);
    let (status, e) = t.ops(&admin, Method::POST, &uri, Some(grant)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");
    let (_, detail) = t.ops(&admin, Method::GET, &format!("/v1/ops/users/{}", driver.user_id), None).await;
    assert!(detail["map_expert_until"].is_string(), "{detail}");

    // A Comfort ride is quoted at 16.5 %; her offer and the booked split use 15 %.
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let (status, ride) =
        t.request(&rider, json!({ "quote": q["quote"], "tier": "comfort", "passengers": 1, "payment_method": "cash" }), &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    let fare = ride["fare"]["total"].as_i64().unwrap();
    assert_eq!(ride["fare"]["service_fee"].as_i64().unwrap(), (fare * 1650 + 5000) / 10_000);
    let expert_fee = (fare * 1500 + 5000) / 10_000;
    assert_eq!(t.tick().await.offered, 1);
    let card = t.driver_status(&driver).await["offer"].clone();
    assert_eq!(card["earning"].as_i64().unwrap(), fare - expert_fee);
    let (status, view) = t.post(&format!("/v1/offers/{}/accept", card["offer_id"].as_str().unwrap()), &driver.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["earning"].as_i64().unwrap(), fare - expert_fee);
    let (fee, earning, breakdown): (i64, i64, Value) =
        sqlx::query_as("SELECT service_fee_cents, driver_earning_cents, fare_breakdown FROM rides WHERE id = $1::uuid")
            .bind(ride["id"].as_str().unwrap())
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((fee, earning), (expert_fee, fare - expert_fee));
    assert_eq!((breakdown["service_fee"].as_i64(), breakdown["total"].as_i64()), (Some(expert_fee), Some(fare)));

    // Ended: once, then there is nothing to end. Both changes are audited.
    let reason = json!({ "reason": "Zone mapped" });
    assert_eq!(t.ops(&admin, Method::POST, &format!("{uri}/end"), Some(reason.clone())).await.0, StatusCode::NO_CONTENT);
    let (status, e) = t.ops(&admin, Method::POST, &format!("{uri}/end"), Some(reason)).await;
    assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("not_map_expert")));
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE user_id = $1 AND action LIKE 'driver.map_expert%' ORDER BY id")
            .bind(driver.user_id)
            .fetch_all(&t.state.db)
            .await
            .unwrap();
    assert_eq!(actions, ["driver.map_expert_grant", "driver.map_expert_end"]);
}

#[tokio::test]
async fn the_console_session_is_an_httponly_cookie_and_changes_need_the_csrf_token() {
    let t = setup!(relaxed());
    let (token, csrf, cookie) = t.staff_session("ops_support").await;
    for part in ["HttpOnly", "SameSite=Strict", "Path=/v1/ops", "Max-Age=28800"] {
        assert!(cookie.contains(part), "{part} missing: {cookie}");
    }
    let rider = t.rider().await;
    let uri = format!("/v1/ops/users/{}/priority", rider.user_id);
    let body = json!({ "role": "rider", "delta": 5, "reason": "Goodwill" });

    // Reading with the cookie alone is fine, and a reload gets the roles and CSRF token back.
    let (status, _, session) = t.browser(Method::GET, "/v1/ops/auth/session", Some(&token), None, None).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!((session["csrf_token"].as_str(), session["roles"][0].as_str()), (Some(csrf.as_str()), Some("ops_support")));
    assert_eq!(t.browser(Method::GET, "/v1/ops/reports", Some(&token), None, None).await.0, StatusCode::OK);

    // A change with the cookie needs the matching CSRF token (another page can't read it).
    let (status, _, e) = t.browser(Method::POST, &uri, Some(&token), None, Some(body.clone())).await;
    assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("csrf_failed")));
    let other = t.staff_session("ops_support").await.1;
    assert_eq!(t.browser(Method::POST, &uri, Some(&token), Some(&other), Some(body.clone())).await.0, StatusCode::FORBIDDEN);
    let (status, _, e) = t.browser(Method::POST, &uri, Some(&token), Some(&csrf), Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{e}");
    // Scripts sending the token as a bearer header need no CSRF token.
    assert_eq!(t.ops(&token, Method::POST, &uri, Some(body)).await.0, StatusCode::OK);

    // Sign-out clears the cookie and ends the session.
    let (status, cleared, _) = t.browser(Method::POST, "/v1/ops/auth/logout", Some(&token), Some(&csrf), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cleared = cleared.unwrap();
    assert!(cleared.starts_with("vara_ops=;") && cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(t.browser(Method::GET, "/v1/ops/auth/session", Some(&token), None, None).await.0, StatusCode::UNAUTHORIZED);
}
