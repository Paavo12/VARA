//! Booking helpers shared by the dispatch and trip tests: verified riders, approved drivers,
//! quotes, requests and matching ticks.

use axum::http::{Method, StatusCode};
use fuber_api::rate_limit::{RateLimits, Rule};
use fuber_db::sqlx;
use fuber_worker::dispatch;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Session, T, new_phone};

/// These tests sign in dozens of riders and drivers from one test IP; the limits themselves
/// are tested in `http.rs` and `onboarding.rs`.
pub fn relaxed() -> RateLimits {
    let lots = Rule::per_minute(100_000);
    RateLimits {
        global: lots,
        otp_request: lots,
        otp_phone: lots,
        otp_verify: lots,
        user: lots,
        quote: lots,
        ride_request: lots,
        ride_request_hourly: lots,
        ..RateLimits::default()
    }
}

pub const POST_ST_MALL: (f64, f64) = (-22.56665, 17.08126);
pub const MAERUA_MALL: (f64, f64) = (-22.58359, 17.09373);
/// About 600 m and 1.5 km from Post Street Mall, and one well outside the 3 km radius.
pub const NEAR: (f64, f64) = (-22.5620, 17.0830);
pub const FURTHER: (f64, f64) = (-22.5540, 17.0870);
pub const FAR_AWAY: (f64, f64) = (-22.5100, 17.0300);

pub fn at(p: (f64, f64)) -> Value {
    json!({ "lat": p.0, "lon": p.1 })
}

pub fn key() -> String {
    Uuid::new_v4().simple().to_string()
}

impl T {
    /// A rider whose identity is verified (the verification flow itself is tested in Phase 1).
    pub async fn rider(&self) -> Session {
        let s = self.onboarded("rider").await;
        sqlx::query(
            "UPDATE users SET verification = 'approved', verified_at = now(), status = 'active', display_name = 'Ndeshi' WHERE id = $1",
        )
        .bind(s.user_id)
        .execute(&self.state.db)
        .await
        .unwrap();
        s
    }

    /// An approved driver with an approved vehicle for `tier` (the approval flow is tested in
    /// Phase 1), not yet online.
    pub async fn approved_driver(&self, name: &str, tier: &str, wheelchair: bool) -> Session {
        let s = self.sign_in(&new_phone(), "driver").await;
        let db = &self.state.db;
        sqlx::query("UPDATE users SET full_name = $2, display_name = split_part($2, ' ', 1), status = 'active' WHERE id = $1")
            .bind(s.user_id)
            .bind(name)
            .execute(db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO driver_profiles (user_id, application_status, approved_at, can_go_online) VALUES ($1, 'approved', now(), true)",
        )
        .bind(s.user_id)
        .execute(db)
        .await
        .unwrap();
        let plate = format!("N {} W", 1000 + u32::from_le_bytes(fuber_common::crypto::random_bytes::<4>()) % 900_000);
        let vehicle: Uuid = sqlx::query_scalar(
            "INSERT INTO vehicles (driver_id, make, model, colour, year, plate, wheelchair_accessible, approved)
             VALUES ($1, 'Toyota', 'Corolla', 'Silver', 2022, $2, $3, true) RETURNING id",
        )
        .bind(s.user_id)
        .bind(&plate)
        .bind(wheelchair)
        .fetch_one(db)
        .await
        .unwrap();
        sqlx::query("UPDATE driver_profiles SET active_vehicle_id = $2 WHERE user_id = $1")
            .bind(s.user_id)
            .bind(vehicle)
            .execute(db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO driver_tiers (driver_id, tier) VALUES ($1, $2::ride_tier)")
            .bind(s.user_id)
            .bind(tier)
            .execute(db)
            .await
            .unwrap();
        s
    }

    pub async fn online_driver(&self, name: &str, tier: &str, p: (f64, f64)) -> Session {
        let s = self.approved_driver(name, tier, false).await;
        let (status, body) = self.post("/v1/driver/online", &s.access, json!({ "at": at(p) })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["online"], true);
        s
    }

    pub async fn quote(&self, rider: &Session, from: (f64, f64), to: (f64, f64)) -> Value {
        let body = json!({ "pickup": at(from), "pickup_address": "Post Street Mall", "dropoff": at(to), "dropoff_address": "Maerua Mall" });
        let (status, q) = self.post("/v1/rides/quote", &rider.access, body).await;
        assert_eq!(status, StatusCode::OK, "{q}");
        q
    }

    pub async fn request(&self, rider: &Session, body: Value, key: &str) -> (StatusCode, Value) {
        self.call_with(Method::POST, "/v1/rides", Some(&rider.access), Some(body), &[("idempotency-key", key)]).await
    }

    /// Quotes Post Street Mall → Maerua Mall and requests a Classic ride for one.
    pub async fn book(&self, rider: &Session) -> Value {
        let q = self.quote(rider, POST_ST_MALL, MAERUA_MALL).await;
        let (status, ride) =
            self.request(rider, json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash" }), &key()).await;
        assert_eq!(status, StatusCode::CREATED, "{ride}");
        ride
    }

    pub async fn tick(&self) -> dispatch::Tick {
        dispatch::tick(&self.ctx).await.unwrap()
    }

    pub async fn driver_status(&self, d: &Session) -> Value {
        let (status, body) = self.get("/v1/driver/status", &d.access).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    pub async fn ride_state(&self, id: &str) -> String {
        sqlx::query_scalar("SELECT state::text FROM rides WHERE id = $1::uuid").bind(id).fetch_one(&self.state.db).await.unwrap()
    }

    /// Makes the pending offer for this ride time out now.
    pub async fn expire_offer(&self, ride_id: &str) {
        sqlx::query("UPDATE ride_offers SET expires_at = now() - interval '1 second' WHERE ride_id = $1::uuid AND response = 'pending'")
            .bind(ride_id)
            .execute(&self.state.db)
            .await
            .unwrap();
    }
}
