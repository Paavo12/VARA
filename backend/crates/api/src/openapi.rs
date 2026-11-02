//! The OpenAPI document. `cargo run -p fuber-api --bin openapi` writes it to
//! `packages/api-client/openapi.json`, from which the Svelte apps' TypeScript types are generated
//! (TRD §2.1: never hand-write request types). CI fails if the committed copy is stale.

use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

use crate::{ops, routes};

#[derive(OpenApi)]
#[openapi(
    info(title = "FUber API", version = "0.3.0", description = "Women-only ride-hailing for Windhoek. Amounts are integer cents of NAD."),
    paths(
        routes::health::live, routes::health::ready, routes::fares::estimate,
        routes::auth::request_otp, routes::auth::verify_otp, routes::auth::refresh, routes::auth::logout,
        routes::legal::latest, routes::legal::accept,
        routes::me::get, routes::me::update, routes::me::delete, routes::me::contacts, routes::me::replace_contacts,
        routes::me::submit_verification,
        routes::uploads::create, routes::uploads::status, routes::uploads::complete,
        routes::driver::get_application, routes::driver::save_application, routes::driver::submit, routes::driver::add_document,
        routes::places::search, routes::places::reverse,
        routes::rides::quote, routes::rides::create, routes::rides::active, routes::rides::get, routes::rides::cancel, routes::rides::retry,
        routes::driving::status, routes::driving::go_online, routes::driving::go_offline, routes::driving::location,
        routes::driving::accept, routes::driving::decline,
        routes::trip::arrive, routes::trip::pin, routes::trip::wait, routes::trip::no_show, routes::trip::start,
        routes::trip::complete, routes::trip::driver_cancel,
        ops::auth::login, ops::auth::logout,
        ops::verifications::queue, ops::verifications::detail, ops::verifications::decide,
        ops::drivers::queue, ops::drivers::detail, ops::drivers::set_tiers, ops::drivers::decide,
        ops::drivers::decide_document, ops::drivers::decide_vehicle,
    ),
    components(schemas(routes::legal::LegalKind)),
    modifiers(&Security),
    tags(
        (name = "health"), (name = "fares"),
        (name = "auth", description = "Phone + SMS code sign-in, token refresh"),
        (name = "me", description = "The signed-in person's profile, emergency contacts, verification and deletion"),
        (name = "legal", description = "Terms, Privacy Policy and consent"),
        (name = "uploads", description = "Signed-URL uploads for identity photos and driver documents"),
        (name = "driver", description = "Driver application"),
        (name = "booking", description = "Places, quotes and ride requests (rider app)"),
        (name = "dispatch", description = "Going online, positions and ride requests (driver app). Live updates: WebSocket `/v1/ws`"),
        (name = "trip", description = "Arriving, the PIN, the free wait, start, complete and cancel (driver app)"),
        (name = "ops", description = "Staff: verification and driver review"),
    )
)]
pub struct ApiDoc;

struct Security;

impl Modify for Security {
    fn modify(&self, doc: &mut utoipa::openapi::OpenApi) {
        let c = doc.components.get_or_insert_with(Default::default);
        c.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some("Rider/driver access token from /v1/auth/otp/verify or /v1/auth/refresh"))
                    .build(),
            ),
        );
        c.add_security_scheme(
            "staff",
            SecurityScheme::Http(
                HttpBuilder::new().scheme(HttpAuthScheme::Bearer).description(Some("Staff session token from /v1/ops/auth/login")).build(),
            ),
        );
    }
}

pub fn json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("OpenAPI document serialises")
}

#[cfg(test)]
mod tests {
    #[test]
    fn documents_every_route() {
        let doc = super::json();
        for path in [
            "/v1/health",
            "/v1/ready",
            "/v1/fares/estimate",
            "/v1/auth/otp/request",
            "/v1/auth/otp/verify",
            "/v1/auth/refresh",
            "/v1/auth/logout",
            "/v1/legal/{kind}/latest",
            "/v1/consents",
            "/v1/me",
            "/v1/me/emergency-contacts",
            "/v1/me/verification",
            "/v1/uploads",
            "/v1/uploads/{id}",
            "/v1/uploads/{id}/complete",
            "/v1/driver/application",
            "/v1/driver/application/submit",
            "/v1/driver/documents",
            "/v1/ops/auth/login",
            "/v1/ops/verifications/{id}/decision",
            "/v1/ops/drivers/{id}/tiers",
            "/v1/ops/vehicles/{id}/decision",
            "/v1/places",
            "/v1/places/reverse",
            "/v1/rides/quote",
            "/v1/rides",
            "/v1/rides/active",
            "/v1/rides/{id}/cancel",
            "/v1/rides/{id}/retry",
            "/v1/driver/status",
            "/v1/driver/online",
            "/v1/driver/offline",
            "/v1/driver/location",
            "/v1/offers/{id}/accept",
            "/v1/offers/{id}/decline",
            "/v1/rides/{id}/arrive",
            "/v1/rides/{id}/pin",
            "/v1/rides/{id}/wait",
            "/v1/rides/{id}/no-show",
            "/v1/rides/{id}/start",
            "/v1/rides/{id}/complete",
            "/v1/rides/{id}/driver-cancel",
        ] {
            assert!(doc.contains(&format!("\"{path}\"")), "{path} missing from OpenAPI");
        }
    }
}
