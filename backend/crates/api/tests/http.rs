//! End-to-end tests of the HTTP layer without external services: rate limits run on the
//! in-memory store and the database pool is lazy (never connected).

use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use fuber_api::{
    AppState,
    rate_limit::{RateLimiter, RateLimits, Rule},
};
use fuber_common::Config;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const PEER: ([u8; 4], u16) = ([10, 0, 0, 7], 50_000);
const POST_ST_MALL: (f64, f64) = (-22.56665, 17.08126);
const MAERUA_MALL: (f64, f64) = (-22.58359, 17.09373);

fn config(extra: &[(&str, &str)]) -> Config {
    let base = [
        ("APP_ENV", "local"),
        ("DATABASE_URL", "postgres://fuber:fuber@127.0.0.1:1/fuber"),
        ("REDIS_URL", "redis://127.0.0.1:1"),
        ("HASH_SECRET", "test-hash-secret-0123456789abcdef0123"),
        ("DATA_KEY", "test-data-key-0123456789abcdef0123456"),
        ("JWT_SECRET", "test-jwt-secret-0123456789abcdef01234"),
        ("VIRUS_SCAN", "clamav"),
        ("CLAMAV_ADDR", "127.0.0.1:1"),
    ];
    Config::from_lookup(|k| extra.iter().chain(base.iter()).find(|(key, _)| *key == k).map(|(_, v)| v.to_string())).unwrap()
}

fn app_with(limits: RateLimits, cfg: Config) -> Router {
    let db = fuber_db::pool(&cfg.database_url).unwrap();
    let state = AppState::new(cfg, db, RateLimiter::memory_only(), limits);
    fuber_api::router(state).layer(MockConnectInfo(SocketAddr::from(PEER)))
}

fn app() -> Router {
    app_with(RateLimits::default(), config(&[]))
}

fn estimate_body(pickup: (f64, f64), dropoff: (f64, f64)) -> Value {
    json!({ "pickup": { "lat": pickup.0, "lon": pickup.1 }, "dropoff": { "lat": dropoff.0, "lon": dropoff.1 } })
}

