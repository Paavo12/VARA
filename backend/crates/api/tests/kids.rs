//! Kids Guardian (client decision 2026-11-20, 2026-11-23): a rider brings her children and may ask
//! for a child seat, which adds the flat surcharge an admin sets (F_kids). The surcharge is locked
//! with the quote and goes to the driver in full; the driver sees the children on the offer.
//! Children never ride alone.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use serde_json::{Value, json};

use common::*;

fn classic(quote: &Value) -> &Value {
    quote["options"].as_array().unwrap().iter().find(|o| o["tier"] == "classic").expect("classic option")
}

fn ride(q: &Value, passengers: i64, children: i64, child_seat: bool) -> Value {
    json!({ "quote": q["quote"], "tier": "classic", "passengers": passengers, "payment_method": "cash",
            "children": children, "child_seat": child_seat })
}

#[tokio::test]
async fn a_child_seat_adds_the_locked_surcharge_and_the_driver_keeps_it() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let rider = t.rider().await;
    let driver = t.online_driver("Hilma Nangolo", "classic", NEAR).await;

    // N$0 until an admin sets it; admins set it within N$0–200.
    assert_eq!(t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await["kids_surcharge"], 0);
    let uri = "/v1/ops/settings/pricing.kids_surcharge_cents";
    assert_eq!(t.call(Method::PUT, uri, Some(&admin), Some(json!({ "value": 30_000 }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, set) = t.call(Method::PUT, uri, Some(&admin), Some(json!({ "value": 2000 }))).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    assert_eq!(q["kids_surcharge"], 2000);
    let base = classic(&q)["fare"].clone();

    // Locked: a change after the quote doesn't touch this booking.
    t.call(Method::PUT, uri, Some(&admin), Some(json!({ "value": 5000 }))).await;

    // Children ride with an adult, a seat needs a child, 3 children at most.
    for bad in [ride(&q, 1, 1, false), ride(&q, 2, 0, true), ride(&q, 4, 4, false)] {
        let (status, e) = t.request(&rider, bad.clone(), &key()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad} → {e}");
    }

    let (status, booked) = t.request(&rider, ride(&q, 3, 2, true), &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{booked}");
    let fare = &booked["fare"];
    let total = base["total"].as_i64().unwrap() + 2000;
    assert_eq!((fare["kids"].as_i64(), fare["total"].as_i64()), (Some(2000), Some(total)), "{fare}");
    assert_eq!(fare["service_fee"], base["service_fee"], "no fee on the surcharge");

    // The driver sees the children and the seat; her earning includes all of the surcharge.
    assert_eq!(t.tick().await.offered, 1);
    let card = t.driver_status(&driver).await["offer"].clone();
    assert_eq!((card["passenger"]["children"].as_i64(), card["passenger"]["child_seat"].as_bool()), (Some(2), Some(true)), "{card}");
    assert_eq!(card["passenger"]["child_seat_surcharge"], 2000);
    let fee = base["service_fee"].as_i64().unwrap();
    assert_eq!(card["earning"].as_i64().unwrap(), total - fee);
    let (status, view) = t.post(&format!("/v1/offers/{}/accept", card["offer_id"].as_str().unwrap()), &driver.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let (booked_fee, earning): (i64, i64) = sqlx::query_as("SELECT service_fee_cents, driver_earning_cents FROM rides WHERE id = $1::uuid")
        .bind(booked["id"].as_str().unwrap())
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!((booked_fee, earning), (fee, total - fee));
}

#[tokio::test]
async fn children_never_ride_alone() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let for_someone = |confirm: bool| {
        json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash",
                "guest": { "name": "Ndapandula Shikongo", "phone": "+264811234599", "confirm_adult": confirm } })
    };
    // Booking for someone else needs her word that the passenger is an adult woman.
    let (status, e) = t.request(&rider, for_someone(false), &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert!(e["error"]["message"].as_str().unwrap().contains("adult"), "{e}");
    let (status, ride) = t.request(&rider, for_someone(true), &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
}
