//! Share trip tests (PRD S-2) against a real PostGIS database: making a link from either app,
//! texting it to her emergency contacts, what the public page shows in each state and never
//! shows, expiry 30 minutes after the trip, a driver's link dying when she cancels, wrong-user
//! access and the per-IP limit on the public page.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_api::rate_limit::{RateLimits, Rule};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

const MAMA: &str = "+264811110001";

impl T {
    async fn share(&self, s: &Session, ride_id: &str, body: Value) -> (StatusCode, Value) {
        self.post(&format!("/v1/rides/{ride_id}/share"), &s.access, body).await
    }

    /// The link's token, from a successful share.
    async fn link(&self, s: &Session, ride_id: &str) -> String {
        let (status, share) = self.share(s, ride_id, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{share}");
        let url = share["url"].as_str().unwrap();
        url.strip_prefix("https://vara.example/t/").expect("a vara.example/t link").to_owned()
    }

    async fn page(&self, token: &str) -> (StatusCode, Value) {
        self.call(Method::GET, &format!("/v1/trip-shares/{token}"), None, None).await
    }
}

#[tokio::test]
async fn the_rider_shares_her_trip_and_the_page_follows_it_to_the_end() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    assert_eq!(
        t.put("/v1/me/emergency-contacts", &trip.rider.access, json!({ "contacts": [{ "name": "Mama", "phone": MAMA }] })).await.0,
        StatusCode::OK
    );

    let (status, share) = t.share(&trip.rider, &trip.id, json!({ "send_to_contacts": true })).await;
    assert_eq!(status, StatusCode::OK, "{share}");
    assert_eq!(share["contacts_texted"], 1);
    let url = share["url"].as_str().unwrap().to_owned();
    assert!(share["text"].as_str().unwrap().starts_with("Ndeshi is sharing her VARA trip with you."));
    let token = url.strip_prefix("https://vara.example/t/").unwrap().to_owned();
    assert_eq!(token.len(), 43);
    // Her contact gets the link by text.
    let mut texted = false;
    for _ in 0..100 {
        texted = t.state.sms.outbox().iter().any(|m| m.to == MAMA && m.body.contains(&url));
        if texted {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(texted, "the link was texted to her emergency contact");

    // Driver on the way: the car, the plate and where it is; never the exact places or the PIN.
    let (status, page) = t.page(&token).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["status"], "driver_on_the_way");
    assert_eq!(page["shared_by"], "Ndeshi");
    assert_eq!(page["shared_by_party"], "rider");
    assert_eq!(page["driver_first_name"], "Hilma");
    assert!(page["car"]["plate"].is_string());
    assert!((page["position"]["lat"].as_f64().unwrap() - NEAR.0).abs() < 1e-6);
    assert!(page["eta"].is_string());
    assert!(
        page["destination_area"].as_str().unwrap().ends_with("area") || page["destination_area"].as_str().unwrap().starts_with("Near ")
    );
    assert_eq!(page["police_phone"], "10111");
    assert!(page["expires_at"].is_null());
    // Exactly these fields: no addresses, no PIN, no phone numbers, no IDs.
    let mut keys: Vec<&str> = page.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "car",
            "destination_area",
            "driver_first_name",
            "eta",
            "expires_at",
            "police_phone",
            "position",
            "shared_by",
            "shared_by_party",
            "status"
        ]
    );
    assert!(!page.to_string().contains(&trip.pin));

    // In the trip: arriving at the destination.
    t.drive_to(&trip.driver, POST_ST_MALL).await;
    for (action, body) in [("pin", json!({ "pin": trip.pin })), ("start", json!({}))] {
        assert_eq!(t.trip_action(&trip, action, body).await.0, StatusCode::OK);
    }
    let (_, page) = t.page(&token).await;
    assert_eq!(page["status"], "in_trip");
    assert!(page["eta"].is_string());

    // Dropped off: the car disappears and the link says when it stops working.
    assert_eq!(t.trip_action(&trip, "complete", json!({})).await.0, StatusCode::OK);
    let (status, page) = t.page(&token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["status"], "ended");
    assert!(page["position"].is_null() && page["car"].is_null() && page["driver_first_name"].is_null() && page["eta"].is_null());
    assert!(page["expires_at"].is_string());

    // 30 minutes after drop-off the link is gone.
    sqlx::query("UPDATE rides SET completed_at = now() - interval '31 minutes' WHERE id = $1::uuid")
        .bind(&trip.id)
        .execute(&t.state.db)
        .await
        .unwrap();
    let (status, e) = t.page(&token).await;
    assert_eq!(status, StatusCode::GONE, "{e}");
    assert_eq!(e["error"]["code"], "link_expired");

    // A finished trip can't be shared again.
    let (status, e) = t.share(&trip.rider, &trip.id, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"]["code"], "not_shareable");
}

