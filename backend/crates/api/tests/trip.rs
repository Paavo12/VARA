//! Phase 3 trip tests against a real PostGIS database: arriving within 50 m, the PIN and its
//! lockout, the free wait and no-show, start and complete, the driver cancelling, the trip
//! track, and wrong-user access to every trip endpoint.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::json;

use common::*;

impl T {
    /// Arrives at the pickup and enters five wrong PINs.
    async fn lock_out(&self, trip: &Trip) {
        sqlx::query("UPDATE driver_presence SET location = ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography WHERE driver_id = $3")
            .bind(POST_ST_MALL.0 + 0.0003)
            .bind(POST_ST_MALL.1)
            .bind(trip.driver.user_id)
            .execute(&self.state.db)
            .await
            .unwrap();
        assert_eq!(self.trip_action(trip, "arrive", json!({})).await.0, StatusCode::OK);
        let bad = wrong(&trip.pin);
        for _ in 0..4 {
            assert_eq!(self.trip_action(trip, "pin", json!({ "pin": bad })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        }
        let (status, e) = self.trip_action(trip, "pin", json!({ "pin": bad })).await;
        assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("pin_locked")), "{e}");
    }

    /// Whether matching would offer her a request at the pickup.
    async fn matchable(&self, d: &Session) -> bool {
        let found = fuber_db::presence::candidates(
            &self.state.db,
            &fuber_db::presence::Search {
                lat: POST_ST_MALL.0,
                lon: POST_ST_MALL.1,
                tier: "classic",
                wheelchair: false,
                radius_m: 3000.0,
                stale_s: 3600,
                exclude: &[],
                rider_id: None,
                limit: 10,
            },
        )
        .await
        .unwrap();
        found.iter().any(|c| c.driver_id == d.user_id)
    }
}

fn wrong(pin: &str) -> String {
    format!("{:04}", (pin.parse::<u32>().unwrap() + 1) % 10_000)
}

