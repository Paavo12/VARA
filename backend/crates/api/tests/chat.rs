//! Phase 3 chat tests against a real PostGIS database: rider and driver talking, quick replies,
//! when the chat is open, resends, a new driver starting clean, the safety desk's audited view,
//! retention, and wrong-user access. Live delivery is tested with the sockets in `dispatch.rs`.
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
    async fn send_with_key(&self, ride: &str, s: &Session, body: Value, idempotency_key: &str) -> (StatusCode, Value) {
        self.call_with(
            Method::POST,
            &format!("/v1/rides/{ride}/messages"),
            Some(&s.access),
            Some(body),
            &[("idempotency-key", idempotency_key)],
        )
        .await
    }

    async fn send(&self, ride: &str, s: &Session, body: Value) -> (StatusCode, Value) {
        self.send_with_key(ride, s, body, &key()).await
    }

    async fn say(&self, ride: &str, s: &Session, text: &str) -> Value {
        let (status, m) = self.send(ride, s, json!({ "text": text })).await;
        assert_eq!(status, StatusCode::OK, "{m}");
        m
    }

    async fn thread(&self, ride: &str, s: &Session) -> Value {
        let (status, thread) = self.get(&format!("/v1/rides/{ride}/messages"), &s.access).await;
        assert_eq!(status, StatusCode::OK, "{thread}");
        thread
    }

    async fn incident_on(&self, ride: &str, s: &Session) -> String {
        let (status, alert) = self.post("/v1/safety/sos", &s.access, json!({ "ride_id": ride })).await;
        assert_eq!(status, StatusCode::OK, "{alert}");
        alert["incident_id"].as_str().unwrap().to_owned()
    }
}

