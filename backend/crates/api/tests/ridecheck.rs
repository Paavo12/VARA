//! RideCheck tests (PRD S-3) against a real PostGIS database, as a scripted drive: the planned
//! route, a car off it, a car stopped or silent, no question near the destination, the 60 s
//! escalation to the safety desk, "I'm fine", "Call for help" as an SOS, the driver never told,
//! and wrong-user access.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use fuber_worker::ridecheck;
use serde_json::{Value, json};

use common::*;

/// On the straight line from Post Street Mall to Maerua Mall (the estimate's planned route).
const MIDWAY: (f64, f64) = (-22.5751, 17.0875);

impl T {
    /// A Classic trip Post Street Mall → Maerua Mall, started.
    async fn in_trip(&self) -> Trip {
        let trip = self.accepted().await;
        self.drive_to(&trip.driver, POST_ST_MALL).await;
        for (action, body) in [("pin", json!({ "pin": trip.pin })), ("start", json!({}))] {
            let (status, v) = self.trip_action(&trip, action, body).await;
            assert_eq!(status, StatusCode::OK, "{action}: {v}");
        }
        trip
    }

    async fn check(&self) -> ridecheck::Tick {
        ridecheck::tick(&self.ctx).await.unwrap()
    }

    /// A track sample `secs_ago` seconds ago.
    async fn sample(&self, trip: &Trip, p: (f64, f64), secs_ago: f64) {
        sqlx::query(
            "INSERT INTO ride_track_points (ride_id, at, location)
             VALUES ($1::uuid, now() - make_interval(secs => $2), ST_SetSRID(ST_MakePoint($4, $3), 4326)::geography)",
        )
        .bind(&trip.id)
        .bind(secs_ago)
        .bind(p.0)
        .bind(p.1)
        .execute(&self.state.db)
        .await
        .unwrap();
    }

