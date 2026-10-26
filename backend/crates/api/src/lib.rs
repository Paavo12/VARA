//! FUber HTTP API.

pub mod error;
pub mod extract;
pub mod rate_limit;
pub mod security;

mod routes {
    pub mod fares;
    pub mod health;
}

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{HeaderName, StatusCode},
    middleware,
    routing::{get, post},
};
use fuber_common::Config;
use sqlx::PgPool;
use tower_http::{
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use rate_limit::{Limit, RateLimiter, RateLimits, Rule};

/// JSON bodies above this are rejected (uploads go straight to object storage instead).
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub limiter: Arc<RateLimiter>,
    pub limits: RateLimits,
}

pub fn router(state: AppState) -> Router {
    let limit = |group: &'static str, rule: Rule| {
        middleware::from_fn_with_state(
            Limit { limiter: state.limiter.clone(), group, rule, trusted_proxy_hops: state.config.trusted_proxy_hops },
            rate_limit::enforce,
        )
    };

    let v1 = Router::new()
        .route("/health", get(routes::health::live))
        .route("/ready", get(routes::health::ready))
        .route("/fares/estimate", post(routes::fares::estimate).route_layer(limit("fare_estimate", state.limits.fare_estimate)));

    let request_id = HeaderName::from_static("x-request-id");
    let app = Router::new()
        .nest("/v1", v1)
        .fallback(error::fallback)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(security::cors(&state.config.cors_origins))
        // Every request, matched or not, passes the global per-IP backstop first.
        .layer(limit("global", state.limits.global))
        .layer(
            // Log the route template, never the raw URI: paths can carry share tokens.
            TraceLayer::new_for_http().make_span_with(|req: &Request| {
                let route = req.extensions().get::<MatchedPath>().map_or("<unmatched>", MatchedPath::as_str);
                let id = req.headers().get("x-request-id").and_then(|v| v.to_str().ok()).unwrap_or("-");
                tracing::info_span!("http", method = %req.method(), route, request_id = id)
            }),
        )
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid));

    let env = state.config.env;
    security::apply_headers(app.with_state(state), env)
}