fn closed(r: (StatusCode, Value)) {
    assert_eq!((r.0, r.1["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("chat_closed")), "{}", r.1);
}

fn texts(thread: &Value) -> Vec<&str> {
    thread["messages"].as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn rider_and_driver_talk_without_seeing_each_others_numbers() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;

    let thread = t.thread(&trip.id, &trip.rider).await;
    assert_eq!(thread["messages"], json!([]));
    assert_eq!(thread["can_send"], true);

    let first = t.say(&trip.id, &trip.rider, "  I'm at the\nside door  ").await;
    assert_eq!(first["sender"], "rider");
    assert_eq!(first["text"], "I'm at the\nside door", "trimmed, line breaks kept");
    t.say(&trip.id, &trip.driver, "Two minutes").await;
    let phones: Vec<String> = sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = ANY($1)")
        .bind(vec![trip.rider.user_id, trip.driver.user_id])
        .fetch_all(&t.state.db)
        .await
        .unwrap();
    assert_eq!(phones.len(), 2);

    // Both see the same conversation, oldest first.
    for s in [&trip.rider, &trip.driver] {
        let thread = t.thread(&trip.id, s).await;
        assert_eq!(texts(&thread), ["I'm at the\nside door", "Two minutes"]);
        assert_eq!(thread["messages"][1]["sender"], "driver");
        let body = thread.to_string();
        for phone in &phones {
            assert!(!body.contains(phone.as_str()), "{body}");
        }
    }
}

#[tokio::test]
async fn quick_replies_use_the_servers_wording_and_belong_to_one_side() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;

    let offered = t.thread(&trip.id, &trip.driver).await["quick_replies"].clone();
    assert!(offered.as_array().unwrap().iter().any(|q| q["id"] == "arrived"), "{offered}");
    let offered = t.thread(&trip.id, &trip.rider).await["quick_replies"].clone();
    assert!(offered.as_array().unwrap().iter().any(|q| q["id"] == "at_entrance"), "{offered}");

    let (status, m) = t.send(&trip.id, &trip.rider, json!({ "quick_reply": "at_entrance" })).await;
    assert_eq!(status, StatusCode::OK, "{m}");
    assert_eq!((m["text"].as_str(), m["quick_reply"].as_str()), (Some("I'm by the entrance"), Some("at_entrance")));

    // A rider can't send the driver's "I've arrived".
    let (status, e) = t.send(&trip.id, &trip.rider, json!({ "quick_reply": "arrived" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
}

#[tokio::test]
async fn messages_are_validated() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let long = "x".repeat(501);
    for body in [
        json!({}),
        json!({ "text": "" }),
        json!({ "text": "   " }),
        json!({ "text": long }),
        json!({ "text": "a\u{0007}b" }),
        json!({ "quick_reply": "nope" }),
        json!({ "text": "hello", "quick_reply": "at_entrance" }),
        json!({ "text": "hello", "extra": 1 }),
    ] {
        assert_eq!(t.send(&trip.id, &trip.rider, body.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    t.say(&trip.id, &trip.rider, &"x".repeat(500)).await;

    // Stored and returned as plain text, never altered into or out of markup.
    let m = t.say(&trip.id, &trip.rider, "<script>alert(1)</script>").await;
    assert_eq!(m["text"], "<script>alert(1)</script>");

    let (status, e) =
        t.call(Method::POST, &format!("/v1/rides/{}/messages", trip.id), Some(&trip.rider.access), Some(json!({ "text": "hi" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "an Idempotency-Key is required: {e}");
}

#[tokio::test]
async fn a_resend_after_a_lost_response_is_the_same_message() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let k = key();
    let (_, first) = t.send_with_key(&trip.id, &trip.rider, json!({ "text": "Hello" }), &k).await;
    let (status, again) = t.send_with_key(&trip.id, &trip.rider, json!({ "text": "Hello" }), &k).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(first["id"], again["id"]);
    assert_eq!(texts(&t.thread(&trip.id, &trip.driver).await), ["Hello"]);

    // Keys are per side, even if the two phones happened to pick the same one.
    let (status, m) = t.send_with_key(&trip.id, &trip.driver, json!({ "text": "Hi" }), &k).await;
    assert_eq!((status, m["text"].as_str()), (StatusCode::OK, Some("Hi")));
}

#[tokio::test]
async fn the_chat_is_open_only_while_a_driver_is_on_the_trip() {
    let t = setup!(relaxed());

    // Before a driver accepts there is nobody to talk to.
    let rider = t.rider().await;
    let ride = t.book(&rider).await;
    let ride_id = ride["id"].as_str().unwrap();
    closed(t.send(ride_id, &rider, json!({ "text": "Anyone?" })).await);
    assert_eq!(t.thread(ride_id, &rider).await["can_send"], false);
    assert_eq!(t.post(&format!("/v1/rides/{ride_id}/cancel"), &rider.access, json!({})).await.0, StatusCode::OK);

    // After the trip it stays readable but closes for both.
    let trip = t.accepted().await;
    t.say(&trip.id, &trip.rider, "On my way down").await;
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    t.say(&trip.id, &trip.driver, "Outside").await;
    for (action, body) in [("pin", json!({ "pin": trip.pin })), ("start", json!({}))] {
        assert_eq!(t.trip_action(&trip, action, body).await.0, StatusCode::OK);
        t.say(&trip.id, &trip.rider, action).await;
    }
    assert_eq!(t.trip_action(&trip, "complete", json!({})).await.0, StatusCode::OK);
    closed(t.send(&trip.id, &trip.rider, json!({ "text": "Left my bag" })).await);
    closed(t.send(&trip.id, &trip.driver, json!({ "text": "Bye" })).await);
    let thread = t.thread(&trip.id, &trip.rider).await;
    assert_eq!(thread["can_send"], false);
    assert_eq!(texts(&thread).len(), 4);
}

#[tokio::test]
async fn whoever_got_the_pin_wrong_five_times_cannot_message_the_passenger() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    let bad = if trip.pin == "0000" { "1111" } else { "0000" };
    for _ in 0..5 {
        t.trip_action(&trip, "pin", json!({ "pin": bad })).await;
    }
    assert_eq!(t.ride_state(&trip.id).await, "BLOCKED");
    closed(t.send(&trip.id, &trip.driver, json!({ "text": "Please give me the PIN" })).await);
    closed(t.send(&trip.id, &trip.rider, json!({ "text": "Hello?" })).await);
}

#[tokio::test]
async fn a_new_driver_starts_clean_and_the_old_one_is_out() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.say(&trip.id, &trip.rider, "The gate code is 4411").await;
    let (status, e) = t.trip_action(&trip, "driver-cancel", json!({ "reason": "vehicle_problem" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");

    // Between drivers: the rider has nobody to talk to, the old driver can no longer read.
    assert_eq!(t.thread(&trip.id, &trip.rider).await["messages"], json!([]));
    let uri = format!("/v1/rides/{}/messages", trip.id);
    assert_eq!(t.get(&uri, &trip.driver.access).await.0, StatusCode::NOT_FOUND);
    assert_eq!(t.send(&trip.id, &trip.driver, json!({ "text": "Sorry" })).await.0, StatusCode::NOT_FOUND);

    let next = t.online_driver("Selma Iipinge", "classic", NEAR).await;
    assert_eq!(t.tick().await.offered, 1);
    let offer = t.driver_status(&next).await["offer"]["offer_id"].as_str().unwrap().to_owned();
    assert_eq!(t.post(&format!("/v1/offers/{offer}/accept"), &next.access, json!({})).await.0, StatusCode::OK);
    assert_eq!(t.thread(&trip.id, &next).await["messages"], json!([]), "the next driver doesn't get the gate code");
    t.say(&trip.id, &next, "On my way").await;
    assert_eq!(texts(&t.thread(&trip.id, &trip.rider).await), ["On my way"]);

    // The safety desk sees all of it, and can tell the two conversations apart.
    let incident = t.incident_on(&trip.id, &trip.rider).await;
    let desk = t.staff("ops_safety").await;
    let (status, all) = t.call(Method::GET, &format!("/v1/ops/incidents/{incident}/messages"), Some(&desk), None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all[0]["text"], "The gate code is 4411");
    assert_eq!(all[1]["text"], "On my way");
    assert_eq!(all[0]["driver_id"], trip.driver.user_id.to_string());
    assert_eq!(all[1]["driver_id"], next.user_id.to_string());
}

#[tokio::test]
async fn only_safety_staff_read_a_chat_and_every_read_is_logged() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.say(&trip.id, &trip.rider, "Hello").await;
    let incident = t.incident_on(&trip.id, &trip.rider).await;
    let uri = format!("/v1/ops/incidents/{incident}/messages");
    let views = || {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_log WHERE action = 'chat.view' AND target_id = $1")
            .bind(&incident)
            .fetch_one(&t.state.db)
    };

    let reviewer = t.staff("ops_reviewer").await;
    assert_eq!(t.call(Method::GET, &uri, Some(&reviewer), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call(Method::GET, &uri, Some(&trip.rider.access), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(views().await.unwrap(), 0);

    let desk = t.staff("ops_safety").await;
    assert_eq!(t.call(Method::GET, &uri, Some(&desk), None).await.0, StatusCode::OK);
    assert_eq!(views().await.unwrap(), 1);
    let missing = format!("/v1/ops/incidents/{}/messages", uuid::Uuid::new_v4());
    assert_eq!(t.call(Method::GET, &missing, Some(&desk), None).await.0, StatusCode::NOT_FOUND);

    // An incident with no trip has no chat.
    let (status, alert) = t.post("/v1/safety/sos", &t.online_driver("Hilma Nangolo", "classic", NEAR).await.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    let uri = format!("/v1/ops/incidents/{}/messages", alert["incident_id"].as_str().unwrap());
    let (status, none) = t.call(Method::GET, &uri, Some(&desk), None).await;
    assert_eq!((status, none), (StatusCode::OK, json!([])));
}

#[tokio::test]
async fn anyone_else_gets_404_and_no_token_gets_401() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    t.say(&trip.id, &trip.rider, "Private").await;
    let uri = format!("/v1/rides/{}/messages", trip.id);
    let other_rider = t.rider().await;
    let other_driver = t.online_driver("Selma Iipinge", "classic", FURTHER).await;
    for s in [&other_rider, &other_driver] {
        let (status, body) = t.get(&uri, &s.access).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(t.send(&trip.id, s, json!({ "text": "Hi" })).await.0, StatusCode::NOT_FOUND);
    }
    let missing = format!("/v1/rides/{}/messages", uuid::Uuid::new_v4());
    assert_eq!(t.get(&missing, &trip.rider.access).await.0, StatusCode::NOT_FOUND);
    assert_eq!(t.call(Method::GET, &uri, None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(texts(&t.thread(&trip.id, &trip.driver).await), ["Private"]);
}

#[tokio::test]
async fn sending_is_rate_limited_per_person() {
    let t = setup!(RateLimits { chat: Rule::per_minute(2), ..relaxed() });
    let trip = t.accepted().await;
    t.say(&trip.id, &trip.rider, "One").await;
    t.say(&trip.id, &trip.rider, "Two").await;
    assert_eq!(t.send(&trip.id, &trip.rider, json!({ "text": "Three" })).await.0, StatusCode::TOO_MANY_REQUESTS);
    // Each person has a separate allowance, and reading isn't limited by it.
    t.say(&trip.id, &trip.driver, "Hello").await;
    assert_eq!(texts(&t.thread(&trip.id, &trip.rider).await).len(), 3);
}

#[tokio::test]
async fn old_chats_and_tracks_are_deleted_after_90_days_unless_an_incident_holds_them() {
    let t = setup!(relaxed());
    // Completed last: a finished driver would otherwise be offered the next ride.
    let held = t.accepted().await;
    let recent = t.accepted().await;
    let plain = t.completed().await;
    for ride in [&plain, &held] {
        sqlx::query(
            "INSERT INTO ride_messages (ride_id, driver_id, sender, body, client_key, sent_at)
             SELECT id, driver_id, 'rider', 'old', 'old-message', now() - interval '91 days' FROM rides WHERE id = $1::uuid",
        )
        .bind(&ride.id)
        .execute(&t.state.db)
        .await
        .unwrap();
    }
    for (ride, age) in [(&plain, "91 days"), (&held, "91 days"), (&recent, "1 day")] {
        sqlx::query(
            "INSERT INTO ride_track_points (ride_id, at, location)
             SELECT id, now() - $2::interval, pickup FROM rides WHERE id = $1::uuid",
        )
        .bind(&ride.id)
        .bind(age)
        .execute(&t.state.db)
        .await
        .unwrap();
    }
    t.say(&recent.id, &recent.rider, "New").await;
    t.incident_on(&held.id, &held.rider).await;

    fuber_worker::cleanup(&t.ctx).await.unwrap();

    assert_eq!(t.thread(&plain.id, &plain.rider).await["messages"], json!([]));
    assert_eq!(texts(&t.thread(&held.id, &held.rider).await), ["old"]);
    assert_eq!(texts(&t.thread(&recent.id, &recent.rider).await), ["New"]);
    for (ride, left) in [(&plain, 0), (&held, 1), (&recent, 1)] {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM ride_track_points WHERE ride_id = $1::uuid")
            .bind(&ride.id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
        assert_eq!(n, left, "track points left on {}", ride.id);
    }
}

#[tokio::test]
async fn erasing_an_account_deletes_its_chats_unless_an_incident_holds_them() {
    let t = setup!(relaxed());
    let plain = t.accepted().await;
    let held = t.accepted().await;
    t.say(&plain.id, &plain.driver, "Delete me").await;
    t.say(&held.id, &held.driver, "Keep me").await;
    t.incident_on(&held.id, &held.rider).await;

    for rider in [&plain.rider, &held.rider] {
        sqlx::query("UPDATE users SET status = 'deleted', deleted_at = now() - interval '31 days' WHERE id = $1")
            .bind(rider.user_id)
            .execute(&t.state.db)
            .await
            .unwrap();
        fuber_worker::erase_account(&t.ctx, rider.user_id).await.unwrap();
    }

    let left: Vec<String> = sqlx::query_scalar("SELECT body FROM ride_messages ORDER BY id").fetch_all(&t.state.db).await.unwrap();
    assert_eq!(left, ["Keep me"]);
}
