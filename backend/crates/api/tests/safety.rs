//! Phase 3 SOS tests against a real PostGIS database: the incident on the safety desk's queue,
//! the emergency texts within 5 s with a live trip link, a repeat press, SOS without a ride or
//! outside Windhoek, the retry job, no rate limit, and a ride that isn't hers.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use fuber_api::rate_limit::{RateLimits, Rule};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

const MAMA: &str = "+264811110001";
const SISTER: &str = "+264811110002";

impl T {
    async fn with_contacts(&self, s: &Session) {
        let contacts = json!({ "contacts": [{ "name": "Mama", "phone": MAMA }, { "name": "Sister", "phone": SISTER }] });
        let (status, body) = self.put("/v1/me/emergency-contacts", &s.access, contacts).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    /// A rider with two emergency contacts on a Classic ride a driver has accepted.
    async fn on_a_ride(&self) -> (Session, Session, String) {
        let rider = self.rider().await;
        self.with_contacts(&rider).await;
        let driver = self.online_driver("Hilma Nangolo", "classic", NEAR).await;
        let ride = self.book(&rider).await;
        assert_eq!(self.tick().await.offered, 1);
        let offer = self.driver_status(&driver).await["offer"]["offer_id"].as_str().unwrap().to_owned();
        let (status, view) = self.post(&format!("/v1/offers/{offer}/accept"), &driver.access, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        (rider, driver, ride["id"].as_str().unwrap().to_owned())
    }

    /// Texts sent to these numbers, waiting up to `within` for `n` of them.
    async fn texts_to(&self, to: &[&str], n: usize, within: Duration) -> Vec<String> {
        let start = Instant::now();
        loop {
            let got: Vec<String> = self.state.sms.outbox().into_iter().filter(|m| to.contains(&m.to.as_str())).map(|m| m.body).collect();
            if got.len() >= n || start.elapsed() > within {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn sos(&self, s: &Session, body: Value) -> (StatusCode, Value) {
        self.post("/v1/safety/sos", &s.access, body).await
    }
}

fn token_in(text: &str) -> &str {
    let at = text.find("https://vara.example/t/").expect("trip link") + "https://vara.example/t/".len();
    &text[at..at + 43]
}

#[tokio::test]
async fn sos_on_a_ride_reaches_the_desk_and_texts_her_contacts_within_5_seconds() {
    let t = setup!(relaxed());
    let (rider, _driver, ride_id) = t.on_a_ride().await;

    let start = Instant::now();
    let (status, alert) = t.sos(&rider, json!({ "location": at(NEAR) })).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    assert_eq!(alert["ride_id"], ride_id.as_str(), "her current ride is found without naming it");
    assert_eq!(alert["contacts_alerted"], 2);
    assert_eq!(alert["police_phone"], "10111");
    let texts = t.texts_to(&[MAMA, SISTER], 2, Duration::from_secs(5)).await;
    assert_eq!(texts.len(), 2, "both contacts texted within 5 s");
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(texts[0].starts_with("VARA SOS: Ndeshi pressed the emergency button."), "{}", texts[0]);

    // The link is a fresh 256-bit token for this ride; only its hash is stored.
    let token = token_in(&texts[0]);
    let hash = fuber_db::safety::share_hash(&t.state.keys.hasher, token);
    let (share_ride, by): (String, String) =
        sqlx::query_as("SELECT ride_id::text, created_by::text FROM trip_shares WHERE token_hash = $1")
            .bind(&hash)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((share_ride.as_str(), by.as_str()), (ride_id.as_str(), "rider"));

    // The safety desk sees it at the top of the queue, with who and where.
    let desk = t.staff("ops_safety").await;
    let (status, queue) = t.call(Method::GET, "/v1/ops/incidents", Some(&desk), None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let first = &queue[0];
    assert_eq!(first["id"], alert["incident_id"]);
    assert_eq!(first["kind"], "sos");
    assert_eq!(first["ride_id"], ride_id.as_str());
    assert_eq!(first["raised_by"]["user_id"], rider.user_id.to_string());
    assert!((first["lat"].as_f64().unwrap() - NEAR.0).abs() < 1e-6);
    let reviewer = t.staff("ops_reviewer").await;
    assert_eq!(t.call(Method::GET, "/v1/ops/incidents", Some(&reviewer), None).await.0, StatusCode::FORBIDDEN);

    // Pressing again within two minutes is the same incident, and nobody is texted twice.
    let (status, again) = t.sos(&rider, json!({ "location": at(FURTHER) })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["incident_id"], alert["incident_id"]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(t.texts_to(&[MAMA, SISTER], 3, Duration::ZERO).await.len(), 2);
    let incidents: i64 = sqlx::query_scalar("SELECT count(*) FROM safety_incidents WHERE raised_by = $1")
        .bind(rider.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(incidents, 1);
}

#[tokio::test]
async fn the_driver_can_raise_sos_on_her_trip_and_between_trips() {
    let t = setup!(relaxed());
    let (_rider, driver, ride_id) = t.on_a_ride().await;
    t.with_contacts(&driver).await;

    // On the trip, with no location: her last reported position is used.
    let (status, alert) = t.sos(&driver, json!({ "ride_id": ride_id })).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    let texts = t.texts_to(&[MAMA, SISTER], 2, Duration::from_secs(5)).await;
    assert!(texts[0].starts_with("VARA SOS: Hilma pressed"), "{}", texts[0]);
    let by: String = sqlx::query_scalar("SELECT created_by::text FROM trip_shares WHERE ride_id = $1::uuid")
        .bind(&ride_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(by, "driver");
    let has_location: bool = sqlx::query_scalar("SELECT location IS NOT NULL FROM safety_incidents WHERE id = $1::uuid")
        .bind(alert["incident_id"].as_str().unwrap())
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert!(has_location);

    // Online with no trip: an incident without a ride, and her position instead of a link.
    let waiting = t.online_driver("Selma Iipinge", "classic", FURTHER).await;
    let (status, alert) = t.sos(&waiting, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    assert!(alert["ride_id"].is_null());
    assert_eq!(alert["contacts_alerted"], 0, "she has no emergency contacts");
    assert!(alert["message"].as_str().unwrap().contains("Add emergency contacts"));
}

#[tokio::test]
async fn sos_is_accepted_outside_windhoek_and_without_a_ride() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    t.with_contacts(&rider).await;
    // Near Okahandja, well outside the service area; unknown fields are ignored.
    let (status, alert) = t.sos(&rider, json!({ "location": { "lat": -21.98, "lon": 16.91 }, "app_version": "1.0" })).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    let texts = t.texts_to(&[MAMA, SISTER], 2, Duration::from_secs(5)).await;
    assert!(texts[0].contains("openstreetmap.org/?mlat=-21.98000&mlon=16.91000"), "{}", texts[0]);
    // Impossible coordinates are dropped rather than refusing the SOS.
    let other = t.rider().await;
    let (status, _) = t.sos(&other, json!({ "location": { "lat": 123.0, "lon": 17.0 } })).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn texts_that_failed_are_resent_by_the_worker_with_a_fresh_link() {
    let t = setup!(relaxed());
    let (rider, _driver, ride_id) = t.on_a_ride().await;
    let (_, alert) = t.sos(&rider, json!({})).await;
    let first = t.texts_to(&[MAMA, SISTER], 2, Duration::from_secs(5)).await;
    let incident = alert["incident_id"].as_str().unwrap().parse().unwrap();

    // As if the text to the second contact had failed.
    fuber_worker::sos_sms(&t.ctx, incident, &[2]).await.unwrap();
    let resent: Vec<_> = t.ctx.sms.outbox().into_iter().filter(|m| m.to == MAMA || m.to == SISTER).collect();
    assert_eq!(resent.len(), 1);
    assert_eq!(resent[0].to, SISTER);
    assert_ne!(token_in(&resent[0].body), token_in(&first[0]));
    let shares: i64 = sqlx::query_scalar("SELECT count(*) FROM trip_shares WHERE ride_id = $1::uuid")
        .bind(&ride_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(shares, 2);
}

#[tokio::test]
async fn sos_is_never_rate_limited() {
    // Room for signing in; then the test IP uses up the rest of its global allowance.
    let limits = RateLimits { global: Rule::per_minute(60), ..relaxed() };
    let t = setup!(limits);
    let rider = t.rider().await;
    let mut limited = false;
    for _ in 0..60 {
        if t.get("/v1/me", &rider.access).await.0 == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
    }
    assert!(limited, "the global limit should have been reached");
    for _ in 0..3 {
        let (status, alert) = t.sos(&rider, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{alert}");
    }
    assert_eq!(t.get("/v1/me", &rider.access).await.0, StatusCode::TOO_MANY_REQUESTS, "other routes stay limited");
}

#[tokio::test]
async fn sos_is_never_held_back_by_her_own_request_allowance() {
    let limits = RateLimits { user: Rule::per_minute(30), ..relaxed() };
    let t = setup!(limits);
    let rider = t.rider().await;
    let mut limited = false;
    for _ in 0..40 {
        if t.get("/v1/me", &rider.access).await.0 == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
    }
    assert!(limited, "her per-user limit should have been reached");
    for _ in 0..3 {
        let (status, alert) = t.sos(&rider, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{alert}");
    }
    assert_eq!(t.get("/v1/me", &rider.access).await.0, StatusCode::TOO_MANY_REQUESTS, "other routes stay limited");
}

#[tokio::test]
async fn sos_needs_a_signed_in_user_and_never_names_someone_elses_ride() {
    let t = setup!(relaxed());
    let (_rider, driver, ride_id) = t.on_a_ride().await;
    let links = || async {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM trip_shares WHERE ride_id = $1::uuid")
            .bind(&ride_id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
        n
    };

    // Someone else's ride, or no such ride: still an SOS, without that ride and its link.
    let stranger = t.rider().await;
    t.with_contacts(&stranger).await;
    let (status, alert) = t.sos(&stranger, json!({ "ride_id": ride_id })).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    assert!(alert["ride_id"].is_null(), "{alert}");
    assert_eq!(links().await, 0);
    let (status, _) = t.sos(&t.rider().await, json!({ "ride_id": uuid::Uuid::new_v4() })).await;
    assert_eq!(status, StatusCode::OK);

    // She cancelled because she felt unsafe and pressed SOS before her screen caught up.
    let (status, e) = t.post(&format!("/v1/rides/{ride_id}/driver-cancel"), &driver.access, json!({ "reason": "feel_unsafe" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{e}");
    let (status, alert) = t.sos(&driver, json!({ "ride_id": ride_id })).await;
    assert_eq!(status, StatusCode::OK, "{alert}");
    assert!(alert["ride_id"].is_null(), "{alert}");
    assert_eq!(links().await, 0);

    assert_eq!(t.call(Method::POST, "/v1/safety/sos", None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED);
}
