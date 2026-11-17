//! Sliding-window rate limiting on every public endpoint (TRD §7.1).
//!
//! Counters live in Redis so limits hold across API instances. If Redis is unreachable the
//! limiter falls back to per-instance memory counters (still enforced, just per instance)
//! and retries Redis after a short back-off, so a Redis outage never disables rate limiting
//! and never takes the API down.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::{error::ApiError, extract::client_ip};

/// At most `limit` requests per `window_s` seconds per key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub limit: u32,
    pub window_s: u64,
}

impl Rule {
    pub const fn per_minute(limit: u32) -> Rule {
        Rule { limit, window_s: 60 }
    }

    pub const fn per_hour(limit: u32) -> Rule {
        Rule { limit, window_s: 3600 }
    }
}

/// Limits per endpoint group. Defaults follow TRD §7.1; any subset can be overridden with the
/// `RATE_LIMITS` environment variable (JSON). New endpoint groups add a field here.
///
/// Keys: groups applied as route middleware are per IP; the others are checked inside the
/// handler with the key noted (TRD §7.1: per user for signed-in routes, per phone for OTP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimits {
    /// Per-IP backstop applied to every request, including unknown routes.
    pub global: Rule,
    pub fare_estimate: Rule,
    /// `POST /auth/otp/request`, per IP.
    pub otp_request: Rule,
    /// `POST /auth/otp/request`, per phone number (SMS-pumping guard).
    pub otp_phone: Rule,
    /// `POST /auth/otp/verify`, per IP. Each challenge also allows only 5 attempts.
    pub otp_verify: Rule,
    /// `POST /auth/refresh`, per IP and per user.
    pub refresh: Rule,
    /// Every signed-in rider/driver request, per user.
    pub user: Rule,
    /// `POST /uploads`, per user.
    pub uploads: Rule,
    /// `POST /driver/application/submit`, per user.
    pub driver_submit: Rule,
    /// Signed-URL storage requests (local storage backend), per IP.
    pub storage: Rule,
    /// Public legal-document lookups, per IP.
    pub legal: Rule,
    /// `POST /ops/auth/login`, per account + IP.
    pub ops_login: Rule,
    /// Every signed-in ops request, per staff member.
    pub ops: Rule,
    /// Places search and naming the pickup pin, per user.
    pub places: Rule,
    /// `POST /rides/quote`, per user.
    pub quote: Rule,
    /// `POST /rides` (request a ride), per user: a short and a long window.
    pub ride_request: Rule,
    pub ride_request_hourly: Rule,
    /// Driver position updates over HTTP, per user (the WebSocket has its own per-connection limit).
    pub location: Rule,
    /// `POST /rides/{id}/messages`, per user.
    pub chat: Rule,
    /// WebSocket handshakes, per IP.
    pub ws_connect: Rule,
    /// Public Share trip page lookups (`GET /trip-shares/{token}`), per IP (TRD §7.1).
    pub trip_share: Rule,
    /// `POST /rides/{id}/share` (new trip links), per user.
    pub trip_share_create: Rule,
    /// `POST /rides/{id}/report`, per user.
    pub reports: Rule,
}

impl Default for RateLimits {
    fn default() -> Self {
        RateLimits {
            global: Rule::per_minute(300),
            fare_estimate: Rule::per_minute(30),
            otp_request: Rule::per_hour(10),
            otp_phone: Rule { limit: 3, window_s: 600 },
            otp_verify: Rule::per_hour(20),
            refresh: Rule::per_hour(30),
            user: Rule::per_minute(120),
            uploads: Rule::per_hour(20),
            driver_submit: Rule { limit: 5, window_s: 86_400 },
            storage: Rule::per_minute(60),
            legal: Rule::per_minute(60),
            ops_login: Rule { limit: 5, window_s: 900 },
            ops: Rule::per_minute(300),
            places: Rule::per_minute(60),
            quote: Rule::per_minute(30),
            ride_request: Rule::per_minute(5),
            ride_request_hourly: Rule::per_hour(30),
            location: Rule::per_minute(40),
            chat: Rule::per_minute(30),
            ws_connect: Rule::per_minute(30),
            trip_share: Rule::per_minute(60),
            trip_share_create: Rule::per_hour(20),
            reports: Rule { limit: 10, window_s: 86_400 },
        }
    }
}

