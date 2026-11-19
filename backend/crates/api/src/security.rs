//! Response hardening headers and CORS (TRD §7.5).

use std::time::Duration;

use axum::{
    Router,
    http::{HeaderName, HeaderValue, Method, header},
};
use fuber_common::Environment;
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer};

pub fn apply_headers(router: Router, env: Environment) -> Router {
    let fixed: [(HeaderName, &'static str); 6] = [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "no-referrer"),
        // The API only serves JSON: nothing may load or frame it.
        (header::CONTENT_SECURITY_POLICY, "default-src 'none'; frame-ancestors 'none'"),
        (header::CACHE_CONTROL, "no-store"),
        (HeaderName::from_static("cross-origin-resource-policy"), "same-site"),
    ];
    let mut router = router;
    for (name, value) in fixed {
        router = router.layer(SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value)));
    }
    if env != Environment::Local {
        router = router.layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
        ));
    }
    router
}

/// Exact-origin allow-list. An empty list allows no cross-origin browser calls at all.
/// Credentials are allowed so the ops console's session cookie is sent; only listed origins
/// get that, and changes also need the CSRF header.
pub fn cors(origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = origins.iter().filter_map(|o| HeaderValue::from_str(o).ok()).collect();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static(crate::ops::CSRF_HEADER),
        ])
        .max_age(Duration::from_secs(600))
}
