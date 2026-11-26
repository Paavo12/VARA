//! Google Cloud Storage (TRD §2.3, §7.2): objects over the JSON API, and V4 signed URLs for
//! clients. Credentials are the service's own identity from the metadata server; there is no
//! key file, so URLs are signed with IAM Credentials `signBlob`, which only the API may call
//! (infra/terraform/iam.tf). The worker reads, writes and deletes but never signs.

use std::{
    io,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::{Mutex, OnceCell};

use crate::storage::{PresignedUpload, valid_key};

const TIMEOUT: Duration = Duration::from_secs(30);
/// Longest a V4 signed URL may live.
const MAX_URL_SECONDS: i64 = 7 * 24 * 3600;

/// Where Google's services are. Tests point these at a local fake.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub storage: String,
    pub metadata: String,
    pub iam: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            storage: "https://storage.googleapis.com".into(),
            metadata: "http://metadata.google.internal".into(),
            iam: "https://iamcredentials.googleapis.com".into(),
        }
    }
}

pub struct Gcs {
    bucket: String,
    at: Endpoints,
    http: reqwest::Client,
    /// Access token and when to fetch a new one.
    token: Mutex<Option<(String, Instant)>>,
    /// The service account the URLs are signed as.
    email: OnceCell<String>,
}

impl Gcs {
    pub fn new(bucket: &str, at: Endpoints) -> Self {
        let http = crate::http_client(TIMEOUT);
        Gcs { bucket: bucket.to_owned(), at, http, token: Mutex::new(None), email: OnceCell::new() }
    }

