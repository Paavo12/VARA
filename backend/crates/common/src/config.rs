//! Configuration from environment variables (TRD §7.3).
//!
//! In staging and production the variables are injected from the cloud secret manager; a
//! local `.env` file is read only when `APP_ENV=local`. Secrets are wrapped in
//! [`SecretString`] so they never appear in `Debug` output or logs. Startup fails, listing
//! every problem at once, if anything required is missing.

use std::{env, net::SocketAddr};

use secrecy::SecretString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Local,
    Staging,
    Production,
}

#[derive(Debug)]
pub struct Config {
    pub env: Environment,
    pub bind_addr: SocketAddr,
    pub database_url: SecretString,
    pub redis_url: SecretString,
    /// Exact origins allowed by CORS (web apps and Capacitor shells).
    pub cors_origins: Vec<String>,
    /// Number of reverse proxies in front of the API that append to `X-Forwarded-For`.
    /// Zero means the peer address is the client (no proxy headers are trusted).
    pub trusted_proxy_hops: usize,
    /// Optional JSON overriding default rate limits, e.g. `{"global":{"limit":600,"window_s":60}}`.
    pub rate_limits_json: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid configuration:\n  - {}", .0.join("\n  - "))]
pub struct ConfigError(pub Vec<String>);

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        if env::var("APP_ENV").as_deref() == Ok("local") {
            // Missing .env is fine; real environments never use one.
            let _ = dotenvy::dotenv();
        }
        Self::from_lookup(|k| env::var(k).ok())
    }

    /// Builds the config from any key → value lookup (lets tests avoid touching process env).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let mut errors = Vec::new();
        let mut required = |key: &str| {
            get(key).filter(|v| !v.trim().is_empty()).unwrap_or_else(|| {
                errors.push(format!("{key} is required"));
                String::new()
            })
        };

        let env_name = required("APP_ENV");
        let database_url = required("DATABASE_URL");
        let redis_url = required("REDIS_URL");

        let env = match env_name.as_str() {
            "local" => Some(Environment::Local),
            "staging" => Some(Environment::Staging),
            "production" => Some(Environment::Production),
            "" => None,
            other => {
                errors.push(format!("APP_ENV must be local, staging or production (got {other:?})"));
                None
            }
        };

        let bind_addr = get("BIND_ADDR").unwrap_or_else(|| "0.0.0.0:8080".into());
        let bind_addr = bind_addr.parse().unwrap_or_else(|_| {
            errors.push(format!("BIND_ADDR is not a socket address: {bind_addr:?}"));
            SocketAddr::from(([0, 0, 0, 0], 8080))
        });

        let cors_origins: Vec<String> =
            get("CORS_ORIGINS").unwrap_or_default().split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        if cors_origins.iter().any(|o| o == "*") {
            errors.push("CORS_ORIGINS must list exact origins; '*' is not allowed".into());
        }

        let trusted_proxy_hops = match get("TRUSTED_PROXY_HOPS").as_deref().unwrap_or("0").parse() {
            Ok(n) => n,
            Err(_) => {
                errors.push("TRUSTED_PROXY_HOPS must be a whole number".into());
                0
            }
        };

        if env != Some(Environment::Local) {
            for (key, url) in [("DATABASE_URL", &database_url), ("REDIS_URL", &redis_url)] {
                if url.contains("password@") || url.contains(":postgres@") {
                    errors.push(format!("{key} uses a development password outside APP_ENV=local"));
                }
            }
            if redis_url.starts_with("redis://") {
                errors.push("REDIS_URL must use TLS (rediss://) outside APP_ENV=local".into());
            }
        }

        if !errors.is_empty() {
            return Err(ConfigError(errors));
        }
        Ok(Config {
            env: env.expect("checked above"),
            bind_addr,
            database_url: database_url.into(),
            redis_url: redis_url.into(),
            cors_origins,
            trusted_proxy_hops,
            rate_limits_json: get("RATE_LIMITS"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn reports_every_missing_variable() {
        let e = load(&[]).unwrap_err();
        assert_eq!(e.0.len(), 3, "{e}");
    }

    #[test]
    fn secrets_are_not_printed() {
        let c = load(&[("APP_ENV", "local"), ("DATABASE_URL", "postgres://u:hunter2@db/x"), ("REDIS_URL", "redis://r")]).unwrap();
        assert!(!format!("{c:?}").contains("hunter2"));
    }

    #[test]
    fn production_rejects_dev_credentials_and_plain_redis() {
        let e = load(&[("APP_ENV", "production"), ("DATABASE_URL", "postgres://postgres:password@db/x"), ("REDIS_URL", "redis://r")])
            .unwrap_err();
        assert_eq!(e.0.len(), 2, "{e}");
    }

    #[test]
    fn wildcard_cors_rejected() {
        assert!(load(&[("APP_ENV", "local"), ("DATABASE_URL", "x"), ("REDIS_URL", "y"), ("CORS_ORIGINS", "*")]).is_err());
    }
}
