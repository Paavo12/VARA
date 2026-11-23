//! Ops pricing, surge and audit log tests (PRD O-5, O-7): new rates are a new version that
//! quotes use from its start, locked quotes keep their old rates, the service fee stays 15–18 %,
//! surge zones change within 1.00–3.00, every change is in the audit log the admin can page
//! through, and only admins can do any of it.
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

#[tokio::test]
async fn new_rates_are_a_version_that_new_quotes_use() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let rider = t.rider().await;
    let before = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let count_classic = "SELECT count(*) FROM pricing_tiers WHERE tier = 'classic'";
    let versions_before: i64 = sqlx::query_scalar(count_classic).fetch_one(&t.state.db).await.unwrap();

    let (status, list) = t.call(Method::GET, "/v1/ops/pricing", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let current: Vec<&Value> = list.as_array().unwrap().iter().filter(|r| r["current"] == true).collect();
    assert_eq!(current.len(), 3, "one in force per ride type");

    let rates = json!({ "tier": "classic", "base_cents": 3000, "per_km_cents": 1600, "per_min_cents": 240,
                        "minimum_cents": 7000, "service_fee_bp": 1500 });
    let (status, added) = t.call(Method::POST, "/v1/ops/pricing", Some(&admin), Some(rates.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    assert_eq!(added["current"], true);
    let after = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    assert!(
        classic(&after)["fare"]["total"].as_i64().unwrap() > classic(&before)["fare"]["total"].as_i64().unwrap(),
        "new quotes use the new rates"
    );
    // The old version is still there for rides already priced with it.
    let versions: i64 = sqlx::query_scalar(count_classic).fetch_one(&t.state.db).await.unwrap();
    assert_eq!(versions, versions_before + 1, "a new version, the old ones untouched");

    // Bounds: the fee stays 15–18 %, rates are sane, nothing starts in the past.
    for bad in [
        json!({ "tier": "classic", "base_cents": 1500, "per_km_cents": 800, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 3200 }),
        json!({ "tier": "classic", "base_cents": -1, "per_km_cents": 800, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 1500 }),
        json!({ "tier": "classic", "base_cents": 1500, "per_km_cents": 0, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 1500 }),
        json!({ "tier": "classic", "base_cents": 1500, "per_km_cents": 800, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 1500,
                "effective_from": "2020-01-01T00:00:00Z" }),
        json!({ "tier": "taxi", "base_cents": 1500, "per_km_cents": 800, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 1500 }),
    ] {
        assert_eq!(
            t.call(Method::POST, "/v1/ops/pricing", Some(&admin), Some(bad.clone())).await.0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}"
        );
    }

    // Rates scheduled for later are listed but not used yet.
    let later = json!({ "tier": "comfort", "base_cents": 9000, "per_km_cents": 5000, "per_min_cents": 900, "minimum_cents": 20000,
                        "service_fee_bp": 1650, "effective_from": "2099-01-01T00:00:00Z" });
    let (status, scheduled) = t.call(Method::POST, "/v1/ops/pricing", Some(&admin), Some(later.clone())).await;
    assert_eq!((status, scheduled["current"].as_bool()), (StatusCode::CREATED, Some(false)));
    assert_eq!(t.call(Method::POST, "/v1/ops/pricing", Some(&admin), Some(later)).await.0, StatusCode::CONFLICT);
    let (_, list) = t.call(Method::GET, "/v1/ops/pricing", Some(&admin), None).await;
    assert!(list.as_array().unwrap().iter().any(|r| r["tier"] == "comfort" && r["current"] == false));
}

#[tokio::test]
async fn surge_zones_change_within_bounds() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO surge_zones (name, area) VALUES ('Test CBD',
           ST_Multi(ST_Buffer(ST_SetSRID(ST_MakePoint(17.08126, -22.56665), 4326)::geography, 500)::geometry)::geography)
         RETURNING id",
    )
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    let uri = format!("/v1/ops/surge-zones/{id}");

    let (status, zone) = t.call(Method::PUT, &uri, Some(&admin), Some(json!({ "multiplier": 1.5, "demand_level": "high" }))).await;
    assert_eq!(status, StatusCode::OK, "{zone}");
    assert_eq!((zone["multiplier"].as_f64(), zone["demand_level"].as_str()), (Some(1.5), Some("high")));
    let rider = t.rider().await;
    let quote = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    assert!(classic(&quote)["fare"]["surge"].as_i64().unwrap() > 0, "a pickup in the zone is surged: {quote}");

    for bad in [
        json!({ "multiplier": 3.5, "demand_level": "high" }),
        json!({ "multiplier": 0.5, "demand_level": "high" }),
        json!({ "multiplier": 1.2, "demand_level": "crazy" }),
    ] {
        assert_eq!(t.call(Method::PUT, &uri, Some(&admin), Some(bad.clone())).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let unknown = format!("/v1/ops/surge-zones/{}", uuid::Uuid::new_v4());
    assert_eq!(
        t.call(Method::PUT, &unknown, Some(&admin), Some(json!({ "multiplier": 1.2, "demand_level": "high" }))).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, zones) = t.call(Method::GET, "/v1/ops/surge-zones", Some(&admin), None).await;
    assert!(zones.as_array().unwrap().iter().any(|z| z["id"] == id.to_string()));
}

#[tokio::test]
async fn admins_page_through_the_audit_log() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    for v in [3500.0, 4000.0, 4500.0] {
        let put = t.call(Method::PUT, "/v1/ops/settings/dispatch.radius_m", Some(&admin), Some(json!({ "value": v }))).await;
        assert_eq!(put.0, StatusCode::OK);
    }
    let (status, page) = t.call(Method::GET, "/v1/ops/audit?action=setting.update", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let entries = page.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["detail"]["to"].as_f64(), Some(4500.0), "newest first");
    assert!(entries[0]["staff_email"].is_string());

    let last = entries[1]["id"].as_i64().unwrap();
    let (_, older) = t.call(Method::GET, &format!("/v1/ops/audit?action=setting.&before={last}"), Some(&admin), None).await;
    assert_eq!(older.as_array().unwrap().len(), 1, "prefix filter and paging");
    // Reading the log is audited too.
    let (_, reads) = t.call(Method::GET, "/v1/ops/audit?action=audit.view", Some(&admin), None).await;
    assert!(!reads.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn only_admins_change_prices_or_read_the_audit_log() {
    let t = setup!(relaxed());
    let rates = json!({ "tier": "classic", "base_cents": 1500, "per_km_cents": 800, "per_min_cents": 120, "minimum_cents": 3500, "service_fee_bp": 1500 });
    for role in ["ops_reviewer", "ops_safety", "ops_support"] {
        let token = t.staff(role).await;
        assert_eq!(t.call(Method::GET, "/v1/ops/pricing", Some(&token), None).await.0, StatusCode::FORBIDDEN, "{role}");
        assert_eq!(t.call(Method::POST, "/v1/ops/pricing", Some(&token), Some(rates.clone())).await.0, StatusCode::FORBIDDEN, "{role}");
        assert_eq!(t.call(Method::GET, "/v1/ops/surge-zones", Some(&token), None).await.0, StatusCode::FORBIDDEN, "{role}");
        assert_eq!(t.call(Method::GET, "/v1/ops/audit", Some(&token), None).await.0, StatusCode::FORBIDDEN, "{role}");
    }
    assert_eq!(t.call(Method::GET, "/v1/ops/audit", None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_launch_promo_charges_13_percent_for_90_days_then_stops_by_itself() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;
    let support = t.staff("ops_support").await;
    let rider = t.rider().await;
    let driver = t.online_driver("Hilma Nangolo", "comfort", NEAR).await;
    let uri = "/v1/ops/pricing/launch-promo";
    let rfc = |t: time::OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap();

    // Nothing set: no promotion.
    let (status, view) = t.call(Method::GET, uri, Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!((view["starts_at"].is_null(), view["active"].as_bool(), view["fee_bp"].as_i64()), (true, Some(false), Some(1300)));

    // Admins only, never in the past.
    let soon = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
    assert_eq!(t.call(Method::PUT, uri, Some(&support), Some(json!({ "starts_at": rfc(soon) }))).await.0, StatusCode::FORBIDDEN);
    let past = json!({ "starts_at": "2020-01-01T00:00:00Z" });
    assert_eq!(t.call(Method::PUT, uri, Some(&admin), Some(past)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, view) = t.call(Method::PUT, uri, Some(&admin), Some(json!({ "starts_at": rfc(soon) }))).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["active"], false, "it starts at the launch, not before");
    assert!(view["ends_at"].is_string(), "{view}");

    // Launched a day ago: a Comfort ride (16.5 %) pays 13 % at the offer and when booked.
    sqlx::query("UPDATE settings SET value = jsonb_build_object('starts_at', now() - interval '1 day') WHERE key = 'pricing.launch_promo'")
        .execute(&t.state.db)
        .await
        .unwrap();
    assert_eq!(t.call(Method::GET, uri, Some(&admin), None).await.1["active"], true);
    let q = t.quote(&rider, POST_ST_MALL, MAERUA_MALL).await;
    let (status, ride) =
        t.request(&rider, json!({ "quote": q["quote"], "tier": "comfort", "passengers": 1, "payment_method": "cash" }), &key()).await;
    assert_eq!(status, StatusCode::CREATED, "{ride}");
    let fare = ride["fare"]["total"].as_i64().unwrap();
    let promo_fee = (fare * 1300 + 5000) / 10_000;
    assert_eq!(t.tick().await.offered, 1);
    let card = t.driver_status(&driver).await["offer"].clone();
    assert_eq!(card["earning"].as_i64().unwrap(), fare - promo_fee);
    let (status, view) = t.post(&format!("/v1/offers/{}/accept", card["offer_id"].as_str().unwrap()), &driver.access, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let (fee, earning): (i64, i64) = sqlx::query_as("SELECT service_fee_cents, driver_earning_cents FROM rides WHERE id = $1::uuid")
        .bind(ride["id"].as_str().unwrap())
        .fetch_one(&t.state.db)
        .await
        .unwrap();
    assert_eq!((fee, earning), (promo_fee, fare - promo_fee));

    // Once started, its dates are fixed.
    let (status, e) = t.call(Method::PUT, uri, Some(&admin), Some(json!({ "starts_at": null }))).await;
    assert_eq!((status, e["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("promo_started")));

    // Day 90 is the last day; on day 91 the tier fees are back with no one doing anything.
    let active_after = |days: i32| {
        let db = t.state.db.clone();
        async move {
            sqlx::query(&format!(
                "UPDATE settings SET value = jsonb_build_object('starts_at', now() - interval '{days} days') WHERE key = 'pricing.launch_promo'"
            ))
            .execute(&db)
            .await
            .unwrap();
            sqlx::query_scalar::<_, bool>("SELECT launch_promo_active()").fetch_one(&db).await.unwrap()
        }
    };
    assert!(active_after(89).await);
    assert!(!active_after(91).await);

    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE action = 'pricing.launch_promo'").fetch_all(&t.state.db).await.unwrap();
    assert_eq!(actions.len(), 1, "the one successful change is audited");
}
