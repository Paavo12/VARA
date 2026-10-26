//! Liveness and readiness probes for the load balancer.

use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

use crate::AppState;

const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

#[utoipa::path(get, path = "/v1/health", tag = "health", responses((status = 200, description = "The process is up")))]
/// The process is up. Never touches dependencies, so a database outage doesn't cause restarts.
pub async fn live() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[utoipa::path(
    get,
    path = "/v1/ready",
    tag = "health",
    responses((status = 200, description = "Postgres and Redis answer"), (status = 503, description = "A dependency is unavailable"))
)]
/// The process can serve traffic: Postgres and Redis answer.
pub async fn ready(State(s): State<AppState>) -> (StatusCode, Json<Value>) {
    let db = tokio::time::timeout(CHECK_TIMEOUT, fuber_db::ping(&s.db)).await;
    let redis = tokio::time::timeout(CHECK_TIMEOUT, s.limiter.ping()).await;
    let db_ok = matches!(db, Ok(Ok(())));
    let redis_status = match redis {
        Ok(None) => "not_configured",
        Ok(Some(Ok(()))) => "ok",
        _ => "unavailable",
    };
    let ok = db_ok && redis_status != "unavailable";
    let status = if ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(json!({ "db": if db_ok { "ok" } else { "unavailable" }, "redis": redis_status })))
}