    /// Pretends the trip started `secs` ago, before any of its samples.
    async fn started_ago(&self, trip: &Trip, secs: f64) {
        sqlx::query("UPDATE rides SET started_at = now() - make_interval(secs => $2) WHERE id = $1::uuid")
            .bind(&trip.id)
            .bind(secs)
            .execute(&self.state.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM ride_track_points WHERE ride_id = $1::uuid").bind(&trip.id).execute(&self.state.db).await.unwrap();
    }

    async fn rider_view(&self, trip: &Trip) -> Value {
        let (status, v) = self.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }

    async fn respond(&self, s: &Session, check_id: &str, body: Value) -> (StatusCode, Value) {
        self.post(&format!("/v1/safety/ridecheck/{check_id}/respond"), &s.access, body).await
    }

    /// Makes the open question older than the 60 s she has to answer.
    async fn let_answer_time_pass(&self, trip: &Trip) {
        sqlx::query("UPDATE ride_checks SET asked_at = asked_at - interval '61 seconds' WHERE ride_id = $1::uuid")
            .bind(&trip.id)
            .execute(&self.state.db)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_car_off_the_route_asks_the_rider_and_never_tells_the_driver() {
    let t = setup!(relaxed());
    let trip = t.in_trip().await;

    // The planned route is filled once the trip starts; on it, nobody is asked.
    assert_eq!(t.check().await, ridecheck::Tick { routes_planned: 1, asked: 0, escalated: 0 });
    t.started_ago(&trip, 60.0).await;
    t.sample(&trip, MIDWAY, 15.0).await;
    t.sample(&trip, FAR_AWAY, 5.0).await;
    assert_eq!(t.check().await.asked, 0, "one sample off the route could be GPS noise");

    t.sample(&trip, FAR_AWAY, 0.0).await;
    assert_eq!(t.check().await.asked, 1);
    let view = t.rider_view(&trip).await;
    let check = &view["ride_check"];
    assert_eq!(check["kind"], "deviation");
    assert_eq!(check["escalated"], false);
    assert!(check["answer_by"].is_string());
    // Asked once, not every tick.
    assert_eq!(t.check().await.asked, 0);

    // The driver's app never hears of it.
    let status = t.driver_status(&trip.driver).await;
    assert!(!status.to_string().contains("ride_check"), "{status}");

    // "Yes, I'm fine" closes it, and the same kind isn't asked again for 5 minutes.
    let id = check["id"].as_str().unwrap().to_owned();
    let (code, r) = t.respond(&trip.rider, &id, json!({ "answer": "fine" })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!((r["answer"].as_str(), r["sos"].is_null()), (Some("fine"), true));
    assert!(t.rider_view(&trip).await["ride_check"].is_null());
    t.sample(&trip, FAR_AWAY, 0.0).await;
    assert_eq!(t.check().await.asked, 0);
    // Answering again keeps the first answer.
    let (code, r) = t.respond(&trip.rider, &id, json!({ "answer": "help" })).await;
    assert_eq!((code, r["answer"].as_str()), (StatusCode::OK, Some("fine")));
}

#[tokio::test]
async fn an_unanswered_stop_alerts_the_safety_desk_and_help_raises_an_sos() {
    let t = setup!(relaxed());
    let trip = t.in_trip().await;
    t.started_ago(&trip, 300.0).await;
    for ago in [125.0, 90.0, 60.0, 30.0, 0.0] {
        t.sample(&trip, MIDWAY, ago).await;
    }
    let tick = t.check().await;
    assert_eq!(tick.asked, 1, "{tick:?}");
    assert_eq!(t.rider_view(&trip).await["ride_check"]["kind"], "stop");

    // 60 s without an answer: an incident for the desk, with the car's position and the rider.
    assert_eq!(t.check().await.escalated, 0, "she still has time");
    t.let_answer_time_pass(&trip).await;
    assert_eq!(t.check().await.escalated, 1);
    assert_eq!(t.check().await.escalated, 0, "escalated once");
    let desk = t.staff("ops_safety").await;
    let (_, queue) = t.call(Method::GET, "/v1/ops/incidents", Some(&desk), None).await;
    let incident = &queue[0];
    assert_eq!(incident["kind"], "ridecheck_stop");
    assert!(incident["raised_by"].is_null());
    assert_eq!(incident["rider"]["user_id"], trip.rider.user_id.to_string());
    assert!(incident["ridecheck_answer"].is_null());
    assert!((incident["lat"].as_f64().unwrap() - MIDWAY.0).abs() < 1e-6);

    // She can still answer: the screen says the desk was alerted; "Call for help" is an SOS.
    let check = t.rider_view(&trip).await["ride_check"].clone();
    assert_eq!(check["escalated"], true);
    let (code, r) = t.respond(&trip.rider, check["id"].as_str().unwrap(), json!({ "answer": "help", "location": at(MIDWAY) })).await;
    assert_eq!(code, StatusCode::OK, "{r}");
    assert_eq!(r["sos"]["ride_id"], trip.id.as_str());
    assert_eq!(r["sos"]["police_phone"], "10111");
    let (_, queue) = t.call(Method::GET, "/v1/ops/incidents", Some(&desk), None).await;
    let kinds: Vec<&str> = queue.as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"sos") && kinds.contains(&"ridecheck_stop"), "{kinds:?}");
    let rc = queue.as_array().unwrap().iter().find(|i| i["kind"] == "ridecheck_stop").unwrap();
    assert_eq!(rc["ridecheck_answer"], "help");
    assert!(t.rider_view(&trip).await["ride_check"].is_null());
}

#[tokio::test]
async fn a_silent_car_is_asked_about_but_a_stop_at_the_destination_is_not() {
    let t = setup!(relaxed());
    let silent = t.in_trip().await;
    t.started_ago(&silent, 180.0).await;
    t.sample(&silent, MIDWAY, 150.0).await;
    assert_eq!(t.check().await.asked, 1, "no position for 2 minutes");
    assert_eq!(t.rider_view(&silent).await["ride_check"]["kind"], "stop");

    let t = setup!(relaxed());
    let arriving = t.in_trip().await;
    t.started_ago(&arriving, 600.0).await;
    for ago in [125.0, 60.0, 0.0] {
        t.sample(&arriving, MAERUA_MALL, ago).await;
    }
    assert_eq!(t.check().await.asked, 0, "waiting at the destination is the drop-off");
    // An SOS answers a waiting question too.
    sqlx::query("INSERT INTO ride_checks (ride_id, kind) VALUES ($1::uuid, 'stop')").bind(&arriving.id).execute(&t.state.db).await.unwrap();
    assert_eq!(t.post("/v1/safety/sos", &arriving.rider.access, json!({})).await.0, StatusCode::OK);
    assert!(t.rider_view(&arriving).await["ride_check"].is_null());
}

#[tokio::test]
async fn only_her_rider_can_answer() {
    let t = setup!(relaxed());
    let trip = t.in_trip().await;
    t.started_ago(&trip, 180.0).await;
    assert_eq!(t.check().await.asked, 1);
    let id = t.rider_view(&trip).await["ride_check"]["id"].as_str().unwrap().to_owned();

    let stranger = t.rider().await;
    assert_eq!(t.respond(&stranger, &id, json!({ "answer": "fine" })).await.0, StatusCode::NOT_FOUND);
    assert_eq!(t.respond(&trip.driver, &id, json!({ "answer": "fine" })).await.0, StatusCode::FORBIDDEN);
    let uri = format!("/v1/safety/ridecheck/{id}/respond");
    assert_eq!(t.call(Method::POST, &uri, None, Some(json!({ "answer": "fine" }))).await.0, StatusCode::UNAUTHORIZED);
    for bad in [json!({ "answer": "maybe" }), json!({}), json!({ "answer": "fine", "extra": 1 })] {
        assert_eq!(t.respond(&trip.rider, &id, bad.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    // Still open after all that.
    assert!(t.rider_view(&trip).await["ride_check"].is_object());
}