#[tokio::test]
async fn a_drivers_link_shows_her_not_the_rider_and_dies_when_she_cancels() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let rider_token = t.link(&trip.rider, &trip.id).await;
    let driver_token = t.link(&trip.driver, &trip.id).await;

    let (status, page) = t.page(&driver_token).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!((page["shared_by"].as_str(), page["shared_by_party"].as_str()), (Some("Hilma"), Some("driver")));
    assert!(!page.to_string().contains("Ndeshi"), "the rider's name is not on the driver's link");

    // She cancels: her link stops at once; the rider's link now says VARA is finding a driver.
    let (status, e) = t.trip_action(&trip, "driver-cancel", json!({ "reason": "vehicle_problem" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");
    assert_eq!(t.page(&driver_token).await.0, StatusCode::NOT_FOUND);
    let (status, page) = t.page(&rider_token).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["status"], "finding_driver");
    assert!(page["car"].is_null() && page["position"].is_null() && page["driver_first_name"].is_null());
}

#[tokio::test]
async fn only_her_own_ride_with_a_driver_can_be_shared() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let stranger = t.rider().await;
    let other_driver = t.online_driver("Selma Shikongo", "classic", FURTHER).await;

    for s in [&stranger, &other_driver] {
        assert_eq!(t.share(s, &trip.id, json!({})).await.0, StatusCode::NOT_FOUND);
    }
    assert_eq!(t.call(Method::POST, &format!("/v1/rides/{}/share", trip.id), None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.share(&trip.rider, &trip.id, json!({ "everyone": true })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(t.share(&trip.rider, "not-a-uuid", json!({})).await.0, StatusCode::BAD_REQUEST);

    // Still searching: nothing to follow yet.
    let ride = t.book(&stranger).await;
    let (status, e) = t.share(&stranger, ride["id"].as_str().unwrap(), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");

    // Unknown, malformed and stopped links all look the same.
    let token = t.link(&trip.rider, &trip.id).await;
    let unknown = "A".repeat(43);
    for bad in [unknown.as_str(), "short", "not%2Fa%2Ftoken0000000000000000000000000000000"] {
        assert_eq!(t.page(bad).await.0, StatusCode::NOT_FOUND, "{bad}");
    }
    sqlx::query("UPDATE trip_shares SET revoked_at = now() WHERE ride_id = $1::uuid").bind(&trip.id).execute(&t.state.db).await.unwrap();
    assert_eq!(t.page(&token).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_public_page_is_rate_limited_per_address_and_never_cached() {
    let t = setup!(RateLimits { trip_share: Rule { limit: 3, window_s: 60 }, ..relaxed() });
    let trip = t.accepted().await;
    let token = t.link(&trip.rider, &trip.id).await;

    let req = axum::http::Request::builder().uri(format!("/v1/trip-shares/{token}")).body(axum::body::Body::empty()).unwrap();
    let res = tower::ServiceExt::oneshot(t.app.clone(), req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["cache-control"], "no-store");
    for _ in 0..2 {
        assert_eq!(t.page(&token).await.0, StatusCode::OK);
    }
    assert_eq!(t.page(&token).await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn new_links_are_limited_per_person() {
    let t = setup!(RateLimits { trip_share_create: Rule { limit: 2, window_s: 3600 }, ..relaxed() });
    let trip = t.accepted().await;
    for _ in 0..2 {
        assert_eq!(t.share(&trip.rider, &trip.id, json!({})).await.0, StatusCode::OK);
    }
    assert_eq!(t.share(&trip.rider, &trip.id, json!({})).await.0, StatusCode::TOO_MANY_REQUESTS);
    // The driver has her own allowance.
    assert_eq!(t.share(&trip.driver, &trip.id, json!({})).await.0, StatusCode::OK);
}
