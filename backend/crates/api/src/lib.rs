//! VARA HTTP API.

pub mod audit;
pub mod auth;
pub mod error;
pub mod extract;
pub mod live;
pub mod openapi;
pub mod rate_limit;
pub mod security;
pub mod views;

mod routes {
    pub mod auth;
    pub mod chat;
    pub mod driver;
    pub mod driving;
    pub mod earnings;
    pub mod fares;
    pub mod health;
    pub mod history;
    pub mod legal;
    pub mod me;
    pub mod places;
    pub mod ratings;
    pub mod report;
    pub mod ridecheck;
    pub mod rides;
    pub mod safety;
    pub mod settlements;
    pub mod share;
    pub mod storage;
    pub mod trip;
    pub mod uploads;
}

pub mod ops;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{HeaderName, StatusCode},
    middleware,
    routing::{delete, get, post, put},
};
use fuber_common::{Config, Environment, RoutingConfig, SmsProvider};
use fuber_integrations::{
    routing::Routing,
    sms::{FakeSms, SmsGateway},
    storage::{MAX_UPLOAD_BYTES, ObjectStore},
};
use sqlx::PgPool;
use tower_http::{
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use auth::Keys;
use rate_limit::{Limit, RateLimiter, RateLimits, Rule};

/// JSON bodies above this are rejected (uploads go straight to object storage instead).
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// File transfers through the local storage backend: 10 MB on a slow mobile connection.
pub const STORAGE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub limiter: Arc<RateLimiter>,
    pub limits: RateLimits,
    pub keys: Arc<Keys>,
    pub sms: Arc<SmsGateway>,
    pub store: Arc<ObjectStore>,
    pub routing: Arc<Routing>,
    /// This instance's WebSockets; `live::listen` feeds it.
    pub hub: Arc<live::Hub>,
}

