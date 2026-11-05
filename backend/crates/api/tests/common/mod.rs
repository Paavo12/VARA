//! Test harness shared by the database-backed integration tests: a scratch PostGIS database
//! per test (copied from a migrated template), the API router, the worker context, and
//! helpers for signing in, onboarding and booking.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one).

#![allow(dead_code)]

mod booking;

// Only the dispatch and trip tests use these.
#[allow(unused_imports)]
pub use booking::*;

use std::net::SocketAddr;

use argon2::{Argon2, password_hash::PasswordHasher};
use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use fuber_api::{
    AppState,
    rate_limit::{RateLimiter, RateLimits},
};
use fuber_common::{
    Config,
    crypto::{Hasher, Sealer, random_bytes, totp},
};
use fuber_db::sqlx::{self, PgPool};
use fuber_integrations::{
    routing::Routing,
    sms::{FakeSms, SmsGateway},
    storage::ObjectStore,
    virus::VirusScanner,
};
use fuber_worker::Ctx;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::OnceCell;
use tower::ServiceExt;
use uuid::Uuid;

pub const PUBLIC_URL: &str = "http://api.test";

static TEMPLATE: OnceCell<Option<(String, String)>> = OnceCell::const_new();

/// A migrated template database, created once per test binary: `(server URL prefix, name)`.
/// Leftover databases from earlier runs are dropped first.
pub async fn template() -> Option<(String, String)> {
    TEMPLATE
        .get_or_init(|| async {
            let base = std::env::var("TEST_DATABASE_URL").ok()?;
            let (prefix, _) = base.rsplit_once('/').unwrap();
            let admin = PgPool::connect(&base).await.expect("TEST_DATABASE_URL connects");
            let old: Vec<String> =
                sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'fuber_t_%'").fetch_all(&admin).await.unwrap();
            for name in old {
                let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).execute(&admin).await;
            }
            let name = format!("fuber_t_template_{}", Uuid::new_v4().simple());
            sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
            admin.close().await;
            let pool = PgPool::connect(&format!("{prefix}/{name}")).await.unwrap();
            fuber_db::MIGRATOR.run(&pool).await.expect("migrations apply");
            pool.close().await;
            Some((prefix.to_owned(), name))
        })
        .await
        .clone()
}