impl RateLimits {
    pub fn from_json(json: Option<&str>) -> Result<Self, serde_json::Error> {
        json.map_or(Ok(RateLimits::default()), serde_json::from_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { retry_after: Duration },
}

// Atomically: drop entries older than the window, then admit and record this request if under
// the limit; otherwise report when the oldest entry leaves the window.
const SLIDING_WINDOW_LUA: &str = r#"
local key, now, window, limit, member = KEYS[1], tonumber(ARGV[1]), tonumber(ARGV[2]), tonumber(ARGV[3]), ARGV[4]
redis.call('ZREMRANGEBYSCORE', key, 0, now - window)
if redis.call('ZCARD', key) < limit then
  redis.call('ZADD', key, now, member)
  redis.call('PEXPIRE', key, window)
  return {1, 0}
end
local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
return {0, tonumber(oldest[2]) + window - now}
"#;

const REDIS_RETRY_AFTER_MS: u64 = 5_000;

pub struct RateLimiter {
    redis: Option<RedisBackend>,
    memory: MemoryStore,
}

struct RedisBackend {
    client: redis::Client,
    conn: OnceCell<ConnectionManager>,
    script: redis::Script,
    /// While `now < skip_until_ms`, Redis is assumed down and memory is used.
    skip_until_ms: AtomicU64,
}

impl RateLimiter {
    /// `ca_cert` (PEM) replaces the system roots for `rediss://` URLs, for managed Redis that
    /// signs its server certificate with its own CA (Memorystore).
    pub fn new(redis_url: &SecretString, ca_cert: Option<&str>) -> Result<Self, redis::RedisError> {
        let client = match ca_cert {
            Some(pem) => redis::Client::build_with_tls(
                redis_url.expose_secret(),
                redis::TlsCertificates { client_tls: None, root_cert: Some(pem.as_bytes().to_vec()) },
            )?,
            None => redis::Client::open(redis_url.expose_secret())?,
        };
        Ok(RateLimiter {
            redis: Some(RedisBackend {
                client,
                conn: OnceCell::new(),
                script: redis::Script::new(SLIDING_WINDOW_LUA),
                skip_until_ms: AtomicU64::new(0),
            }),
            memory: MemoryStore::default(),
        })
    }

    /// Per-process counters only. For tests and single-instance local runs.
    pub fn memory_only() -> Self {
        RateLimiter { redis: None, memory: MemoryStore::default() }
    }

    pub async fn hit(&self, group: &str, key: &str, rule: Rule) -> Decision {
        let now = now_ms();
        if let Some(r) = &self.redis
            && now >= r.skip_until_ms.load(Ordering::Relaxed)
        {
            match r.hit(group, key, rule, now).await {
                Ok(d) => return d,
                Err(e) => {
                    tracing::warn!(error = %e, "rate limiter: Redis unavailable, using in-memory counters");
                    r.skip_until_ms.store(now + REDIS_RETRY_AFTER_MS, Ordering::Relaxed);
                }
            }
        }
        self.memory.hit(&format!("{group}:{key}"), rule, now)
    }

    /// For limits checked inside handlers (keys other than the IP). The key is hashed or an
    /// opaque ID, never a raw phone number.
    pub async fn check(&self, group: &'static str, key: &str, rule: Rule) -> Result<(), ApiError> {
        match self.hit(group, key, rule).await {
            Decision::Allow => Ok(()),
            Decision::Deny { retry_after } => {
                tracing::warn!(group, "rate limit exceeded");
                Err(ApiError::rate_limited(retry_after))
            }
        }
    }

    /// Readiness probe for Redis (`None` when running memory-only).
    pub async fn ping(&self) -> Option<Result<(), redis::RedisError>> {
        let r = self.redis.as_ref()?;
        Some(
            async {
                let mut c = r.connection().await?;
                redis::cmd("PING").query_async::<String>(&mut c).await.map(|_| ())
            }
            .await,
        )
    }
}

impl RedisBackend {
    async fn connection(&self) -> Result<ConnectionManager, redis::RedisError> {
        let cfg = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_millis(500)))
            .set_response_timeout(Some(Duration::from_millis(250)))
            .set_number_of_retries(1);
        self.conn.get_or_try_init(|| ConnectionManager::new_with_config(self.client.clone(), cfg)).await.cloned()
    }