#[tokio::test]
async fn a_full_trip_from_pickup_to_destination() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;

    // 600 m away: not arrived, and the arrive button says so.
    let (status, e) = t.trip_action(&trip, "arrive", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"]["code"], "not_at_pickup");
    // The PIN and start need arrival first.
    assert_eq!(t.trip_action(&trip, "pin", json!({ "pin": trip.pin })).await.0, StatusCode::CONFLICT);
    assert_eq!(t.trip_action(&trip, "start", json!({})).await.0, StatusCode::CONFLICT);

    // Within 50 m of the pickup her position update marks her arrived: the exact pickup
    // unlocks, the destination does not.
    t.drive_to(&trip.driver, (POST_ST_MALL.0 + 0.0002, POST_ST_MALL.1)).await;
    assert_eq!(t.ride_state(&trip.id).await, "DRIVER_ARRIVED");
    let ride = &t.driver_status(&trip.driver).await["ride"];
    assert_eq!(ride["pickup"]["address"], "Post Street Mall");
    assert!(ride["dropoff"].is_null(), "{ride}");
    assert_eq!(ride["pin_attempts_left"], 5);
    assert!(!ride.to_string().contains("Maerua"));
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert!(rider_view["arrived_at"].is_string(), "{rider_view}");
    assert!(rider_view["trip_duration_s"].as_i64().unwrap() > 0);

    // A wrong PIN is refused and counted, and still reveals nothing.
    let (status, e) = t.trip_action(&trip, "pin", json!({ "pin": wrong(&trip.pin) })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(e["error"]["code"], "wrong_pin");
    assert!(e["error"]["message"].as_str().unwrap().contains("4 attempts left"), "{e}");
    assert!(!e.to_string().contains("Maerua"));
    assert_eq!(t.driver_status(&trip.driver).await["ride"]["pin_attempts_left"], 4);

    // The right PIN unlocks the destination.
    let (status, v) = t.trip_action(&trip, "pin", json!({ "pin": trip.pin })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "PIN_VERIFIED");
    assert_eq!(v["dropoff"]["address"], "Maerua Mall");
    assert!(!v.to_string().contains("\"pin\""), "the driver never receives the PIN: {v}");

    let (status, v) = t.trip_action(&trip, "start", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "IN_TRIP");
    assert!(v["started_at"].is_string());
    // A double tap doesn't start it twice.
    assert_eq!(t.trip_action(&trip, "start", json!({})).await.0, StatusCode::CONFLICT);
    // Riders can't cancel once in the car.
    assert_eq!(t.post(&format!("/v1/rides/{}/cancel", trip.id), &trip.rider.access, json!({})).await.0, StatusCode::CONFLICT);

    // Positions during the trip are kept as a track, at most one point per 10 s.
    t.drive_to(&trip.driver, ON_THE_WAY).await;
    t.drive_to(&trip.driver, (ON_THE_WAY.0 - 0.001, ON_THE_WAY.1)).await;
    let points: i64 = sqlx::query_scalar("SELECT count(*) FROM ride_track_points WHERE ride_id = $1::uuid")
        .bind(&trip.id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(points, 1);

    t.drive_to(&trip.driver, MAERUA_MALL).await;
    let (status, v) = t.trip_action(&trip, "complete", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "COMPLETED");
    let ended: f64 = sqlx::query_scalar(
        "SELECT (detail->>'from_dropoff_m')::float8 FROM ride_events WHERE ride_id = $1::uuid AND to_state = 'COMPLETED'",
    )
    .bind(&trip.id)
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    assert!(ended < 1.0, "{ended}");
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert_eq!(rider_view["state"], "COMPLETED");
    assert!(rider_view["completed_at"].is_string());
    // A completed trip earns both of them priority points; she is free for the next request.
    assert_eq!(t.points("rider_profiles", &trip.rider).await, 102);
    assert_eq!(t.points("driver_profiles", &trip.driver).await, 102);
    assert!(t.driver_status(&trip.driver).await["ride"].is_null());
    assert_eq!(t.trip_action(&trip, "complete", json!({})).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn five_wrong_pins_block_the_trip_and_alert_the_safety_desk() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    // Her position reached the pickup (e.g. a late update) and she taps "I've arrived".
    sqlx::query("UPDATE driver_presence SET location = ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography WHERE driver_id = $3")
        .bind(POST_ST_MALL.0 + 0.0003)
        .bind(POST_ST_MALL.1)
        .bind(trip.driver.user_id)
        .execute(&t.state.db)
        .await
        .unwrap();
    let (status, v) = t.trip_action(&trip, "arrive", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "DRIVER_ARRIVED");
    assert_eq!(t.trip_action(&trip, "arrive", json!({})).await.0, StatusCode::CONFLICT, "only once");

    let bad = wrong(&trip.pin);
    for left in [4, 3, 2, 1] {
        let (status, e) = t.trip_action(&trip, "pin", json!({ "pin": bad })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
        let expected = if left == 1 { "1 attempt left".to_owned() } else { format!("{left} attempts left") };
        assert!(e["error"]["message"].as_str().unwrap().contains(&expected), "{e}");
    }
    let (status, e) = t.trip_action(&trip, "pin", json!({ "pin": bad })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"]["code"], "pin_locked");
    assert_eq!(t.ride_state(&trip.id).await, "BLOCKED");

    // The safety desk has an open incident with where it happened.
    let (kind, status, has_location): (String, String, bool) =
        sqlx::query_as("SELECT kind::text, status::text, location IS NOT NULL FROM safety_incidents WHERE ride_id = $1::uuid")
            .bind(&trip.id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((kind.as_str(), status.as_str(), has_location), ("pin_lockout", "open", true));

    // Even the right PIN is refused now, and the destination never appeared.
    let (status, e) = t.trip_action(&trip, "pin", json!({ "pin": trip.pin })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(e["error"]["code"], "invalid_state");
    assert_eq!(t.trip_action(&trip, "start", json!({})).await.0, StatusCode::CONFLICT);
    let attempts: i16 =
        sqlx::query_scalar("SELECT pin_attempts FROM rides WHERE id = $1::uuid").bind(&trip.id).fetch_one(&t.state.db).await.unwrap();
    assert_eq!(attempts, 5);
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert_eq!(rider_view["state"], "BLOCKED");
}

#[tokio::test]
async fn after_a_lockout_she_gets_no_requests_until_the_safety_desk_closes_it() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let points = t.points("driver_profiles", &trip.driver).await;
    t.lock_out(&trip).await;
    assert_eq!(t.points("driver_profiles", &trip.driver).await, points, "a lockout costs her no points");
    let db = &t.state.db;
    let incident: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM safety_incidents WHERE ride_id = $1::uuid").bind(&trip.id).fetch_one(db).await.unwrap();

    // Held, and the app says why; still online.
    let status = t.driver_status(&trip.driver).await;
    assert!(status["online"].as_bool().unwrap());
    assert!(status["hold"]["message"].is_string(), "{status}");
    assert!(!t.matchable(&trip.driver).await);

    // Time alone never releases her.
    sqlx::query("UPDATE safety_incidents SET created_at = now() - interval '90 days' WHERE id = $1")
        .bind(incident)
        .execute(db)
        .await
        .unwrap();
    assert!(t.driver_status(&trip.driver).await["hold"].is_object());
    assert!(!t.matchable(&trip.driver).await);

    // The safety desk sees who is held. Only safety staff may close, and it needs an outcome.
    let desk = t.staff("ops_safety").await;
    let (_, queue) = t.call(Method::GET, "/v1/ops/incidents", Some(&desk), None).await;
    let item = queue.as_array().unwrap().iter().find(|i| i["id"] == incident.to_string()).unwrap();
    assert_eq!(item["driver"]["user_id"], trip.driver.user_id.to_string());
    let close = format!("/v1/ops/incidents/{incident}/close");
    let outcome = json!({ "outcome": "Spoke to both; the rider's PIN was mistyped." });
    let reviewer = t.staff("ops_reviewer").await;
    assert_eq!(t.call(Method::POST, &close, Some(&reviewer), Some(outcome.clone())).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call(Method::POST, &close, None, Some(outcome.clone())).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.call(Method::POST, &close, Some(&desk), Some(json!({}))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!t.matchable(&trip.driver).await, "refused closes change nothing");

    // Closing releases her, ends the blocked trip and is audited.
    assert_eq!(t.call(Method::POST, &close, Some(&desk), Some(outcome.clone())).await.0, StatusCode::NO_CONTENT);
    assert!(t.driver_status(&trip.driver).await["hold"].is_null());
    assert!(t.matchable(&trip.driver).await);
    assert_eq!(t.ride_state(&trip.id).await, "CANCELLED");
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'incident.close' AND target_id = $1")
        .bind(incident.to_string())
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(audited, 1);
    assert_eq!(t.call(Method::POST, &close, Some(&desk), Some(outcome)).await.0, StatusCode::NOT_FOUND, "already closed");
}

#[tokio::test]
async fn after_the_free_wait_the_driver_may_cancel_as_a_rider_no_show() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    assert_eq!(t.ride_state(&trip.id).await, "DRIVER_ARRIVED");

    // Not before she has waited.
    let (status, e) = t.trip_action(&trip, "no-show", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(e["error"]["code"], "wait_not_over");
    let (status, v) = t.trip_action(&trip, "wait", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let started = v["wait_started_at"].as_str().unwrap().to_owned();
    assert!(v["no_show_from"].is_string());
    // Starting it again doesn't restart the clock.
    let (_, again) = t.trip_action(&trip, "wait", json!({})).await;
    assert_eq!(again["wait_started_at"], started.as_str());
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert_eq!(rider_view["wait_started_at"], started.as_str());
    assert_eq!(t.trip_action(&trip, "no-show", json!({})).await.0, StatusCode::CONFLICT);

    // Five minutes later.
    sqlx::query("UPDATE rides SET wait_started_at = wait_started_at - interval '301 seconds' WHERE id = $1::uuid")
        .bind(&trip.id)
        .execute(&t.state.db)
        .await
        .unwrap();
    let (status, v) = t.trip_action(&trip, "no-show", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "CANCELLED");
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert_eq!(rider_view["cancelled_by"], "driver");
    // It counts against the rider, not the driver.
    assert_eq!(t.points("rider_profiles", &trip.rider).await, 95);
    assert_eq!(t.points("driver_profiles", &trip.driver).await, 100);
    let reason: String = sqlx::query_scalar("SELECT reason FROM priority_ledger WHERE user_id = $1")
        .bind(trip.rider.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(reason, "no_show");
}

#[tokio::test]
async fn a_driver_who_cancels_sends_the_request_to_someone_else() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let other = t.online_driver("Selma Iipinge", "classic", FURTHER).await;

    let (status, e) = t.trip_action(&trip, "driver-cancel", json!({ "reason": "bored" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(t.trip_action(&trip, "driver-cancel", json!({})).await.0, StatusCode::UNPROCESSABLE_ENTITY, "a reason is required");
    let (status, e) = t.trip_action(&trip, "driver-cancel", json!({ "reason": "vehicle_problem" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");

    // Searching again, with no driver attached; it cost her priority, not the rider.
    assert_eq!(t.ride_state(&trip.id).await, "SEARCHING");
    let (_, rider_view) = t.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
    assert!(rider_view["driver"].is_null(), "{rider_view}");
    assert_eq!(rider_view["pin"], trip.pin.as_str());
    assert_eq!(t.points("driver_profiles", &trip.driver).await, 95);
    assert_eq!(t.points("rider_profiles", &trip.rider).await, 100);
    let reason: String = sqlx::query_scalar(
        "SELECT detail->>'reason' FROM ride_events WHERE ride_id = $1::uuid AND from_state = 'DRIVER_EN_ROUTE' AND to_state = 'SEARCHING'",
    )
    .bind(&trip.id)
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    assert_eq!(reason, "vehicle_problem");

    // The next offer goes to someone else, even though she is nearer; the ride is gone for her.
    assert_eq!(t.tick().await.offered, 1);
    assert_eq!(t.driver_status(&other).await["offer"]["ride_id"], trip.id.as_str());
    let hers = t.driver_status(&trip.driver).await;
    assert!(hers["ride"].is_null() && hers["offer"].is_null(), "{hers}");
    assert_eq!(t.trip_action(&trip, "arrive", json!({})).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn no_driver_cancel_after_the_pin() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    assert_eq!(t.trip_action(&trip, "pin", json!({ "pin": trip.pin })).await.0, StatusCode::OK);
    let (status, e) = t.trip_action(&trip, "driver-cancel", json!({ "reason": "other" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(t.trip_action(&trip, "no-show", json!({})).await.0, StatusCode::CONFLICT);
    assert_eq!(t.trip_action(&trip, "wait", json!({})).await.0, StatusCode::CONFLICT);
    assert_eq!(t.ride_state(&trip.id).await, "PIN_VERIFIED");
}

#[tokio::test]
async fn wrong_user_and_wrong_app_access_is_refused_on_every_trip_endpoint() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    let stranger = t.online_driver("Martha Kandjii", "classic", FAR_AWAY).await;
    let endpoints = [
        ("arrive", json!({})),
        ("pin", json!({ "pin": trip.pin })),
        ("wait", json!({})),
        ("no-show", json!({})),
        ("start", json!({})),
        ("complete", json!({})),
        ("driver-cancel", json!({ "reason": "other" })),
    ];
    for (action, body) in &endpoints {
        let uri = format!("/v1/rides/{}/{action}", trip.id);
        // Another driver: indistinguishable from a missing ride.
        assert_eq!(t.post(&uri, &stranger.access, body.clone()).await.0, StatusCode::NOT_FOUND, "{action}");
        // The rider app can't drive.
        let (status, e) = t.post(&uri, &trip.rider.access, body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{action}");
        assert_eq!(e["error"]["code"], "wrong_app");
        assert_eq!(t.call(Method::POST, &uri, None, Some(body.clone())).await.0, StatusCode::UNAUTHORIZED, "{action}");
        let missing = format!("/v1/rides/{}/{action}", uuid::Uuid::new_v4());
        assert_eq!(t.post(&missing, &trip.driver.access, body.clone()).await.0, StatusCode::NOT_FOUND, "{action}");
    }
    // Nothing they tried changed the ride or used up a PIN attempt.
    assert_eq!(t.ride_state(&trip.id).await, "DRIVER_ARRIVED");
    assert_eq!(t.driver_status(&trip.driver).await["ride"]["pin_attempts_left"], 5);

    // Bodies are validated.
    for bad in [json!({ "pin": "12a4" }), json!({ "pin": "123" }), json!({ "pin": 1234 }), json!({ "pin": "1234", "x": 1 })] {
        let (status, e) = t.trip_action(&trip, "pin", bad.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {e}");
        assert_eq!(e["error"]["code"], "invalid_request", "{bad}");
    }
    assert_eq!(t.driver_status(&trip.driver).await["ride"]["pin_attempts_left"], 5);
}
