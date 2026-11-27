//! Third-party services behind VARA-owned interfaces (TRD §8). Each has a fake or local
//! implementation, so the whole system runs end to end in development and CI without vendor
//! accounts; real vendors are added as variants once they are chosen (PRD §10).

pub mod gcs;
mod google;
pub mod media;
pub mod push;
pub mod routing;
pub mod sms;
pub mod storage;
pub mod virus;

/// Every outbound HTTP client is built here. reqwest is built without its own crypto
/// provider (aws-lc needs CMake and NASM to build), so ring is installed first, as sqlx and
/// redis already use it.
pub(crate) fn http_client(timeout: std::time::Duration) -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().timeout(timeout).build().expect("HTTP client builds")
}
