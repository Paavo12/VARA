//! Prints the OpenAPI document. Run from the repo root:
//! `cargo run -q --manifest-path backend/Cargo.toml -p fuber-api --bin openapi > packages/api-client/openapi.json`

fn main() {
    println!("{}", fuber_api::openapi::json());
}