fn post_json(uri: &str, body: &Value) -> Request<Body> {
    Request::post(uri).header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap()
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (parts.status, parts.headers, json)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn health_is_ok_with_hardening_headers() {
    let (status, headers, body) = send(&app(), get("/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(headers["cache-control"], "no-store");
    assert!(headers["content-security-policy"].to_str().unwrap().contains("default-src 'none'"));
    assert!(headers.contains_key("x-request-id"));
    // HSTS is only sent outside local development.
    assert!(!headers.contains_key("strict-transport-security"));
}

#[tokio::test]
async fn hsts_outside_local() {
    let cfg =
        config(&[("APP_ENV", "staging"), ("DATABASE_URL", "postgres://fuber:s3cret-from-vault@db/fuber"), ("REDIS_URL", "rediss://cache")]);
    let (_, headers, _) = send(&app_with(RateLimits::default(), cfg), get("/v1/health")).await;
    assert!(headers["strict-transport-security"].to_str().unwrap().contains("max-age="));
}

#[tokio::test]
async fn ready_reports_unavailable_database() {
    let (status, _, body) = send(&app(), get("/v1/ready")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["db"], "unavailable");
}

#[tokio::test]
async fn unknown_route_is_json_404() {
    let (status, _, body) = send(&app(), get("/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn estimate_prices_all_tiers() {
    let (status, _, body) = send(&app(), post_json("/v1/fares/estimate", &estimate_body(POST_ST_MALL, MAERUA_MALL))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["currency"], "NAD");
    let totals: Vec<i64> = body["estimates"].as_array().unwrap().iter().map(|e| e["fare"]["total"].as_i64().unwrap()).collect();
    assert_eq!(totals.len(), 3);
    assert!(totals[0] < totals[1] && totals[1] < totals[2], "tiers must be ordered by price: {totals:?}");
    assert!(totals[0] >= 3500, "at least the Classic minimum");
}

#[tokio::test]
async fn estimate_rejects_unknown_fields() {
    let mut b = estimate_body(POST_ST_MALL, MAERUA_MALL);
    b["price"] = json!(1);
    let (status, _, body) = send(&app(), post_json("/v1/fares/estimate", &b)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_request");
    assert!(body["error"]["message"].as_str().unwrap().contains("unknown field"));
}

#[tokio::test]
async fn estimate_rejects_points_outside_service_area() {
    let cape_town = (-33.92, 18.42);
    let (status, _, body) = send(&app(), post_json("/v1/fares/estimate", &estimate_body(POST_ST_MALL, cape_town))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["message"].as_str().unwrap().contains("service area"));
}

#[tokio::test]
async fn estimate_rejects_wrong_types_and_same_point() {
    let bad = json!({ "pickup": { "lat": "-22.5", "lon": 17.0 }, "dropoff": { "lat": -22.5, "lon": 17.0 } });
    let (status, _, _) = send(&app(), post_json("/v1/fares/estimate", &bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = send(&app(), post_json("/v1/fares/estimate", &estimate_body(POST_ST_MALL, POST_ST_MALL))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn malformed_json_and_wrong_content_type() {
    let req = Request::post("/v1/fares/estimate").header(header::CONTENT_TYPE, "application/json").body(Body::from("{not json")).unwrap();
    let (status, _, body) = send(&app(), req).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("malformed_json")));

    let req = Request::post("/v1/fares/estimate").header(header::CONTENT_TYPE, "text/plain").body(Body::from("{}")).unwrap();
    let (status, _, _) = send(&app(), req).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let huge = json!({ "pickup": "x".repeat(fuber_api::MAX_BODY_BYTES + 1) });
    let (status, _, _) = send(&app(), post_json("/v1/fares/estimate", &huge)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn endpoint_rate_limit_returns_429_with_retry_after() {
    let limits = RateLimits { fare_estimate: Rule { limit: 2, window_s: 60 }, ..RateLimits::default() };
    let app = app_with(limits, config(&[]));
    let body = estimate_body(POST_ST_MALL, MAERUA_MALL);
    for _ in 0..2 {
        assert_eq!(send(&app, post_json("/v1/fares/estimate", &body)).await.0, StatusCode::OK);
    }
    let (status, headers, json) = send(&app, post_json("/v1/fares/estimate", &body)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json["error"]["code"], "rate_limited");
    let retry: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry));
    // Other endpoints are unaffected by this group's limit.
    assert_eq!(send(&app, get("/v1/health")).await.0, StatusCode::OK);
}

#[tokio::test]
async fn global_limit_covers_every_route_including_unknown_ones() {
    let limits = RateLimits { global: Rule { limit: 3, window_s: 60 }, ..RateLimits::default() };
    let app = app_with(limits, config(&[]));
    for uri in ["/v1/health", "/v1/nope", "/anything"] {
        assert_ne!(send(&app, get(uri)).await.0, StatusCode::TOO_MANY_REQUESTS);
    }
    for uri in ["/v1/health", "/v1/nope", "/anything"] {
        assert_eq!(send(&app, get(uri)).await.0, StatusCode::TOO_MANY_REQUESTS, "{uri}");
    }
}

#[tokio::test]
async fn forwarded_for_is_ignored_unless_proxies_are_trusted() {
    let limits = RateLimits { global: Rule { limit: 1, window_s: 60 }, ..RateLimits::default() };
    let spoof = |ip: &str| Request::get("/v1/health").header("x-forwarded-for", ip).body(Body::empty()).unwrap();

    // No trusted proxy: a client cannot dodge the limit by inventing X-Forwarded-For values.
    let app = app_with(limits, config(&[]));
    assert_eq!(send(&app, spoof("1.1.1.1")).await.0, StatusCode::OK);
    assert_eq!(send(&app, spoof("2.2.2.2")).await.0, StatusCode::TOO_MANY_REQUESTS);

    // One trusted proxy: the right-most entry (added by our load balancer) is the client.
    let app = app_with(limits, config(&[("TRUSTED_PROXY_HOPS", "1")]));
    assert_eq!(send(&app, spoof("9.9.9.9, 1.1.1.1")).await.0, StatusCode::OK);
    assert_eq!(send(&app, spoof("9.9.9.9, 2.2.2.2")).await.0, StatusCode::OK);
    assert_eq!(send(&app, spoof("8.8.8.8, 1.1.1.1")).await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn cors_allows_only_listed_origins() {
    let app = app_with(RateLimits::default(), config(&[("CORS_ORIGINS", "https://ops.fuber.app, capacitor://localhost")]));
    let preflight = |origin: &str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/v1/fares/estimate")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap()
    };
    let (_, headers, _) = send(&app, preflight("capacitor://localhost")).await;
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "capacitor://localhost");
    let (_, headers, _) = send(&app, preflight("https://evil.example")).await;
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
}
