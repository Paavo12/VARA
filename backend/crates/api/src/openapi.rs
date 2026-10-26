//! The OpenAPI document. `cargo run -p fuber-api --bin openapi` writes it to
//! `packages/api-client/openapi.json`, from which the Svelte apps' TypeScript types are generated
//! (TRD §2.1: never hand-write request types). CI fails if the committed copy is stale.

use utoipa::OpenApi;

use crate::routes;

#[derive(OpenApi)]
#[openapi(
    info(title = "FUber API", version = "0.1.0", description = "Women-only ride-hailing for Windhoek. Amounts are integer cents of NAD."),
    paths(routes::health::live, routes::health::ready, routes::fares::estimate),
    tags((name = "health"), (name = "fares"))
)]
pub struct ApiDoc;

pub fn json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("OpenAPI document serialises")
}

#[cfg(test)]
mod tests {
    #[test]
    fn documents_every_route() {
        let doc = super::json();
        for path in ["/v1/health", "/v1/ready", "/v1/fares/estimate"] {
            assert!(doc.contains(path), "{path} missing from OpenAPI");
        }
    }
}
