//! Phase 2 end-to-end tests against a real PostGIS database: places search, locked quotes,
//! ride requests, matching through the worker, offers, the accept race, privacy of the
//! driver's view, cancellations, scheduled rides, wrong-user access and the WebSocket.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use std::{net::SocketAddr, time::Duration};

use axum::http::{Method, StatusCode};
use fuber_db::sqlx;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use common::*;

#[tokio::test]
async fn places_search_finds_landmarks_suburbs_and_streets_like_the_demo() {
    let t = setup!(relaxed());
    let r = t.rider().await;
    let names = |v: &Value| v.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>();

    // Empty query: the landmarks, for the quick-place chips.
    let (status, all) = t.get("/v1/places?q=", &r.access).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all.as_array().unwrap().len(), 14);
    assert!(all.as_array().unwrap().iter().all(|p| p["kind"] == "landmark"));

    // Landmarks come first; case doesn't matter.
    let (_, malls) = t.get("/v1/places?q=MALL", &r.access).await;
    let malls = names(&malls);
    for m in ["Post Street Mall", "Maerua Mall", "Grove Mall of Namibia", "Wernhil Park Mall"] {
        assert!(malls[..4].contains(&m.to_owned()), "{m} in {malls:?}");
    }
    // Suburbs, streets, accents and a leading house number.
    let (_, klein) = t.get("/v1/places?q=klein%20windhoek", &r.access).await;
    assert_eq!(klein[0]["name"], "Klein Windhoek");
    assert_eq!(klein[0]["kind"], "suburb");
    let (_, mandela) = t.get("/v1/places?q=nelson%20mandela", &r.access).await;
    assert!(names(&mandela).iter().any(|n| n.contains("Nelson Mandela")), "{mandela}");
    assert!(mandela.as_array().unwrap().iter().all(|p| p["kind"] == "street"));
    let (_, accented) = t.get("/v1/places?q=M%C3%A1erua", &r.access).await;
    assert_eq!(accented[0]["name"], "Maerua Mall");
    let (_, numbered) = t.get("/v1/places?q=5%20sam%20nujoma", &r.access).await;
    assert!(names(&numbered).iter().any(|n| n.starts_with("Sam Nujoma")), "{numbered}");
    // Landmarks are also found by kind ("hospital").
    let (_, hospitals) = t.get("/v1/places?q=hospital", &r.access).await;
    assert!(names(&hospitals).contains(&"Katutura State Hospital".to_owned()));

    // Naming the pickup pin.
    let (status, pin) = t.get(&format!("/v1/places/reverse?lat={}&lon={}", MAERUA_MALL.0, MAERUA_MALL.1), &r.access).await;
    assert_eq!(status, StatusCode::OK, "{pin}");
    assert!(!pin["address"].as_str().unwrap().is_empty());
    assert!(!pin["area"].as_str().unwrap().is_empty());
    let (status, _) = t.get("/v1/places/reverse?lat=-33.9&lon=18.4", &r.access).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = t.get(&format!("/v1/places?q={}", "a".repeat(81)), &r.access).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_quoted_price_is_exactly_the_price_charged() {
    let t = setup!(relaxed());
    let r = t.rider().await;
    let q = t.quote(&r, POST_ST_MALL, MAERUA_MALL).await;
    assert_eq!(q["currency"], "NAD");
    assert_eq!(q["routing"], "estimate");
    let opts = q["options"].as_array().unwrap();
    let totals: Vec<i64> = opts.iter().map(|o| o["fare"]["total"].as_i64().unwrap()).collect();
    assert!(totals[0] < totals[1] && totals[1] < totals[2], "{totals:?}");
    // Nobody is online: "No driver online within 3 km".
    assert!(opts.iter().all(|o| o["pickup_eta_s"].is_null()));

    let k = key();
    let body = json!({ "quote": q["quote"], "tier": "comfort", "passengers": 2, "payment_method": "cash" });
    let (status, ride) = t.request(&r, body.clone(), &k).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    assert_eq!(ride["state"], "SEARCHING");
    assert_eq!(ride["fare"], opts[1]["fare"], "charged fare = quoted Comfort fare");
    assert!(ride["code"].as_str().unwrap().starts_with("FU-"));
    assert_eq!(ride["pin"].as_str().unwrap().len(), 4);

    // A retry with the same key returns the same ride instead of booking twice.
    let (status, again) = t.request(&r, body, &k).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["id"], ride["id"]);
    // A new request while one is active is refused.
    let q2 = t.quote(&r, POST_ST_MALL, MAERUA_MALL).await;
    let (status, e) =
        t.request(&r, json!({ "quote": q2["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash" }), &key()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(e["error"]["code"], "ride_in_progress");
    let (status, active) = t.get("/v1/rides/active", &r.access).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(active["id"], ride["id"]);
}

#[tokio::test]
async fn quotes_cannot_be_forged_reused_by_others_or_skipped() {
    let t = setup!(relaxed());
    let r = t.rider().await;
    let q = t.quote(&r, POST_ST_MALL, MAERUA_MALL).await;
    let token = q["quote"].as_str().unwrap();
    let req = |quote: &str| json!({ "quote": quote, "tier": "classic", "passengers": 1, "payment_method": "cash" });

    // Tampered payload (cheaper distance) fails the signature.
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut claims: Value = serde_json::from_slice(&b64.decode(&parts[1]).unwrap()).unwrap();
    claims["m"] = json!(100);
    parts[1] = b64.encode(claims.to_string());
    let (status, e) = t.request(&r, req(&parts.join(".")), &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");

    // Another rider can't use it.
    let other = t.rider().await;
    let (status, _) = t.request(&other, req(token), &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // An access token is not a quote.
    let (status, _) = t.request(&r, req(&r.access), &key()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Card payments wait for Phase 4; an Idempotency-Key is required.
    let mut card = req(token);
    card["payment_method"] = json!("card");
    assert_eq!(t.request(&r, card, &key()).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(t.post("/v1/rides", &r.access, req(token)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    // Unverified riders can't book.
    let unverified = t.onboarded("rider").await;
    let q3 = t.quote(&unverified, POST_ST_MALL, MAERUA_MALL).await;
    let (status, e) = t.request(&unverified, req(q3["quote"].as_str().unwrap()), &key()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(e["error"]["code"], "not_verified");
}

#[tokio::test]
async fn best_driver_is_asked_first_then_the_next_and_the_accepting_driver_never_sees_the_destination() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let near = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let further = t.online_driver("Selma Iipinge", "classic", FURTHER).await;
    let _far = t.online_driver("Martha Kandjii", "classic", FAR_AWAY).await;
    let _comfort = t.online_driver("Ndapewa Haufiku", "comfort", NEAR).await;

    // Live ETA on the options screen once drivers are online.
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    assert!(q["options"][0]["pickup_eta_s"].as_i64().unwrap() > 0);
    assert!(q["options"][2]["pickup_eta_s"].is_null(), "no Soft Life driver online");

    let ride = t.book(&rider).await;
    let ride_id = ride["id"].as_str().unwrap();
    assert_eq!(t.tick().await.offered, 1);

    // The nearest Classic driver gets the card; nobody else has one.
    let card = &t.driver_status(&near).await["offer"];
    assert_eq!(card["ride_id"], ride_id);
    assert!(card["seconds_left"].as_i64().unwrap() > 10);
    assert_eq!(card["passenger"]["name"], "Ndeshi");
    assert_eq!(card["earning"], ride["fare"]["driver_earning"]);
    assert_eq!(card["pickup_area"]["radius_m"], 250);
    let text = card.to_string();
    for secret in ["Maerua", "\"pin\"", "dropoff", "\"pickup\""] {
        assert!(!text.contains(secret), "offer card leaks {secret}: {text}");
    }
    assert!(t.driver_status(&further).await["offer"].is_null());

    // She declines; the next tick asks the next-best driver.
    let offer = card["offer_id"].as_str().unwrap();
    assert_eq!(t.post(&format!("/v1/offers/{offer}/decline"), &near.access, json!({})).await.0, StatusCode::NO_CONTENT);
    assert_eq!(t.tick().await.offered, 1);
    let card = t.driver_status(&further).await["offer"].clone();
    assert_eq!(card["ride_id"], ride_id);
    let (status, view) = t.post(&format!("/v1/offers/{}/accept", card["offer_id"].as_str().unwrap()), &further.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], "DRIVER_EN_ROUTE");
    assert!(view["pickup"].is_null() && view["dropoff"].is_null(), "{view}");
    let text = view.to_string();
    assert!(!text.contains("Maerua") && !text.contains("\"pin\""), "{text}");

    // The rider sees who is coming.
    let (_, mine) = t.get(&format!("/v1/rides/{ride_id}"), &rider.access).await;
    assert_eq!(mine["state"], "DRIVER_EN_ROUTE");
    assert_eq!(mine["driver"]["first_name"], "Selma");
    assert!(mine["driver"]["vehicle"]["plate"].as_str().unwrap().starts_with("N "));
    assert!(mine["driver"]["eta_s"].as_i64().is_some());

    // The destination stays hidden in every state before PIN_VERIFIED, and appears after.
    let dest_visible = |state: &str| {
        let t = &t;
        let id = ride_id.to_owned();
        let s = state.to_owned();
        let d = further.access.clone();
        async move {
            sqlx::query("UPDATE rides SET state = $2::ride_state WHERE id = $1::uuid")
                .bind(&id)
                .bind(&s)
                .execute(&t.state.db)
                .await
                .unwrap();
            let v = t.driver_status_with(&d).await;
            !v["ride"]["dropoff"].is_null()
        }
    };
    for s in ["DRIVER_ASSIGNED", "DRIVER_EN_ROUTE", "DRIVER_ARRIVED"] {
        assert!(!dest_visible(s).await, "destination visible in {s}");
    }
    for s in ["PIN_VERIFIED", "IN_TRIP"] {
        assert!(dest_visible(s).await, "destination hidden in {s}");
    }
}

impl T {
    async fn driver_status_with(&self, access: &str) -> Value {
        self.get("/v1/driver/status", access).await.1
    }
}

#[tokio::test]
async fn unanswered_requests_move_on_and_no_drivers_left_can_be_retried() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let d = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let ride = t.book(&rider).await;
    let id = ride["id"].as_str().unwrap();
    assert_eq!(t.tick().await.offered, 1);
    let before: f64 = sqlx::query_scalar("SELECT acceptance_rate::float8 FROM driver_profiles WHERE user_id = $1")
        .bind(d.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();

    // 15 s pass without an answer.
    t.expire_offer(id).await;
    let tick = t.tick().await;
    assert_eq!((tick.expired, tick.no_drivers), (1, 1));
    assert_eq!(t.ride_state(id).await, "NO_DRIVERS");
    let after: f64 = sqlx::query_scalar("SELECT acceptance_rate::float8 FROM driver_profiles WHERE user_id = $1")
        .bind(d.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert!(after < before, "letting a request expire lowers acceptance");

    // Try again: a new round, so the same driver can be asked again.
    let (status, again) = t.post(&format!("/v1/rides/{id}/retry"), &rider.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["state"], "SEARCHING");
    assert_eq!(t.tick().await.offered, 1);
    assert_eq!(t.driver_status(&d).await["offer"]["ride_id"], id);
    // Retry is only for NO_DRIVERS.
    assert_eq!(t.post(&format!("/v1/rides/{id}/retry"), &rider.access, json!({})).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn twenty_drivers_accepting_the_same_ride_yield_exactly_one_winner() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let mut drivers = Vec::new();
    for i in 0..20 {
        let p = (NEAR.0 + i as f64 * 0.0004, NEAR.1);
        drivers.push(t.online_driver(&format!("Driver {i}"), "classic", p).await);
    }
    let ride = t.book(&rider).await;
    let id = ride["id"].as_str().unwrap();
    assert_eq!(t.tick().await.offered, 1);
    let offer: Uuid = sqlx::query_scalar("SELECT id FROM ride_offers WHERE ride_id = $1::uuid AND response = 'pending'")
        .bind(id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();

    // Every driver fires an accept at once, and the offered one fires twenty more.
    let offered: Uuid =
        sqlx::query_scalar("SELECT driver_id FROM ride_offers WHERE id = $1").bind(offer).fetch_one(&t.state.db).await.unwrap();
    let holder = drivers.iter().find(|d| d.user_id == offered).unwrap().access.clone();
    let mut tokens: Vec<String> = drivers.iter().map(|d| d.access.clone()).collect();
    tokens.extend(std::iter::repeat_n(holder.clone(), 20));
    let app = t.app.clone();
    let tasks: Vec<_> = tokens
        .into_iter()
        .map(|tok| {
            let app = app.clone();
            tokio::spawn(async move {
                use tower::ServiceExt;
                let req = axum::http::Request::post(format!("/v1/offers/{offer}/accept"))
                    .header("authorization", format!("Bearer {tok}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap();
                app.oneshot(req).await.unwrap().status()
            })
        })
        .collect();
    let mut statuses = Vec::new();
    for task in tasks {
        statuses.push(task.await.unwrap());
    }
    let wins = statuses.iter().filter(|s| **s == StatusCode::OK).count();
    assert!(wins <= 1, "{statuses:?}");
    // Everyone else: not their offer (404), already taken (409) or no connection free in time
    // (503, on a slow machine). Never a 500.
    let answers = [StatusCode::OK, StatusCode::NOT_FOUND, StatusCode::CONFLICT, StatusCode::SERVICE_UNAVAILABLE];
    assert!(statuses.iter().all(|s| answers.contains(s)), "{statuses:?}");
    // If the database turned away every try that could have won, one more wins.
    let taken: Option<Uuid> =
        sqlx::query_scalar("SELECT driver_id FROM rides WHERE id = $1::uuid").bind(id).fetch_one(&t.state.db).await.unwrap();
    if taken.is_none() {
        let uri = format!("/v1/offers/{offer}/accept");
        assert_eq!(t.call(Method::POST, &uri, Some(&holder), Some(json!({}))).await.0, StatusCode::OK);
    }
    let (assigned, events): (Uuid, i64) = sqlx::query_as(
        "SELECT driver_id, (SELECT count(*) FROM ride_events WHERE ride_id = r.id AND to_state = 'DRIVER_ASSIGNED') FROM rides r WHERE id = $1::uuid",
    )
    .bind(id)
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    assert_eq!(assigned, offered);
    assert_eq!(events, 1);
}

#[tokio::test]
async fn a_driver_on_a_trip_gets_no_new_requests_and_a_cancelled_ride_withdraws_its_offer() {
    let t = setup!(relaxed());
    let d = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let r1 = t.rider().await;
    let ride1 = t.book(&r1).await;
    t.tick().await;
    let offer = t.driver_status(&d).await["offer"]["offer_id"].as_str().unwrap().to_owned();
    assert_eq!(t.post(&format!("/v1/offers/{offer}/accept"), &d.access, json!({})).await.0, StatusCode::OK);

    // A second rider's request finds nobody: Hilma is busy.
    let r2 = t.rider().await;
    let ride2 = t.book(&r2).await;
    assert_eq!(t.tick().await.no_drivers, 1);
    assert_eq!(t.ride_state(ride2["id"].as_str().unwrap()).await, "NO_DRIVERS");

    // The first rider cancels after assignment: free, but it costs priority (5, then 10).
    let (status, c) = t.post(&format!("/v1/rides/{}/cancel", ride1["id"].as_str().unwrap()), &r1.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    assert_eq!(c["state"], "CANCELLED");
    assert_eq!(c["cancelled_by"], "rider");
    let points: i32 = sqlx::query_scalar("SELECT priority_points FROM rider_profiles WHERE user_id = $1")
        .bind(r1.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!(points, 95);
    assert!(t.driver_status(&d).await["ride"].is_null());

    // A cancel while a driver is looking at the request withdraws it.
    let (_, retried) = t.post(&format!("/v1/rides/{}/retry", ride2["id"].as_str().unwrap()), &r2.access, json!({})).await;
    assert_eq!(retried["state"], "SEARCHING");
    t.tick().await;
    let offer = t.driver_status(&d).await["offer"]["offer_id"].as_str().unwrap().to_owned();
    t.post(&format!("/v1/rides/{}/cancel", ride2["id"].as_str().unwrap()), &r2.access, json!({})).await;
    assert!(t.driver_status(&d).await["offer"].is_null());
    let (status, e) = t.post(&format!("/v1/offers/{offer}/accept"), &d.access, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(e["error"]["code"], "offer_gone");
}

#[tokio::test]
async fn only_approved_drivers_signed_in_on_a_phone_receive_requests() {
    let t = setup!(relaxed());
    // Not approved: can't go online.
    let pending = t.sign_in(&new_phone(), "driver").await;
    let (status, e) = t.post("/v1/driver/online", &pending.access, json!({ "at": at(NEAR) })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{e}");
    assert_eq!(e["error"]["code"], "not_approved");

    // Wheelchair requests go only to accessible vehicles.
    let rider = t.rider().await;
    let ordinary = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let accessible = t.approved_driver("Selma Iipinge", "classic", true).await;
    t.post("/v1/driver/online", &accessible.access, json!({ "at": at(FURTHER) })).await;
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let (status, ride) = t
        .request(
            &rider,
            json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "wheelchair": true, "payment_method": "cash" }),
            &key(),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    t.tick().await;
    assert!(t.driver_status(&ordinary).await["offer"].is_null());
    assert_eq!(t.driver_status(&accessible).await["offer"]["wheelchair"], true);
    t.post(&format!("/v1/rides/{}/cancel", ride["id"].as_str().unwrap()), &rider.access, json!({})).await;

    // Signing in on another phone moves her: the old session no longer counts as online.
    let phone: String =
        sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = $1").bind(ordinary.user_id).fetch_one(&t.state.db).await.unwrap();
    let second = t.sign_in(&phone, "driver").await;
    assert!(!t.driver_status(&second).await["online"].as_bool().unwrap());
    assert_eq!(t.call(Method::GET, "/v1/driver/status", Some(&ordinary.access), None).await.0, StatusCode::UNAUTHORIZED);
    let found = fuber_db::presence::candidates(
        &t.state.db,
        &fuber_db::presence::Search {
            lat: POST_ST_MALL.0,
            lon: POST_ST_MALL.1,
            tier: "classic",
            wheelchair: false,
            radius_m: 3000.0,
            stale_s: 60,
            exclude: &[],
            rider_id: None,
            limit: 10,
        },
    )
    .await
    .unwrap();
    assert!(found.iter().all(|c| c.driver_id != ordinary.user_id), "signed-out session still matchable");

    // Going online again from the new phone, then signing out, takes her offline.
    assert_eq!(t.post("/v1/driver/online", &second.access, json!({ "at": at(NEAR) })).await.0, StatusCode::OK);
    assert_eq!(t.call(Method::POST, "/v1/auth/logout", Some(&second.access), None).await.0, StatusCode::NO_CONTENT);
    let online: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM driver_presence WHERE driver_id = $1)")
        .bind(ordinary.user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert!(!online);
    let ended: Option<String> =
        sqlx::query_scalar("SELECT end_reason FROM driver_shifts WHERE driver_id = $1 ORDER BY started_at DESC LIMIT 1")
            .bind(ordinary.user_id)
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!(ended.as_deref(), Some("signed_out"));
}

#[tokio::test]
async fn scheduled_rides_start_searching_fifteen_minutes_before_pickup() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let when = |mins: i64| (OffsetDateTime::now_utc() + time::Duration::minutes(mins)).format(&Rfc3339).unwrap();
    let body =
        |at: String| json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash", "scheduled_for": at });
    // 30 min – 7 days ahead.
    assert_eq!(t.request(&rider, body(when(10)), &key()).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(t.request(&rider, body(when(60 * 24 * 8)), &key()).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, ride) = t.request(&rider, body(when(120)), &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    assert_eq!(ride["state"], "SCHEDULED");
    let id = ride["id"].as_str().unwrap();
    assert_eq!(t.tick().await.started, 0);

    sqlx::query("UPDATE rides SET scheduled_for = now() + interval '10 minutes' WHERE id = $1::uuid")
        .bind(id)
        .execute(&t.state.db)
        .await
        .unwrap();
    assert_eq!(t.tick().await.started, 1);
    assert_eq!(t.ride_state(id).await, "NO_DRIVERS", "searched straight away and found nobody");
}

#[tokio::test]
async fn a_guest_passenger_gets_her_driver_plate_and_pin_by_sms() {
    let t = setup!(relaxed());
    let rider = t.rider().await;
    let d = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let guest_phone = new_phone();
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let body = json!({
        "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash",
        "guest": { "name": "Ndapandula Shikongo", "phone": guest_phone },
    });
    let (status, ride) = t.request(&rider, body, &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    assert_eq!(ride["guest_name"], "Ndapandula Shikongo");
    t.tick().await;
    let card = t.driver_status(&d).await["offer"].clone();
    // Shown as a guest booked by a verified rider, not as "Verified" with the booker's rating.
    assert_eq!(card["passenger"]["name"], "Ndapandula Shikongo");
    assert_eq!(card["passenger"]["booked_by"], "booked by Ndeshi (verified)");
    assert!(card["passenger"]["rating"].is_null());
    t.post(&format!("/v1/offers/{}/accept", card["offer_id"].as_str().unwrap()), &d.access, json!({})).await;
    t.work().await;
    let sms = t.ctx.sms.outbox().into_iter().find(|m| m.to == guest_phone).expect("guest SMS");
    let plate: String =
        sqlx::query_scalar("SELECT plate FROM vehicles WHERE driver_id = $1").bind(d.user_id).fetch_one(&t.state.db).await.unwrap();
    assert!(sms.body.contains(&plate), "{}", sms.body);
    assert!(sms.body.contains(ride["pin"].as_str().unwrap()), "{}", sms.body);
    assert!(sms.body.contains("Hilma"), "{}", sms.body);
}

#[tokio::test]
async fn wrong_user_and_wrong_app_access_is_refused_on_every_phase_2_endpoint() {
    let t = setup!(relaxed());
    let owner = t.rider().await;
    let other = t.rider().await;
    let d = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let other_driver = t.online_driver("Selma Iipinge", "classic", FURTHER).await;
    let ride = t.book(&owner).await;
    let id = ride["id"].as_str().unwrap();
    t.tick().await;
    let offer: Uuid =
        sqlx::query_scalar("SELECT id FROM ride_offers WHERE ride_id = $1::uuid").bind(id).fetch_one(&t.state.db).await.unwrap();
    let holder = if t.driver_status(&d).await["offer"].is_null() { &other_driver } else { &d };
    let bystander = if holder.user_id == d.user_id { &other_driver } else { &d };

    // Someone else's ride looks exactly like a missing one.
    for (m, path) in [
        (Method::GET, format!("/v1/rides/{id}")),
        (Method::POST, format!("/v1/rides/{id}/cancel")),
        (Method::POST, format!("/v1/rides/{id}/retry")),
    ] {
        let (status, _) = t.call(m, &path, Some(&other.access), Some(json!({}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    for path in [format!("/v1/offers/{offer}/accept"), format!("/v1/offers/{offer}/decline")] {
        assert_eq!(t.post(&path, &bystander.access, json!({})).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    // Rider tokens on driver routes and the other way round.
    let rider_routes = [
        (Method::GET, "/v1/places?q=mall".to_owned()),
        (Method::GET, "/v1/places/reverse?lat=-22.56&lon=17.08".to_owned()),
        (Method::POST, "/v1/rides/quote".to_owned()),
        (Method::POST, "/v1/rides".to_owned()),
        (Method::GET, "/v1/rides/active".to_owned()),
        (Method::GET, format!("/v1/rides/{id}")),
        (Method::POST, format!("/v1/rides/{id}/cancel")),
        (Method::POST, format!("/v1/rides/{id}/retry")),
    ];
    let driver_routes = [
        (Method::GET, "/v1/driver/status".to_owned()),
        (Method::POST, "/v1/driver/online".to_owned()),
        (Method::POST, "/v1/driver/offline".to_owned()),
        (Method::PUT, "/v1/driver/location".to_owned()),
        (Method::POST, format!("/v1/offers/{offer}/accept")),
        (Method::POST, format!("/v1/offers/{offer}/decline")),
    ];
    for (m, path) in &rider_routes {
        assert_eq!(t.call(m.clone(), path, Some(&holder.access), Some(json!({}))).await.0, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(t.call(m.clone(), path, None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    for (m, path) in &driver_routes {
        assert_eq!(t.call(m.clone(), path, Some(&owner.access), Some(json!({}))).await.0, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(t.call(m.clone(), path, None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    // The offer still stands after all that.
    assert_eq!(t.driver_status(holder).await["offer"]["offer_id"], offer.to_string());
}

#[tokio::test]
async fn validation_failures_on_every_phase_2_body() {
    let t = setup!(relaxed());
    let r = t.rider().await;
    let d = t.approved_driver("Hilma Nangolo", "classic", false).await;
    let good_quote = json!({ "pickup": at(POST_ST_MALL), "pickup_address": "Post Street Mall", "dropoff": at(MAERUA_MALL), "dropoff_address": "Maerua Mall" });
    let mut cases: Vec<(&str, &Session, Value)> = Vec::new();
    let with = |k: &str, v: Value| {
        let mut b = good_quote.clone();
        b[k] = v;
        b
    };
    cases.push(("/v1/rides/quote", &r, with("extra", json!(1))));
    cases.push(("/v1/rides/quote", &r, with("pickup", json!({ "lat": -33.9, "lon": 18.4 }))));
    cases.push(("/v1/rides/quote", &r, with("dropoff", at(POST_ST_MALL))));
    cases.push(("/v1/rides/quote", &r, with("pickup_address", json!(""))));
    cases.push(("/v1/rides/quote", &r, with("dropoff_address", json!("x".repeat(121)))));
    let q = t.quote(&r, POST_ST_MALL, MAERUA_MALL).await;
    let ride = |k: &str, v: Value| {
        let mut b = json!({ "quote": q["quote"], "tier": "classic", "passengers": 1, "payment_method": "cash" });
        b[k] = v;
        b
    };
    let ride_cases = [
        ride("passengers", json!(5)),
        ride("passengers", json!(0)),
        ride("tier", json!("helicopter")),
        ride("payment_method", json!("crypto")),
        ride("guest", json!({ "name": "Ndapandula", "phone": "+27821234567" })),
        ride("guest", json!({ "name": "", "phone": "+264811234567" })),
        ride("scheduled_for", json!("tomorrow")),
        ride("price", json!(1)),
    ];
    for b in ride_cases {
        let (status, e) = t.request(&r, b.clone(), &key()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{b} → {e}");
    }
    for (path, who, b) in cases {
        let (status, e) = t.post(path, &who.access, b.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path} {b} → {e}");
    }
    for b in [
        json!({ "at": { "lat": -33.9, "lon": 18.4 } }),
        json!({ "at": at(NEAR), "heading": 400 }),
        json!({ "at": at(NEAR), "speed_kmh": -1 }),
        json!({ "at": at(NEAR), "x": 1 }),
    ] {
        let (status, e) = t.post("/v1/driver/online", &d.access, b.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{b} → {e}");
    }
    // A bad Idempotency-Key.
    let (status, _) = t
        .call_with(Method::POST, "/v1/rides", Some(&r.access), Some(ride("tier", json!("classic"))), &[("idempotency-key", "bad key!")])
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------------------------
// Real-time updates over a real socket.

async fn serve(t: &T) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = fuber_api::router(t.state.clone());
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    tokio::spawn(fuber_api::live::listen(t.state.db.clone(), t.state.hub.clone()));
    addr
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: SocketAddr, token: &str) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws")).await.unwrap();
    ws.send(Message::text(json!({ "type": "auth", "data": { "token": token } }).to_string())).await.unwrap();
    assert_eq!(next(&mut ws).await["type"], "ready");
    ws
}

async fn next(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a message within 10 s").unwrap().unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// The next message of this type, skipping others.
async fn next_of(ws: &mut Ws, kind: &str) -> Value {
    loop {
        let m = next(ws).await;
        if m["type"] == kind {
            return m;
        }
    }
}

#[tokio::test]
async fn both_phones_see_request_offer_and_accept_live() {
    let t = setup!(relaxed());
    let addr = serve(&t).await;
    let rider = t.rider().await;
    let d = t.online_driver("Hilma Nangolo", "classic", NEAR).await;
    let mut rider_ws = connect(addr, &rider.access).await;
    let mut driver_ws = connect(addr, &d.access).await;
    // Give the listener a moment to subscribe.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let ride = t.book(&rider).await;
    assert_eq!(next_of(&mut rider_ws, "ride.updated").await["data"]["state"], "SEARCHING");
    t.tick().await;
    let offer = next_of(&mut driver_ws, "offer.created").await;
    assert_eq!(offer["data"]["ride_id"], ride["id"]);
    assert!(!offer.to_string().contains("Maerua"));

    // Her position over the socket; accept over HTTPS.
    driver_ws
        .send(Message::text(json!({ "type": "location.update", "data": { "at": at(NEAR), "heading": 90 } }).to_string()))
        .await
        .unwrap();
    let (status, _) = t.post(&format!("/v1/offers/{}/accept", offer["data"]["offer_id"].as_str().unwrap()), &d.access, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let update = loop {
        let m = next_of(&mut rider_ws, "ride.updated").await;
        if m["data"]["state"] == "DRIVER_EN_ROUTE" {
            break m;
        }
    };
    assert_eq!(update["data"]["driver"]["first_name"], "Hilma");
    let mine = next_of(&mut driver_ws, "ride.updated").await;
    assert!(mine["data"]["dropoff"].is_null());
    assert!(!mine.to_string().contains("\"pin\""));

    // Positions now reach the rider.
    driver_ws.send(Message::text(json!({ "type": "location.update", "data": { "at": at(NEAR) } }).to_string())).await.unwrap();
    assert_eq!(next_of(&mut rider_ws, "driver.location").await["data"]["ride_id"], ride["id"]);

    // A rider's socket can't send driver positions; flooding closes the socket.
    rider_ws.send(Message::text(json!({ "type": "location.update", "data": { "at": at(NEAR) } }).to_string())).await.unwrap();
    assert_eq!(next_of(&mut rider_ws, "error").await["data"]["code"], "bad_message");
    for _ in 0..30 {
        let _ = driver_ws.send(Message::text(json!({ "type": "ping" }).to_string())).await;
    }
    assert_eq!(next_of(&mut driver_ws, "error").await["data"]["code"], "rate_limited");
}

#[tokio::test]
async fn a_chat_message_reaches_both_phones_live() {
    let t = setup!(relaxed());
    let addr = serve(&t).await;
    let trip = t.accepted().await;
    let mut rider_ws = connect(addr, &trip.rider.access).await;
    let mut driver_ws = connect(addr, &trip.driver.access).await;
    // Give the listener a moment to subscribe.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let k = key();
    let (status, sent) = t
        .call_with(
            Method::POST,
            &format!("/v1/rides/{}/messages", trip.id),
            Some(&trip.driver.access),
            Some(json!({ "quick_reply": "two_minutes" })),
            &[("idempotency-key", k.as_str())],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    for ws in [&mut rider_ws, &mut driver_ws] {
        let pushed = next_of(ws, "chat.message").await;
        assert_eq!(pushed["data"], sent);
        assert_eq!(pushed["data"]["text"], "I'm 2 minutes away");
    }
}

#[tokio::test]
async fn sockets_need_a_valid_token_in_the_first_message() {
    let t = setup!(relaxed());
    let addr = serve(&t).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws")).await.unwrap();
    ws.send(Message::text(json!({ "type": "auth", "data": { "token": "not-a-token" } }).to_string())).await.unwrap();
    assert_eq!(next(&mut ws).await["data"]["code"], "unauthorized");
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws")).await.unwrap();
    ws.send(Message::text(json!({ "type": "ping" }).to_string())).await.unwrap();
    assert_eq!(next(&mut ws).await["data"]["code"], "unauthorized");
}

// ---------------------------------------------------------------------------------------------

/// Capacity (TRD §6): 500 online drivers and a burst of 50 requests. Ignored by default;
/// `cargo test -p fuber-api --test dispatch -- --ignored --nocapture` prints the timings.
#[tokio::test]
#[ignore = "load test: run explicitly"]
async fn load_500_drivers_and_50_requests() {
    let t = setup!(relaxed());
    let db = &t.state.db;
    let started = std::time::Instant::now();
    // Drivers inserted directly (sign-in per driver would dominate the run).
    sqlx::query(
        "WITH d AS (
           INSERT INTO users (phone_e164, full_name, display_name, status)
           SELECT '+26485' || lpad(g::text, 7, '0'), 'Load Driver ' || g, 'Driver' || g, 'active' FROM generate_series(1, 500) g
           RETURNING id),
         p AS (INSERT INTO driver_profiles (user_id, application_status, can_go_online) SELECT id, 'approved', true FROM d RETURNING user_id),
         v AS (INSERT INTO vehicles (driver_id, make, model, colour, year, plate, approved)
               SELECT user_id, 'Toyota', 'Corolla', 'White', 2020, 'N ' || (100000 + row_number() OVER ()) || ' LD', true FROM p RETURNING id, driver_id),
         -- Not chained through an UPDATE of driver_profiles: it cannot see rows inserted by `p`
         -- in the same statement, so it would match none (active_vehicle_id is set below).
         tr AS (INSERT INTO driver_tiers (driver_id, tier) SELECT driver_id, 'classic' FROM v RETURNING driver_id),
         s AS (INSERT INTO sessions (user_id, app, family_id, refresh_hash, expires_at)
               SELECT driver_id, 'driver', gen_random_uuid(), sha256(gen_random_uuid()::text::bytea), now() + interval '1 day' FROM tr RETURNING id, user_id),
         sh AS (INSERT INTO driver_shifts (driver_id, vehicle_id, tier)
                SELECT s.user_id, v.id, 'classic' FROM s JOIN v ON v.driver_id = s.user_id RETURNING id, driver_id, vehicle_id)
         INSERT INTO driver_presence (driver_id, session_id, shift_id, vehicle_id, tiers, wheelchair, location)
         SELECT sh.driver_id, s.id, sh.id, sh.vehicle_id, '{classic}', false,
                ST_SetSRID(ST_MakePoint(17.03 + random() * 0.10, -22.62 + random() * 0.10), 4326)::geography
         FROM sh JOIN s ON s.user_id = sh.driver_id",
    )
    .execute(db)
    .await
    .unwrap();
    sqlx::query("UPDATE driver_profiles dp SET active_vehicle_id = v.id FROM vehicles v WHERE dp.user_id = v.driver_id")
        .execute(db)
        .await
        .unwrap();
    let online: i64 = sqlx::query_scalar("SELECT count(*) FROM driver_presence").fetch_one(db).await.unwrap();
    assert_eq!(online, 500);
    println!("500 drivers online in {:?}", started.elapsed());

    let mut riders = Vec::new();
    for _ in 0..50 {
        riders.push(t.rider().await);
    }
    let started = std::time::Instant::now();
    for r in &riders {
        t.book(r).await;
    }
    println!("50 quotes + requests in {:?} ({:?} each)", started.elapsed(), started.elapsed() / 50);
    let started = std::time::Instant::now();
    let tick = t.tick().await;
    let took = started.elapsed();
    println!("matching tick for 50 rides over 500 drivers: {took:?} → {tick:?}");
    assert_eq!(tick.offered + tick.no_drivers, 50);
    assert!(tick.offered >= 45, "{tick:?}");
    // Offers expire after 15 s; a tick must stay well inside one second at this load.
    assert!(took < Duration::from_secs(5), "tick took {took:?}");
}