impl AppState {
    /// Builds keys and integrations from the configuration.
    pub fn new(config: Config, db: PgPool, limiter: RateLimiter, limits: RateLimits) -> AppState {
        let keys = Keys::from_config(&config);
        let sms = match config.sms_provider {
            SmsProvider::Fake => SmsGateway::Fake(FakeSms::new(config.env == Environment::Local)),
        };
        let store = ObjectStore::from_config(&config, keys.hasher.clone());
        let routing = match &config.routing {
            RoutingConfig::Osrm(url) => Routing::osrm(url),
            RoutingConfig::Estimate => Routing::Estimate,
        };
        AppState {
            config: Arc::new(config),
            db,
            limiter: Arc::new(limiter),
            limits,
            keys,
            sms: Arc::new(sms),
            store: Arc::new(store),
            routing: Arc::new(routing),
            hub: Arc::new(live::Hub::default()),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let limit = |group: &'static str, rule: Rule| {
        middleware::from_fn_with_state(
            Limit { limiter: state.limiter.clone(), group, rule, trusted_proxy_hops: state.config.trusted_proxy_hops },
            rate_limit::enforce,
        )
    };
    let l = state.limits;

    let v1 = Router::new()
        .route("/health", get(routes::health::live))
        .route("/ready", get(routes::health::ready))
        .route("/fares/estimate", post(routes::fares::estimate).route_layer(limit("fare_estimate", l.fare_estimate)))
        // Sign-in
        .route("/auth/otp/request", post(routes::auth::request_otp).route_layer(limit("otp_request", l.otp_request)))
        .route("/auth/otp/verify", post(routes::auth::verify_otp).route_layer(limit("otp_verify", l.otp_verify)))
        .route("/auth/refresh", post(routes::auth::refresh).route_layer(limit("refresh_ip", l.refresh)))
        .route("/auth/logout", post(routes::auth::logout))
        // Legal documents (public) and consent
        .route("/legal/{kind}/latest", get(routes::legal::latest).route_layer(limit("legal", l.legal)))
        .route("/consents", post(routes::legal::accept))
        // Profile
        .route("/me", get(routes::me::get).patch(routes::me::update).delete(routes::me::delete))
        .route("/me/emergency-contacts", get(routes::me::contacts).put(routes::me::replace_contacts))
        .route("/me/verification", post(routes::me::submit_verification))
        .route("/me/push-token", put(routes::me::set_push_token).delete(routes::me::clear_push_token))
        // Uploads
        .route("/uploads", post(routes::uploads::create))
        .route("/uploads/{id}", get(routes::uploads::status))
        .route("/uploads/{id}/complete", post(routes::uploads::complete))
        // Driver onboarding
        .route("/driver/application", get(routes::driver::get_application).put(routes::driver::save_application))
        .route("/driver/application/submit", post(routes::driver::submit))
        .route("/driver/documents", post(routes::driver::add_document))
        // Booking (rider)
        .route("/places", get(routes::places::search))
        .route("/places/reverse", get(routes::places::reverse))
        .route("/rides/quote", post(routes::rides::quote))
        .route("/rides", post(routes::rides::create).get(routes::history::rider_trips))
        .route("/rides/active", get(routes::rides::active))
        .route("/rides/{id}", get(routes::rides::get))
        .route("/rides/{id}/cancel", post(routes::rides::cancel))
        .route("/rides/{id}/retry", post(routes::rides::retry))
        // Dispatch (driver)
        .route("/driver/status", get(routes::driving::status))
        .route("/driver/online", post(routes::driving::go_online))
        .route("/driver/offline", post(routes::driving::go_offline))
        .route("/driver/location", put(routes::driving::location))
        .route("/offers/{id}/accept", post(routes::driving::accept))
        .route("/offers/{id}/decline", post(routes::driving::decline))
        // Trip (driver)
        .route("/rides/{id}/arrive", post(routes::trip::arrive))
        .route("/rides/{id}/pin", post(routes::trip::pin))
        .route("/rides/{id}/wait", post(routes::trip::wait))
        .route("/rides/{id}/no-show", post(routes::trip::no_show))
        .route("/rides/{id}/start", post(routes::trip::start))
        .route("/rides/{id}/complete", post(routes::trip::complete))
        .route("/rides/{id}/driver-cancel", post(routes::trip::driver_cancel))
        // After the trip: cash, ratings, favourites, history, earnings
        .route("/rides/{id}/cash-received", post(routes::earnings::cash_received))
        .route("/rides/{id}/rating", post(routes::ratings::rate))
        .route("/rides/{id}/rating/skip", post(routes::ratings::skip))
        .route("/me/favourite-drivers", get(routes::ratings::favourites))
        .route("/me/favourite-drivers/{driver_id}", delete(routes::ratings::remove_favourite))
        .route("/driver/trips", get(routes::history::driver_trips))
        .route("/driver/earnings", get(routes::earnings::earnings))
        .route("/driver/statements", get(routes::settlements::statements))
        // Chat (either app)
        .route("/rides/{id}/messages", get(routes::chat::thread).post(routes::chat::send))
        // Safety (either app; never rate-limited, see rate_limit::enforce)
        .route("/safety/sos", post(routes::safety::sos))
        .route("/safety/ridecheck/{id}/respond", post(routes::ridecheck::respond))
        .route("/rides/{id}/report", post(routes::report::report))
        // Share trip: a link from either app, and the public page's data (website /t/{token})
        .route("/rides/{id}/share", post(routes::share::create))
        .route("/trip-shares/{token}", get(routes::share::view).route_layer(limit("trip_share", l.trip_share)))
        // Real-time updates
        .route("/ws", get(live::upgrade))
        // Ops (staff)
        .nest("/ops", ops::router(&state));

    let json_api = Router::new()
        .nest("/v1", v1)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT));

    // Signed-URL file transfer, only with the local storage backend (cloud storage serves
    // these requests itself). Larger bodies and a longer timeout than the JSON API.
    let mut app = json_api;
    if state.store.local().is_some() {
        let max = MAX_UPLOAD_BYTES as usize;
        let storage = Router::new()
            .route("/v1/storage/{*key}", put(routes::storage::upload).get(routes::storage::download))
            .route_layer(limit("storage", l.storage))
            .layer(DefaultBodyLimit::max(max))
            .layer(RequestBodyLimitLayer::new(max))
            .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, STORAGE_TIMEOUT));
        app = app.merge(storage);
    }

    let request_id = HeaderName::from_static("x-request-id");
    let app = app
        .fallback(error::fallback)
        .layer(security::cors(&state.config.cors_origins))
        // Every request, matched or not, passes the global per-IP backstop first.
        .layer(limit("global", l.global))
        .layer(
            // Log the route template, never the raw URI: paths can carry share tokens and
            // storage signatures.
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