    async fn token(&self) -> io::Result<String> {
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            expires_in: u64,
        }
        let mut cached = self.token.lock().await;
        if let Some((token, until)) = &*cached
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let url = format!("{}/computeMetadata/v1/instance/service-accounts/default/token", self.at.metadata);
        let t: Token =
            check(self.http.get(url).header("Metadata-Flavor", "Google").send().await, "token")?.json().await.map_err(failed("token"))?;
        // Renew a minute early, so a token never expires mid-request.
        *cached = Some((t.access_token.clone(), Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(60))));
        Ok(t.access_token)
    }

    async fn email(&self) -> io::Result<&str> {
        self.email
            .get_or_try_init(|| async {
                let url = format!("{}/computeMetadata/v1/instance/service-accounts/default/email", self.at.metadata);
                check(self.http.get(url).header("Metadata-Flavor", "Google").send().await, "email")?.text().await.map_err(failed("email"))
            })
            .await
            .map(String::as_str)
    }

    fn object_url(&self, key: &str) -> io::Result<String> {
        Ok(format!("{}/storage/v1/b/{}/o/{}", self.at.storage, self.bucket, encode(checked(key)?)))
    }

    /// A missing object is `NotFound`.
    pub async fn read(&self, key: &str) -> io::Result<Vec<u8>> {
        let res = self.http.get(format!("{}?alt=media", self.object_url(key)?)).bearer_auth(self.token().await?).send().await;
        Ok(check(res, "read")?.bytes().await.map_err(failed("read"))?.to_vec())
    }

    pub async fn write(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let url = format!("{}/upload/storage/v1/b/{}/o?uploadType=media&name={}", self.at.storage, self.bucket, encode(checked(key)?));
        let res = self
            .http
            .post(url)
            .bearer_auth(self.token().await?)
            .header("content-type", content_type(key))
            .body(bytes.to_vec())
            .send()
            .await;
        check(res, "write").map(drop)
    }

    /// Deleting a missing object is not an error.
    pub async fn delete(&self, key: &str) -> io::Result<()> {
        match check(self.http.delete(self.object_url(key)?).bearer_auth(self.token().await?).send().await, "delete") {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r.map(drop),
        }
    }

    /// An upload URL for exactly this type and at most `max_bytes` (GCS refuses a larger body).
    pub async fn presign_put(&self, key: &str, content_type: &str, max_bytes: u64, expires_at: i64) -> io::Result<PresignedUpload> {
        let headers = vec![("content-type", content_type.to_owned()), ("x-goog-content-length-range", format!("0,{max_bytes}"))];
        let url = self.sign("PUT", key, &headers, OffsetDateTime::now_utc(), expires_at).await?;
        Ok(PresignedUpload { method: "PUT", url, headers, expires_at })
    }

    pub async fn presign_get(&self, key: &str, expires_at: i64) -> io::Result<String> {
        self.sign("GET", key, &[], OffsetDateTime::now_utc(), expires_at).await
    }

    async fn sign(
        &self,
        method: &str,
        key: &str,
        headers: &[(&'static str, String)],
        now: OffsetDateTime,
        expires_at: i64,
    ) -> io::Result<String> {
        let email = self.email().await?;
        let (url, to_sign) = self.unsigned(method, key, headers, email, now, expires_at)?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Signed {
            signed_blob: String,
        }
        let res = self
            .http
            .post(format!("{}/v1/projects/-/serviceAccounts/{}:signBlob", self.at.iam, encode(email)))
            .bearer_auth(self.token().await?)
            .json(&serde_json::json!({ "payload": B64.encode(to_sign) }))
            .send()
            .await;
        let signed: Signed = check(res, "signBlob")?.json().await.map_err(failed("signBlob"))?;
        let signature = B64.decode(signed.signed_blob).map_err(failed("signBlob"))?;
        Ok(format!("{url}&X-Goog-Signature={}", hex(&signature)))
    }

    /// The V4 signed URL without its signature, and the string to sign
    /// (cloud.google.com/storage/docs/authentication/signatures).
    fn unsigned(
        &self,
        method: &str,
        key: &str,
        headers: &[(&'static str, String)],
        email: &str,
        now: OffsetDateTime,
        expires_at: i64,
    ) -> io::Result<(String, String)> {
        let path = format!("/{}/{}", self.bucket, checked(key)?.split('/').map(encode).collect::<Vec<_>>().join("/"));
        let host = self.at.storage.split("://").nth(1).unwrap_or(&self.at.storage);
        let datetime = format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        );
        let scope = format!("{}/auto/storage/goog4_request", &datetime[..8]);
        let mut all: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).chain([("host", host)]).collect();
        all.sort();
        let canonical_headers: String = all.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
        let signed_headers = all.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
        let expires = (expires_at - now.unix_timestamp()).clamp(1, MAX_URL_SECONDS);
        // Already in sorted order, as the canonical query string must be.
        let query = [
            ("X-Goog-Algorithm", "GOOG4-RSA-SHA256".to_owned()),
            ("X-Goog-Credential", format!("{email}/{scope}")),
            ("X-Goog-Date", datetime.clone()),
            ("X-Goog-Expires", expires.to_string()),
            ("X-Goog-SignedHeaders", signed_headers.clone()),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&");
        let request = [method, &path, &query, &canonical_headers, &signed_headers, "UNSIGNED-PAYLOAD"].join("\n");
        let to_sign = format!("GOOG4-RSA-SHA256\n{datetime}\n{scope}\n{}", hex(&Sha256::digest(request.as_bytes())));
        Ok((format!("{}{path}?{query}", self.at.storage), to_sign))
    }
}

fn checked(key: &str) -> io::Result<&str> {
    if valid_key(key) { Ok(key) } else { Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid object key")) }
}

/// Clean objects are named by their type (`.jpg`, `.pdf`); quarantined ones are unchecked bytes.
fn content_type(key: &str) -> &'static str {
    match key.rsplit_once('.').map(|(_, ext)| ext) {
        Some("jpg") => "image/jpeg",
        Some("png") => "image/png",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Percent-encoding of everything but the URI's unreserved characters.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn failed<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> io::Error {
    move |e| io::Error::other(format!("GCS {what}: {e}"))
}

/// The response if it succeeded; 404 becomes `NotFound`.
fn check(res: reqwest::Result<reqwest::Response>, what: &'static str) -> io::Result<reqwest::Response> {
    let res = res.map_err(failed(what))?;
    match res.status() {
        s if s.is_success() => Ok(res),
        reqwest::StatusCode::NOT_FOUND => Err(io::Error::new(io::ErrorKind::NotFound, format!("GCS {what}: not found"))),
        s => Err(io::Error::other(format!("GCS {what}: HTTP {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex as StdMutex},
    };

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };

    use super::*;

    const EMAIL: &str = "vara-api@vara-staging.iam.gserviceaccount.com";

    #[derive(Default)]
    struct Fake {
        objects: StdMutex<HashMap<String, (String, Vec<u8>)>>,
        tokens_issued: StdMutex<u32>,
    }

    fn authorised(h: &HeaderMap) -> bool {
        h.get("authorization").is_some_and(|v| v == "Bearer tok-1")
    }

    /// Google's three services, in memory, on a local port.
    async fn fake() -> (Gcs, Arc<Fake>) {
        let state = Arc::new(Fake::default());
        let app = Router::new()
            .route(
                "/computeMetadata/v1/instance/service-accounts/default/token",
                get(|State(f): State<Arc<Fake>>, h: HeaderMap| async move {
                    assert_eq!(h["metadata-flavor"], "Google");
                    *f.tokens_issued.lock().unwrap() += 1;
                    Json(serde_json::json!({ "access_token": "tok-1", "expires_in": 3599, "token_type": "Bearer" }))
                }),
            )
            .route("/computeMetadata/v1/instance/service-accounts/default/email", get(|| async { EMAIL }))
            // Echoes the payload as the "signature", so tests can read what was signed.
            .route(
                "/v1/projects/-/serviceAccounts/{account}",
                post(|Path(account): Path<String>, h: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                    assert!(authorised(&h));
                    assert_eq!(account, format!("{EMAIL}:signBlob"));
                    Json(serde_json::json!({ "keyId": "k1", "signedBlob": body["payload"] }))
                }),
            )
            .route(
                "/storage/v1/b/{bucket}/o/{name}",
                get(|State(f): State<Arc<Fake>>, Path((bucket, name)): Path<(String, String)>, h: HeaderMap| async move {
                    assert!(authorised(&h) && bucket == "docs");
                    match f.objects.lock().unwrap().get(&name) {
                        Some((_, bytes)) => Ok(bytes.clone()),
                        None => Err(StatusCode::NOT_FOUND),
                    }
                })
                .delete(|State(f): State<Arc<Fake>>, Path((_, name)): Path<(String, String)>, h: HeaderMap| async move {
                    assert!(authorised(&h));
                    if f.objects.lock().unwrap().remove(&name).is_some() { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
                }),
            )
            .route(
                "/upload/storage/v1/b/{bucket}/o",
                post(|State(f): State<Arc<Fake>>, Query(q): Query<HashMap<String, String>>, h: HeaderMap, body: Bytes| async move {
                    assert!(authorised(&h) && q["uploadType"] == "media");
                    let ct = h["content-type"].to_str().unwrap().to_owned();
                    f.objects.lock().unwrap().insert(q["name"].clone(), (ct, body.to_vec()));
                    Json(serde_json::json!({}))
                }),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let at = Endpoints { storage: base.clone(), metadata: base.clone(), iam: base };
        (Gcs::new("docs", at), state)
    }

    #[tokio::test]
    async fn write_read_delete_with_one_token() {
        let (gcs, fake) = fake().await;
        gcs.write("clean/u1/a.jpg", b"jpeg bytes").await.unwrap();
        assert_eq!(fake.objects.lock().unwrap()["clean/u1/a.jpg"].0, "image/jpeg");
        assert_eq!(gcs.read("clean/u1/a.jpg").await.unwrap(), b"jpeg bytes");
        gcs.delete("clean/u1/a.jpg").await.unwrap();
        gcs.delete("clean/u1/a.jpg").await.unwrap();
        assert_eq!(gcs.read("clean/u1/a.jpg").await.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(*fake.tokens_issued.lock().unwrap(), 1, "the token is reused until it nearly expires");
        assert_eq!(gcs.write("../escape", b"x").await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn signed_upload_url_binds_type_and_size() {
        let (gcs, _) = fake().await;
        let expires_at = OffsetDateTime::now_utc().unix_timestamp() + 900;
        let p = gcs.presign_put("quarantine/abc", "image/png", 5000, expires_at).await.unwrap();
        assert_eq!(p.headers, vec![("content-type", "image/png".to_owned()), ("x-goog-content-length-range", "0,5000".to_owned())]);
        assert!(p.url.contains("/docs/quarantine/abc?X-Goog-Algorithm=GOOG4-RSA-SHA256&"), "{}", p.url);
        assert!(p.url.contains("X-Goog-SignedHeaders=content-type%3Bhost%3Bx-goog-content-length-range&"), "{}", p.url);
        // The fake signer echoes what it was given: the V4 string to sign.
        let signed = p.url.rsplit_once("X-Goog-Signature=").unwrap().1;
        let bytes: Vec<u8> = (0..signed.len()).step_by(2).map(|i| u8::from_str_radix(&signed[i..i + 2], 16).unwrap()).collect();
        assert!(String::from_utf8(bytes).unwrap().starts_with("GOOG4-RSA-SHA256\n"));
    }

    /// The canonical request for fixed inputs, written out by hand from Google's V4 spec.
    #[test]
    fn canonical_request_matches_the_v4_format() {
        let gcs = Gcs::new("vara-staging-documents", Endpoints::default());
        let now = time::macros::datetime!(2026-11-25 09:05:03 UTC);
        let headers = [("content-type", "image/jpeg".to_owned()), ("x-goog-content-length-range", "0,1000".to_owned())];
        let (url, to_sign) = gcs.unsigned("PUT", "quarantine/0b8e", &headers, EMAIL, now, now.unix_timestamp() + 900).unwrap();
        let query = "X-Goog-Algorithm=GOOG4-RSA-SHA256\
                     &X-Goog-Credential=vara-api%40vara-staging.iam.gserviceaccount.com%2F20261125%2Fauto%2Fstorage%2Fgoog4_request\
                     &X-Goog-Date=20261125T090503Z&X-Goog-Expires=900\
                     &X-Goog-SignedHeaders=content-type%3Bhost%3Bx-goog-content-length-range";
        assert_eq!(url, format!("https://storage.googleapis.com/vara-staging-documents/quarantine/0b8e?{query}"));
        let request = format!(
            "PUT\n/vara-staging-documents/quarantine/0b8e\n{query}\n\
             content-type:image/jpeg\nhost:storage.googleapis.com\nx-goog-content-length-range:0,1000\n\n\
             content-type;host;x-goog-content-length-range\nUNSIGNED-PAYLOAD"
        );
        let expected =
            format!("GOOG4-RSA-SHA256\n20261125T090503Z\n20261125/auto/storage/goog4_request\n{}", hex(&Sha256::digest(request)));
        assert_eq!(to_sign, expected);
    }

    /// Cases from Google's signing conformance tests (googleapis/conformance-tests,
    /// storage/v1/v4_signatures.json), with Google's expected strings to sign.
    #[test]
    fn google_v4_conformance_cases() {
        let gcs = Gcs::new("test-bucket", Endpoints::default());
        let email = "test-iam-credentials@dummy-project-id.iam.gserviceaccount.com";
        let now = time::macros::datetime!(2019-02-01 09:00:00 UTC);
        let resumable = [("x-goog-resumable", "start".to_owned())];
        for (method, headers, hash) in [
            ("GET", &[][..], "00e2fb794ea93d7adb703edaebdd509821fcc7d4f1a79ac5c8d2b394df109320"),
            ("PUT", &[][..], "78742860705da91404222d5d66ff89850292471199c3c2808d116ad12e6177b4"),
            ("POST", &resumable[..], "877f8b40179d2753296f2fd6de815ab40503c7a3c446a7b44aa4e74422ff4daf"),
        ] {
            let (_, to_sign) = gcs.unsigned(method, "test-object", headers, email, now, now.unix_timestamp() + 10).unwrap();
            let expected = format!("GOOG4-RSA-SHA256\n20190201T090000Z\n20190201/auto/storage/goog4_request\n{hash}");
            assert_eq!(to_sign, expected, "{method}");
        }
    }
}
