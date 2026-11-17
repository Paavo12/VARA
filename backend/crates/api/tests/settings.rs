//! Ops settings tests (PRD D-6, O-5): admins change matching and RideCheck parameters within
//! bounds, the change is audited and used by the next matching run, a minimum can't pass its
//! maximum, and other roles can't change anything.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#[macro_use]
mod common;

use axum::http::{Method, StatusCode};
use fuber_db::{settings::Dispatch, sqlx};
use serde_json::json;

use common::*;

#[tokio::test]
async fn admins_change_parameters_within_bounds_and_it_is_audited() {
    let t = setup!(relaxed());
    let admin = t.staff("ops_admin").await;

    let (status, list) = t.call(Method::GET, "/v1/ops/settings", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let radius = list.as_array().unwrap().iter().find(|s| s["key"] == "dispatch.radius_m").unwrap();
    assert_eq!(radius["value"], 3000.0);

    let (status, saved) = t.call(Method::PUT, "/v1/ops/settings/dispatch.radius_m", Some(&admin), Some(json!({ "value": 4500 }))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(Dispatch::load(&t.state.db).await.unwrap().radius_m, 4500.0, "matching reads it on its next run");
    let detail: serde_json::Value =
        sqlx::query_scalar("SELECT detail FROM audit_log WHERE action = 'setting.update' AND target_id = 'dispatch.radius_m'")
            .fetch_one(&t.state.db)
            .await
            .unwrap();
    assert_eq!((detail["from"].as_f64(), detail["to"].as_f64()), (Some(3000.0), Some(4500.0)));

    for (key, value) in [("dispatch.radius_m", 100.0), ("dispatch.offer_ms", 1_000_000.0), ("safety.deviation_m", 5.0)] {
        let (status, e) = t.call(Method::PUT, &format!("/v1/ops/settings/{key}"), Some(&admin), Some(json!({ "value": value }))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{key}: {e}");
    }
    assert_eq!(
        t.call(Method::PUT, "/v1/ops/settings/pricing.base", Some(&admin), Some(json!({ "value": 1 }))).await.0,
        StatusCode::NOT_FOUND
    );
    // The fuzzy pickup minimum can't pass its maximum (200 m).
    let (status, e) = t.call(Method::PUT, "/v1/ops/settings/dispatch.fuzz_min_m", Some(&admin), Some(json!({ "value": 300 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(
        t.call(Method::PUT, "/v1/ops/settings/dispatch.fuzz_max_m", Some(&admin), Some(json!({ "value": 400 }))).await.0,
        StatusCode::OK
    );
    assert_eq!(
        t.call(Method::PUT, "/v1/ops/settings/dispatch.fuzz_min_m", Some(&admin), Some(json!({ "value": 300 }))).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn only_admins_change_parameters() {
    let t = setup!(relaxed());
    for role in ["ops_reviewer", "ops_safety", "ops_support"] {
        let token = t.staff(role).await;
        assert_eq!(t.call(Method::GET, "/v1/ops/settings", Some(&token), None).await.0, StatusCode::FORBIDDEN, "{role}");
        let put = t.call(Method::PUT, "/v1/ops/settings/dispatch.radius_m", Some(&token), Some(json!({ "value": 4000 }))).await;
        assert_eq!(put.0, StatusCode::FORBIDDEN, "{role}");
    }
    assert_eq!(t.call(Method::GET, "/v1/ops/settings", None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn any_staff_role_sees_the_counts() {
    let t = setup!(relaxed());
    let _trip = t.completed().await;
    let reviewer = t.staff("ops_reviewer").await;
    let (status, m) = t.call(Method::GET, "/v1/ops/metrics?days=7", Some(&reviewer), None).await;
    assert_eq!(status, StatusCode::OK, "{m}");
    assert_eq!((m["requested"].as_i64(), m["completed"].as_i64()), (Some(1), Some(1)));
    assert_eq!(m["completion_rate"], 1.0);
    assert_eq!(m["drivers_online_now"], 1);
    assert!(m["median_match_min"].is_number());
    assert_eq!(t.call(Method::GET, "/v1/ops/metrics?days=0", Some(&reviewer), None).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(t.call(Method::GET, "/v1/ops/metrics", None, None).await.0, StatusCode::UNAUTHORIZED);
}
