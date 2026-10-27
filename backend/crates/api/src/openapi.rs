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
    info(title = "FUber API", version = "0.2.0", description = "Women-only ride-hailing for Windhoek. Amounts are integer cents of NAD."),
    paths(
        routes::health::live, routes::health::ready, routes::fares::estimate,
        routes::auth::request_otp, routes::auth::verify_otp, routes::auth::refresh, routes::auth::logout,
        routes::legal::latest, routes::legal::accept,
        routes::me::get, routes::me::update, routes::me::delete, routes::me::contacts, routes::me::replace_contacts,
        routes::me::submit_verification,
        routes::uploads::create, routes::uploads::status, routes::uploads::complete,
        routes::driver::get_application, routes::driver::save_application, routes::driver::submit, routes::driver::add_document,
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
        ] {
            assert!(doc.contains(&format!("\"{path}\"")), "{path} missing from OpenAPI");
        }
    }
}