/// A fresh database for one test, copied from the template (fast), so tests running in
/// parallel never see each other's jobs, codes or rows.
pub async fn database_url() -> Option<String> {
    let (prefix, template) = template().await?;
    let admin = PgPool::connect(&format!("{prefix}/postgres")).await.unwrap();
    let name = format!("fuber_t_{}", Uuid::new_v4().simple());
    // Postgres refuses to copy a template two sessions at a time; retry briefly.
    for attempt in 0.. {
        match sqlx::query(&format!("CREATE DATABASE {name} TEMPLATE {template}")).execute(&admin).await {
            Ok(_) => break,
            Err(e) if attempt < 50 => {
                let _ = e;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => panic!("cannot create test database: {e}"),
        }
    }
    admin.close().await;
    Some(format!("{prefix}/{name}"))
}

pub struct T {
    pub app: Router,
    pub state: AppState,
    pub ctx: Ctx,
}

macro_rules! setup {
    () => {
        setup!(RateLimits::default())
    };
    ($limits:expr) => {
        match T::new($limits).await {
            Some(t) => t,
            None => {
                eprintln!("skipped: TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

impl T {
    pub async fn new(limits: RateLimits) -> Option<T> {
        let url = database_url().await?;
        let dir = std::env::temp_dir().join(format!("fuber-test-{}", Uuid::new_v4().simple()));
        let pairs = [
            ("APP_ENV", "local".to_owned()),
            ("DATABASE_URL", url),
            ("REDIS_URL", "redis://127.0.0.1:1".to_owned()),
            ("HASH_SECRET", "test-hash-secret-0123456789abcdef0123".to_owned()),
            ("DATA_KEY", "test-data-key-0123456789abcdef0123456".to_owned()),
            ("JWT_SECRET", "test-jwt-secret-0123456789abcdef01234".to_owned()),
            ("STORAGE_DIR", dir.to_string_lossy().into_owned()),
            ("PUBLIC_API_URL", PUBLIC_URL.to_owned()),
        ];
        let cfg = Config::from_lookup(|k| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone())).unwrap();
        let db = fuber_db::pool(&cfg.database_url).unwrap();
        let store = ObjectStore::from_config(&cfg, Hasher::new(&cfg.hash_secret));
        let ctx = Ctx {
            db: db.clone(),
            sealer: Sealer::new(&cfg.data_key),
            hasher: Hasher::new(&cfg.hash_secret),
            trip_link_base: cfg.trip_link_base.clone(),
            store,
            scanner: VirusScanner::Disabled,
            sms: SmsGateway::Fake(FakeSms::new(false)),
            routing: Routing::Estimate,
        };
        let state = AppState::new(cfg, db, RateLimiter::memory_only(), limits);
        let app = fuber_api::router(state.clone()).layer(MockConnectInfo(SocketAddr::from(([10, 1, 2, 3], 4000))));
        Some(T { app, state, ctx })
    }

    pub async fn call(&self, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        self.call_with(method, uri, token, body, &[]).await
    }

    /// `call` with extra request headers (e.g. `Idempotency-Key`).
    pub async fn call_with(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    pub async fn get(&self, uri: &str, token: &str) -> (StatusCode, Value) {
        self.call(Method::GET, uri, Some(token), None).await
    }

    pub async fn post(&self, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, uri, Some(token), Some(body)).await
    }

    pub async fn put(&self, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::PUT, uri, Some(token), Some(body)).await
    }

    pub async fn patch(&self, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::PATCH, uri, Some(token), Some(body)).await
    }

    /// The last code the fake SMS gateway sent to `phone`.
    pub fn last_code(&self, phone: &str) -> String {
        let sms = self.state.sms.outbox().into_iter().rev().find(|m| m.to == phone).expect("an SMS was sent");
        sms.body[..6].to_owned()
    }

    /// Pretends this number's earlier codes were sent two hours ago (skips the resend cooldown).
    pub async fn age_codes(&self, phone: &str) {
        let hash = self.state.keys.hasher.hash("phone", phone.as_bytes()).to_vec();
        sqlx::query("UPDATE otp_challenges SET created_at = created_at - interval '2 hours' WHERE phone_hash = $1")
            .bind(hash)
            .execute(&self.state.db)
            .await
            .unwrap();
    }

    pub async fn request_code(&self, phone: &str) -> String {
        let (status, body) = self.call(Method::POST, "/v1/auth/otp/request", None, Some(json!({ "phone": phone }))).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        body["challenge_id"].as_str().unwrap().to_owned()
    }

    /// Signs in through OTP and returns the verify response.
    pub async fn sign_in(&self, phone: &str, app: &str) -> Session {
        self.age_codes(phone).await;
        let challenge = self.request_code(phone).await;
        let code = self.last_code(phone);
        let (status, body) = self
            .call(
                Method::POST,
                "/v1/auth/otp/verify",
                None,
                Some(json!({ "challenge_id": challenge, "phone": phone, "code": code, "app": app })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        Session {
            access: body["access_token"].as_str().unwrap().to_owned(),
            refresh: body["refresh_token"].as_str().unwrap().to_owned(),
            user_id: body["user_id"].as_str().unwrap().parse().unwrap(),
            new_user: body["new_user"].as_bool().unwrap(),
        }
    }

    /// A signed-in user with name and date of birth filled in and legal documents accepted.
    pub async fn onboarded(&self, app: &str) -> Session {
        let s = self.sign_in(&new_phone(), app).await;
        let (status, me) = self.patch("/v1/me", &s.access, json!({ "full_name": "Ndeshi Shikongo", "date_of_birth": "1994-03-12" })).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        let ids: Vec<Value> = me["pending_consents"].as_array().unwrap().iter().map(|d| d["id"].clone()).collect();
        assert_eq!(self.post("/v1/consents", &s.access, json!({ "document_ids": ids })).await.0, StatusCode::NO_CONTENT);
        s
    }

    /// Creates an upload, sends the bytes to its signed URL and completes it. Does not run the worker.
    pub async fn send_upload(&self, token: &str, purpose: &str, content_type: &str, bytes: &[u8]) -> String {
        let (status, target) =
            self.post("/v1/uploads", token, json!({ "purpose": purpose, "content_type": content_type, "size_bytes": bytes.len() })).await;
        assert_eq!(status, StatusCode::CREATED, "{target}");
        let path = target["url"].as_str().unwrap().strip_prefix(PUBLIC_URL).unwrap().to_owned();
        let req = Request::put(&path).header(header::CONTENT_TYPE, content_type).body(Body::from(bytes.to_vec())).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let id = target["upload_id"].as_str().unwrap().to_owned();
        assert_eq!(self.post(&format!("/v1/uploads/{id}/complete"), token, json!({})).await.0, StatusCode::ACCEPTED);
        id
    }

    /// Uploads a valid photo and runs the worker so it is clean.
    pub async fn photo(&self, token: &str, purpose: &str) -> String {
        let id = self.send_upload(token, purpose, "image/png", &png()).await;
        self.work().await;
        id
    }

    pub async fn work(&self) {
        fuber_worker::drain(&self.ctx).await.unwrap();
    }

    /// A staff member with the given role; returns their bearer token.
    pub async fn staff(&self, role: &str) -> String {
        let email = format!("{}@ops.fuber.test", Uuid::new_v4().simple());
        let secret = random_bytes::<20>();
        let hash = Argon2::default().hash_password(b"correct horse battery").unwrap().to_string();
        sqlx::query("INSERT INTO staff_users (email, full_name, password_hash, totp_secret, roles) VALUES ($1, 'Test Reviewer', $2, $3, ARRAY[$4]::staff_role[])")
            .bind(&email)
            .bind(hash)
            .bind(self.state.keys.sealer.seal("staff_users.totp_secret", &secret))
            .bind(role)
            .execute(&self.state.db)
            .await
            .unwrap();
        let code = totp::code(&secret, OffsetDateTime::now_utc().unix_timestamp() / 30);
        let login = json!({ "email": email, "password": "correct horse battery", "totp": code });
        let (status, body) = self.call(Method::POST, "/v1/ops/auth/login", None, Some(login.clone())).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // The same code can't be used twice.
        assert_eq!(self.call(Method::POST, "/v1/ops/auth/login", None, Some(login)).await.0, StatusCode::UNAUTHORIZED);
        body["token"].as_str().unwrap().to_owned()
    }
}

pub struct Session {
    pub access: String,
    pub refresh: String,
    pub user_id: Uuid,
    pub new_user: bool,
}

pub fn new_phone() -> String {
    let n = u32::from_le_bytes(random_bytes::<4>()) % 10_000_000;
    format!("+26481{n:07}")
}

pub fn png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(400, 300, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 120]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