    async fn hit(&self, group: &str, key: &str, rule: Rule, now: u64) -> Result<Decision, redis::RedisError> {
        let mut conn = self.connection().await?;
        let window_ms = rule.window_s * 1000;
        let member = format!("{now}-{:016x}", rand::random::<u64>());
        let (allowed, retry_ms): (i64, i64) = self
            .script
            .key(format!("rl:{group}:{key}"))
            .arg(now)
            .arg(window_ms)
            .arg(rule.limit)
            .arg(member)
            .invoke_async(&mut conn)
            .await?;
        Ok(if allowed == 1 { Decision::Allow } else { Decision::Deny { retry_after: Duration::from_millis(retry_ms.max(1) as u64) } })
    }
}

#[derive(Default)]
struct MemoryStore {
    windows: Mutex<HashMap<String, (u64, VecDeque<u64>)>>,
}

const MEMORY_PRUNE_AT: usize = 50_000;

impl MemoryStore {
    fn hit(&self, key: &str, rule: Rule, now: u64) -> Decision {
        let window_ms = rule.window_s * 1000;
        let mut map = self.windows.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= MEMORY_PRUNE_AT {
            map.retain(|_, (w, q)| q.back().is_some_and(|&t| t + *w > now));
        }
        let (w, q) = map.entry(key.to_owned()).or_insert_with(|| (window_ms, VecDeque::new()));
        *w = window_ms;
        while q.front().is_some_and(|&t| t + window_ms <= now) {
            q.pop_front();
        }
        if q.len() < rule.limit as usize {
            q.push_back(now);
            Decision::Allow
        } else {
            let oldest = *q.front().expect("limit > 0 so queue is non-empty");
            Decision::Deny { retry_after: Duration::from_millis(oldest + window_ms - now) }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Middleware state: one endpoint group's rule, keyed by client IP.
#[derive(Clone)]
pub struct Limit {
    pub limiter: Arc<RateLimiter>,
    pub group: &'static str,
    pub rule: Rule,
    pub trusted_proxy_hops: usize,
}

/// The one route no limit may refuse.
pub const SOS_PATH: &str = "/v1/safety/sos";

pub async fn enforce(State(l): State<Limit>, req: Request, next: Next) -> Response {
    let ip = client_ip(&req, l.trusted_proxy_hops);
    match l.limiter.hit(l.group, &ip.to_string(), l.rule).await {
        Decision::Allow => next.run(req).await,
        // Safety must never be throttled (TRD §5): an SOS over the limit is logged, not refused.
        Decision::Deny { .. } if req.uri().path() == SOS_PATH => {
            tracing::warn!(group = l.group, "rate limit exceeded by an SOS; let through");
            next.run(req).await
        }
        Decision::Deny { retry_after } => {
            // The IP is not logged: the group is enough to spot abuse patterns in metrics.
            tracing::warn!(group = l.group, "rate limit exceeded");
            ApiError::rate_limited(retry_after).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_window_slides() {
        let m = MemoryStore::default();
        let r = Rule { limit: 2, window_s: 10 };
        assert_eq!(m.hit("k", r, 0), Decision::Allow);
        assert_eq!(m.hit("k", r, 1_000), Decision::Allow);
        assert_eq!(m.hit("k", r, 2_000), Decision::Deny { retry_after: Duration::from_millis(8_000) });
        assert_eq!(m.hit("other", r, 2_000), Decision::Allow);
        assert_eq!(m.hit("k", r, 10_000), Decision::Allow); // first entry has left the window
    }

    #[test]
    fn overrides_merge_with_defaults() {
        let l = RateLimits::from_json(Some(r#"{"fare_estimate":{"limit":5,"window_s":60}}"#)).unwrap();
        assert_eq!(l.fare_estimate.limit, 5);
        assert_eq!(l.global, RateLimits::default().global);
        assert!(RateLimits::from_json(Some(r#"{"typo":{"limit":1,"window_s":1}}"#)).is_err());
    }
}
