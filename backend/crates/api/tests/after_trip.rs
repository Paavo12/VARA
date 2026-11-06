//! Phase 4 tests against a real PostGIS database: cash confirmation and the driver ledger,
//! the earnings dashboard, two-way ratings and favourites, trip history for both apps, and
//! wrong-user access to each.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::json;

use common::*;

impl T {
    /// (fare, service fee, driver earning) in cents.
    async fn money(&self, ride_id: &str) -> (i64, i64, i64) {
        sqlx::query_as("SELECT fare_cents, service_fee_cents, driver_earning_cents FROM rides WHERE id = $1::uuid")
            .bind(ride_id)
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }

    async fn rider_view(&self, trip: &Trip) -> serde_json::Value {
        let (status, v) = self.get(&format!("/v1/rides/{}", trip.id), &trip.rider.access).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }
}

#[tokio::test]
async fn cash_is_confirmed_once_and_books_the_service_fee_against_the_driver() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let (fare, fee, _) = t.money(&trip.id).await;
    assert_eq!(t.rider_view(&trip).await["payment_status"], "pending");

    let (status, v) = t.trip_action(&trip, "cash-received", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["payment_status"], "cash_confirmed");
    assert_eq!(v["service_fee"], fee);
    assert_eq!(v["balance"], -fee, "she owes VARA the service fee");
    // A second tap changes nothing.
    let (_, again) = t.trip_action(&trip, "cash-received", json!({})).await;
    assert_eq!(again["balance"], -fee);
    let (entries, amount): (i64, i64) =
        sqlx::query_as("SELECT count(*), sum(amount_cents)::int8 FROM driver_ledger WHERE ride_id = $1::uuid")
            .bind(&trip.id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((entries, amount), (1, -fee));
    let (method, status, paid): (String, String, i64) =
        sqlx::query_as("SELECT method::text, status::text, amount_cents FROM payments WHERE ride_id = $1::uuid")
            .bind(&trip.id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((method.as_str(), status.as_str(), paid), ("cash", "cash_confirmed", fare));
    assert_eq!(t.rider_view(&trip).await["payment_status"], "cash_confirmed");
}

#[tokio::test]
async fn cash_cannot_be_confirmed_before_the_trip_ends_or_by_anyone_else() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let (status, e) = t.trip_action(&trip, "cash-received", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"]["code"], "not_completed");

    let stranger = t.online_driver("Martha Kandjii", "classic", FAR_AWAY).await;
    let uri = format!("/v1/rides/{}/cash-received", trip.id);
    assert_eq!(t.post(&uri, &stranger.access, json!({})).await.0, StatusCode::NOT_FOUND);
    let (status, e) = t.post(&uri, &trip.rider.access, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(e["error"]["code"], "wrong_app");
    assert_eq!(t.call(Method::POST, &uri, None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_earnings_dashboard_adds_up() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let (fare, fee, earning) = t.money(&trip.id).await;

    // Before she confirms the cash, nothing is collected or owed yet.
    let (status, e) = t.get("/v1/driver/earnings", &trip.driver.access).await;
    assert_eq!(status, StatusCode::OK, "{e}");
    assert_eq!(e["period"], "today");
    assert_eq!(e["trips"], 1);
    assert_eq!(e["earnings"], earning);
    assert_eq!(e["service_fees"], fee);
    assert_eq!(e["cash_collected"], 0);
    assert_eq!(e["balance"], 0);
    assert_eq!(e["recent"][0]["payment_status"], "pending");
    assert!(e["online_s"].as_i64().unwrap() >= 0);

    t.trip_action(&trip, "cash-received", json!({})).await;
    let (_, e) = t.get("/v1/driver/earnings?period=week", &trip.driver.access).await;
    assert_eq!(e["period"], "week");
    assert_eq!(e["trips"], 1);
    assert_eq!(e["cash_collected"], fare);
    assert_eq!(e["balance"], -fee);
    assert_eq!(e["recent"][0]["ride_id"], trip.id.as_str());
    assert_eq!(e["recent"][0]["payment_status"], "cash_confirmed");
    // Fare = earning + fee: the receipt adds up.
    assert_eq!(fare, earning + fee);

    assert_eq!(t.get("/v1/driver/earnings?period=year", &trip.driver.access).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(t.get("/v1/driver/earnings", &trip.rider.access).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn both_sides_rate_and_the_ride_becomes_rated() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let rate = |who: &Session, body: serde_json::Value| {
        let uri = format!("/v1/rides/{}/rating", trip.id);
        let token = who.access.clone();
        let t = &t;
        async move { t.post(&uri, &token, body).await }
    };

    // The rider: 5 stars and a favourite. Not RATED until the driver is done too.
    let (status, v) = rate(&trip.rider, json!({ "stars": 5, "favourite": true, "comment": "Lovely, careful driver" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "COMPLETED");
    assert_eq!(t.rider_view(&trip).await["rated"], true);
    let (avg, count): (f64, i32) = sqlx::query_as("SELECT rating_avg::float8, rating_count FROM driver_profiles WHERE user_id = $1")
        .bind(trip.driver.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!((avg, count), (5.0, 1));
    let (status, e) = rate(&trip.rider, json!({ "stars": 4 })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(e["error"]["code"], "already_rated");

    // Her favourites list has the driver; she can remove her.
    let (_, favs) = t.get("/v1/me/favourite-drivers", &trip.rider.access).await;
    assert_eq!(favs.as_array().unwrap().len(), 1, "{favs}");
    assert_eq!(favs[0]["driver_id"], trip.driver.user_id.to_string());
    assert_eq!(favs[0]["first_name"], "Hilma");

    // The driver: below 3 stars needs a reason, and only reasons that fit her side.
    assert_eq!(rate(&trip.driver, json!({ "stars": 2 })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rate(&trip.driver, json!({ "stars": 2, "reasons": ["route"] })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rate(&trip.driver, json!({ "stars": 4, "favourite": true })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rate(&trip.driver, json!({ "stars": 6 })).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, v) = rate(&trip.driver, json!({ "stars": 2, "reasons": ["late", "rude"] })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "RATED");
    assert_eq!(t.ride_state(&trip.id).await, "RATED");
    let avg: f64 = sqlx::query_scalar("SELECT rating_avg::float8 FROM rider_profiles WHERE user_id = $1")
        .bind(trip.rider.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(avg, 2.0);
    // Cash can still be confirmed on a rated trip.
    assert_eq!(t.trip_action(&trip, "cash-received", json!({})).await.0, StatusCode::OK);

    let uri = format!("/v1/me/favourite-drivers/{}", trip.driver.user_id);
    assert_eq!(t.call(Method::DELETE, &uri, Some(&trip.rider.access), None).await.0, StatusCode::NO_CONTENT);
    let (_, favs) = t.get("/v1/me/favourite-drivers", &trip.rider.access).await;
    assert!(favs.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_driver_who_skips_rating_leaves_it_to_the_rider() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    let (status, v) = t.trip_action(&trip, "rating/skip", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "COMPLETED", "still waiting for the rider");
    let (status, v) = t.post(&format!("/v1/rides/{}/rating", trip.id), &trip.rider.access, json!({ "stars": 4 })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "RATED");
}

#[tokio::test]
async fn nobody_rates_an_unfinished_trip_or_someone_elses() {
    let t = setup!(relaxed());
    let trip = t.accepted().await;
    let uri = format!("/v1/rides/{}/rating", trip.id);
    let (status, e) = t.post(&uri, &trip.rider.access, json!({ "stars": 5 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{e}");
    assert_eq!(e["error"]["code"], "not_completed");
    assert_eq!(t.trip_action(&trip, "rating/skip", json!({})).await.0, StatusCode::CONFLICT);

    let other = t.rider().await;
    assert_eq!(t.post(&uri, &other.access, json!({ "stars": 5 })).await.0, StatusCode::NOT_FOUND);
    let stranger = t.online_driver("Martha Kandjii", "classic", FAR_AWAY).await;
    assert_eq!(t.post(&uri, &stranger.access, json!({ "stars": 5 })).await.0, StatusCode::NOT_FOUND);
    let skip = format!("/v1/rides/{}/rating/skip", trip.id);
    assert_eq!(t.post(&skip, &trip.rider.access, json!({})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.call(Method::POST, &uri, None, Some(json!({ "stars": 5 }))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.get("/v1/me/favourite-drivers", &trip.driver.access).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn trip_history_shows_each_side_only_what_it_saw() {
    let t = setup!(relaxed());
    let trip = t.completed().await;
    t.post(&format!("/v1/rides/{}/rating", trip.id), &trip.rider.access, json!({ "stars": 5 })).await;

    // A second ride with the same pair, which the rider cancels while the driver is on the way.
    let ride2 = t.book(&trip.rider).await;
    let id2 = ride2["id"].as_str().unwrap().to_owned();
    assert_eq!(t.tick().await.offered, 1);
    let offer = t.driver_status(&trip.driver).await["offer"]["offer_id"].as_str().unwrap().to_owned();
    assert_eq!(t.post(&format!("/v1/offers/{offer}/accept"), &trip.driver.access, json!({})).await.0, StatusCode::OK);
    assert_eq!(t.post(&format!("/v1/rides/{id2}/cancel"), &trip.rider.access, json!({})).await.0, StatusCode::OK);

    // Rider: both trips, newest first, with the driver's name and her rating.
    let (status, mine) = t.get("/v1/rides", &trip.rider.access).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    let mine = mine.as_array().unwrap();
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0]["id"], id2.as_str());
    assert_eq!(mine[0]["state"], "CANCELLED");
    assert_eq!(mine[1]["id"], trip.id.as_str());
    assert_eq!(mine[1]["driver_first_name"], "Hilma");
    assert_eq!(mine[1]["my_rating"], 5);
    assert_eq!(mine[1]["dropoff_address"], "Maerua Mall");

    // Driver: the completed trip in full; the cancelled one without pickup or destination.
    let (status, hers) = t.get("/v1/driver/trips", &trip.driver.access).await;
    assert_eq!(status, StatusCode::OK, "{hers}");
    let hers = hers.as_array().unwrap();
    assert_eq!(hers.len(), 2);
    assert_eq!(hers[0]["state"], "CANCELLED");
    assert!(hers[0]["pickup_address"].is_null() && hers[0]["dropoff_address"].is_null(), "{}", hers[0]);
    assert!(!hers[0].to_string().contains("Maerua"));
    assert_eq!(hers[1]["dropoff_address"], "Maerua Mall");
    assert_eq!(hers[1]["pickup_address"], "Post Street Mall");
    assert_eq!(hers[1]["payment_status"], "pending");

    // Paging: everything before the newest trip.
    let before = mine[0]["requested_at"].as_str().unwrap().replace('+', "%2B");
    let (_, older) = t.get(&format!("/v1/rides?before={before}"), &trip.rider.access).await;
    assert_eq!(older.as_array().unwrap().len(), 1);
    assert_eq!(older[0]["id"], trip.id.as_str());

    // Nobody else's trips, and each list is for its own app.
    let other = t.rider().await;
    assert!(t.get("/v1/rides", &other.access).await.1.as_array().unwrap().is_empty());
    assert_eq!(t.get("/v1/rides", &trip.driver.access).await.0, StatusCode::FORBIDDEN);
    assert_eq!(t.get("/v1/driver/trips", &trip.rider.access).await.0, StatusCode::FORBIDDEN);
}
