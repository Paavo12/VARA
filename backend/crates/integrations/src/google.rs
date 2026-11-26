//! The service's own Google credentials, from the metadata server (Cloud Run). There is no key
//! file anywhere; Cloud Storage and Firebase Cloud Messaging both use these tokens.

use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

/// An access token, fetched when first needed and renewed a minute before it expires.
pub(crate) struct MetadataToken {
    metadata: String,
    http: reqwest::Client,
    cached: Mutex<Option<(String, Instant)>>,
}

impl MetadataToken {
    /// `metadata` is the metadata server's base URL (tests point it at a fake).
    pub(crate) fn new(metadata: &str, http: reqwest::Client) -> Self {
        MetadataToken { metadata: metadata.to_owned(), http, cached: Mutex::new(None) }
    }

    pub(crate) async fn get(&self) -> Result<String, String> {
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            expires_in: u64,
        }
        let mut cached = self.cached.lock().await;
        if let Some((token, until)) = &*cached
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let url = format!("{}/computeMetadata/v1/instance/service-accounts/default/token", self.metadata);
        let res = self.http.get(url).header("Metadata-Flavor", "Google").send().await.map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("HTTP {}", res.status()));
        }
        let t: Token = res.json().await.map_err(|e| e.to_string())?;
        // Renew a minute early, so a token never expires mid-request.
        *cached = Some((t.access_token.clone(), Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(60))));
        Ok(t.access_token)
    }
}
