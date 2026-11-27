//! Consent enforcement (PRD A-7): new activity waits until she has accepted the current Terms
//! and Privacy Policy, checked on the server, not only by the app's screens. A ride already
//! under way, and her safety tools, never wait for it.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

/// Publishes new Terms that everyone must accept again.
async fn new_terms(t: &T) {
    sqlx::query(
        "INSERT INTO legal_documents (kind, version, url, content_sha256, published_at, requires_reacceptance)
         VALUES ('terms', '2099-01-01', 'https://vara.example/legal/terms-2099', decode('00', 'hex'), now(), true)",
    )
    .execute(&t.state.db)
    .await
    .unwrap();
}

fn refused(r: &(StatusCode, Value)) -> bool {
    r.0 == StatusCode::FORBIDDEN && r.1["error"]["code"] == "consent_required"
}

#[tokio::test]
async fn new_terms_hold_back_new_rides_but_never_a_trip_under_way() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let idle = t.approved_driver("Selma Iipinge", "classic", false).await;
    new_terms(&t).await;

    // The trip carries on: pickup, start, chat and SOS all still work.
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    for (action, body) in [("pin", json!({ "pin": trip.pin })), ("start", json!({}))] {
        let (status, v) = t.trip_action(&trip, action, body).await;
        assert_eq!(status, StatusCode::OK, "{action}: {v}");
    }
    let chat = format!("/v1/rides/{}/messages", trip.id);
    let (status, v) = t
        .call_with(Method::POST, &chat, Some(&trip.rider.access), Some(json!({ "text": "Nearly there" })), &[("idempotency-key", &key())])
        .await;
    assert!(status.is_success(), "{v}");
    let (status, v) = t.post("/v1/safety/sos", &trip.rider.access, json!({ "ride_id": trip.id })).await;
    assert!(status.is_success(), "{v}");

    // Anything new waits for the new Terms.
    let q = t.quote(&trip.rider, POST_ST_MALL, MAERUA_MALL).await;
    let booking = json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash" });
    assert!(refused(&t.request(&trip.rider, booking.clone(), &key()).await));
    let online = json!({ "at": at(NEAR) });
    assert!(refused(&t.post("/v1/driver/online", &idle.access, online.clone()).await));
    let upload = json!({ "purpose": "selfie", "content_type": "image/png", "size_bytes": 1000 });
    assert!(refused(&t.post("/v1/uploads", &trip.rider.access, upload).await));
    let contacts = json!({ "contacts": [{ "name": "Martha Shikongo", "phone": "081 222 3344" }] });
    assert!(refused(&t.put("/v1/me/emergency-contacts", &trip.rider.access, contacts).await));

    // Her account says what to accept; once she has, she can drive again.
    let (_, me) = t.get("/v1/me", &idle.access).await;
    let pending: Vec<&str> = me["pending_consents"].as_array().unwrap().iter().map(|d| d["version"].as_str().unwrap()).collect();
    assert_eq!(pending, ["2099-01-01"]);
    t.accept_consents(&idle).await;
    let (status, v) = t.post("/v1/driver/online", &idle.access, online).await;
    assert_eq!(status, StatusCode::OK, "{v}");
}
