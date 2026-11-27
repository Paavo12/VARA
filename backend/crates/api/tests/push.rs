//! Push notifications (App Flow §10): each app registers its device token on the session; ride
//! changes queue the rider's notices with the change; the worker sends them, forgets dead
//! tokens, and texts her instead when "driver assigned" or "arrived" reaches no device.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::json;
use uuid::Uuid;

use common::*;

async fn register(t: &T, s: &Session, token: &str) -> StatusCode {
    t.call(Method::PUT, "/v1/me/push-token", Some(&s.access), Some(json!({ "token": token, "platform": "fcm" }))).await.0
}

/// The notices pushed to `token`, oldest first.
fn pushed(t: &T, token: &str) -> Vec<String> {
    t.ctx
        .push
        .outbox()
        .into_iter()
        .filter(|p| p.token == token)
        .map(|p| p.message.data.iter().find(|(k, _)| *k == "kind").unwrap().1.clone())
        .collect()
}

async fn token_of(t: &T, user: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT push_token FROM sessions WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user)
        .fetch_one(&t.state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_device_belongs_to_one_session_and_bad_tokens_are_refused() {
    let t = setup!(relaxed());
    let a = t.rider().await;
    let b = t.rider().await;
    assert_eq!(register(&t, &a, "device-1").await, StatusCode::NO_CONTENT);
    assert_eq!(token_of(&t, a.user_id).await.as_deref(), Some("device-1"));

    // B signs in on the same phone: A's notices stop going to it.
    assert_eq!(register(&t, &b, "device-1").await, StatusCode::NO_CONTENT);
    assert_eq!(token_of(&t, a.user_id).await, None);
    assert_eq!(token_of(&t, b.user_id).await.as_deref(), Some("device-1"));

    let (status, _) =
        t.call(Method::PUT, "/v1/me/push-token", Some(&b.access), Some(json!({ "token": "device-1", "platform": "apns" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for bad in ["", "has space", &"x".repeat(4097)] {
        let (status, _) = t.call(Method::PUT, "/v1/me/push-token", Some(&b.access), Some(json!({ "token": bad, "platform": "fcm" }))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad:?}");
    }
    let (status, _) = t.call(Method::PUT, "/v1/me/push-token", None, Some(json!({ "token": "device-1", "platform": "fcm" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = t.call(Method::DELETE, "/v1/me/push-token", Some(&b.access), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(token_of(&t, b.user_id).await, None);
}

#[tokio::test]
async fn the_rider_hears_about_her_trip_and_lock_screens_show_no_details() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    register(&t, &trip.rider, "rider-phone").await;
    // Accepted before she registered: that notice is still queued and finds her device.
    t.work().await;
    assert_eq!(pushed(&t, "rider-phone"), ["driver_assigned"]);

    t.drive_to(&trip.driver, POST_ST_MALL).await;
    for (action, body) in [("pin", json!({ "pin": trip.pin })), ("start", json!({})), ("complete", json!({}))] {
        let (status, v) = t.trip_action(&trip, action, body).await;
        assert_eq!(status, StatusCode::OK, "{action}: {v}");
    }
    t.work().await;
    let kinds = pushed(&t, "rider-phone");
    assert_eq!(kinds.first().map(String::as_str), Some("driver_assigned"));
    assert!(kinds.ends_with(&["trip_started".to_owned(), "trip_completed".to_owned()]), "{kinds:?}");

    for p in t.ctx.push.outbox() {
        let text = format!("{} {}", p.message.title, p.message.body);
        for secret in ["Hilma", "Ndeshi", "Post Street", "Maerua", &trip.pin] {
            assert!(!text.contains(secret), "{text:?} shows {secret:?}");
        }
        let ride = p.message.data.iter().find(|(k, _)| *k == "ride_id").map(|(_, v)| v.as_str());
        assert_eq!(ride, Some(trip.id.as_str()));
    }
    // Pushed, so no SMS fallback.
    assert!(t.ctx.sms.outbox().iter().all(|m| !m.body.contains("your driver")), "{:?}", t.ctx.sms.outbox());
}

#[tokio::test]
async fn no_device_or_a_dead_token_means_a_text_and_the_token_is_forgotten() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    register(&t, &trip.rider, "unregistered-old-install").await;
    t.work().await;

    assert_eq!(token_of(&t, trip.rider.user_id).await, None, "dead token forgotten");
    let texts: Vec<String> = t.ctx.sms.outbox().into_iter().map(|m| m.body).collect();
    assert!(texts.iter().any(|b| b.contains("your driver is on the way")), "{texts:?}");
}

#[tokio::test]
async fn a_cancelled_ride_sends_nothing_late() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    register(&t, &trip.rider, "rider-phone").await;
    let (status, v) = t.call(Method::POST, &format!("/v1/rides/{}/cancel", trip.id), Some(&trip.rider.access), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    t.work().await;
    assert!(pushed(&t, "rider-phone").is_empty());
    assert!(t.ctx.sms.outbox().iter().all(|m| !m.body.contains("your driver")));
}

#[tokio::test]
async fn reviewers_decisions_reach_the_right_app() {
    let t = setup!(relaxed());
    let reviewer = t.staff("ops_reviewer").await;
    let driver = t.sign_in(&new_phone(), "driver").await;
    sqlx::query("INSERT INTO driver_profiles (user_id, application_status) VALUES ($1, 'pending')")
        .bind(driver.user_id)
        .execute(&t.state.db)
        .await
        .unwrap();
    register(&t, &driver, "driver-phone").await;

    let (status, v) = t
        .post(
            &format!("/v1/ops/drivers/{}/decision", driver.user_id),
            &reviewer,
            json!({ "decision": "reject", "reason": "Your licence photo is blurred. Please upload it again." }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    t.work().await;
    assert_eq!(pushed(&t, "driver-phone"), ["driver_not_approved"]);
}
