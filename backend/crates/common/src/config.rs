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
    /// PEM CA certificate for `rediss://` when the server's CA is not publicly trusted
    /// (Memorystore). Public, not a secret.
    pub redis_ca_cert: Option<String>,
    /// Exact origins allowed by CORS (web apps and Capacitor shells).
    pub cors_origins: Vec<String>,
    /// Number of reverse proxies in front of the API that append to `X-Forwarded-For`.
    /// Zero means the peer address is the client (no proxy headers are trusted).
    pub trusted_proxy_hops: usize,
    /// Optional JSON overriding default rate limits, e.g. `{"global":{"limit":600,"window_s":60}}`.
    pub rate_limits_json: Option<String>,
    /// Keys the HMACs for phone numbers, OTP codes, refresh tokens and national IDs.
    pub hash_secret: SecretString,
    /// Encrypts the 🔒 columns (AES-256-GCM).
    pub data_key: SecretString,
    /// Signs access tokens (Ed25519), with its key id.
    pub jwt_kid: String,
    pub jwt_secret: SecretString,
    /// The previous signing key, still accepted for verification during rotation.
    pub jwt_previous: Option<(String, SecretString)>,
    pub sms_provider: SmsProvider,
    pub storage: StorageConfig,
    pub virus_scan: VirusScan,
    pub routing: RoutingConfig,
    /// Base of the live trip links texted to emergency contacts, e.g. `https://vara.example/t`.
    pub trip_link_base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmsProvider {
    /// Records messages in memory and, in local development only, prints them to the log.
    /// Real providers are added once the SMS vendors are chosen (PRD §10).
    Fake,
}

/// Road routing for quotes and ETAs (TRD §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingConfig {
    /// The self-hosted OSRM server, e.g. `http://osrm:5000`.
    Osrm(String),
    /// Straight line × 1.3 at free-flow speed. Local development only.
    Estimate,
}

#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// Files on local disk, uploaded and downloaded through signed URLs that the API serves
    /// itself. For development and single-machine test environments, never production.
    /// S3-compatible storage is added once the cloud provider is chosen (PRD §10).
    Local { dir: std::path::PathBuf, public_url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirusScan {
    /// ClamAV daemon at `host:port` (INSTREAM protocol).
    ClamAv(String),
    /// No scanning. Local development only.
    Disabled,
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
        let hash_secret = required("HASH_SECRET");
        let data_key = required("DATA_KEY");
        let jwt_secret = required("JWT_SECRET");

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
        let redis_ca_cert = get("REDIS_CA_CERT").filter(|v| !v.trim().is_empty());
        if let Some(pem) = &redis_ca_cert {
            if !pem.contains("-----BEGIN CERTIFICATE-----") {
                errors.push("REDIS_CA_CERT must be a PEM certificate".into());
            }
            if !redis_url.starts_with("rediss://") {
                errors.push("REDIS_CA_CERT needs a TLS REDIS_URL (rediss://)".into());
            }
        }

        let jwt_kid = get("JWT_KID").unwrap_or_else(|| "k1".into());
        let jwt_previous = match (get("JWT_PREVIOUS_KID"), get("JWT_PREVIOUS_SECRET")) {
            (Some(k), Some(s)) if !k.is_empty() && !s.is_empty() => Some((k, s)),
            (None, None) => None,
            _ => {
                errors.push("JWT_PREVIOUS_KID and JWT_PREVIOUS_SECRET must be set together".into());
                None
            }
        };
        if jwt_previous.as_ref().is_some_and(|(k, _)| *k == jwt_kid) {
            errors.push("JWT_PREVIOUS_KID must differ from JWT_KID".into());
        }

        let mut secrets = vec![("HASH_SECRET", &hash_secret), ("DATA_KEY", &data_key), ("JWT_SECRET", &jwt_secret)];
        if let Some((_, s)) = &jwt_previous {
            secrets.push(("JWT_PREVIOUS_SECRET", s));
        }
        for (key, value) in secrets {
            if !value.is_empty() && value.chars().count() < 32 {
                errors.push(format!("{key} must be at least 32 random characters"));
            }
            if env != Some(Environment::Local) && value.contains("local-only") {
                errors.push(format!("{key} uses the development example value outside APP_ENV=local"));
            }
        }

        let sms_provider = match get("SMS_PROVIDER").as_deref().unwrap_or("fake") {
            "fake" => SmsProvider::Fake,
            other => {
                errors.push(format!("SMS_PROVIDER must be fake (got {other:?}); real providers arrive with the vendor decision"));
                SmsProvider::Fake
            }
        };

        let storage = match get("STORAGE_BACKEND").as_deref().unwrap_or("local") {
            "local" => StorageConfig::Local {
                dir: get("STORAGE_DIR").unwrap_or_else(|| "var/storage".into()).into(),
                public_url: get("PUBLIC_API_URL").unwrap_or_else(|| format!("http://{bind_addr}")).trim_end_matches('/').to_owned(),
            },
            other => {
                errors.push(format!("STORAGE_BACKEND must be local (got {other:?})"));
                StorageConfig::Local { dir: "var/storage".into(), public_url: String::new() }
            }
        };

        let virus_scan = match (get("VIRUS_SCAN").as_deref().unwrap_or("disabled"), get("CLAMAV_ADDR")) {
            ("clamav", Some(addr)) if !addr.is_empty() => VirusScan::ClamAv(addr),
            ("clamav", _) => {
                errors.push("VIRUS_SCAN=clamav needs CLAMAV_ADDR (host:port)".into());
                VirusScan::Disabled
            }
            ("disabled", _) => VirusScan::Disabled,
            (other, _) => {
                errors.push(format!("VIRUS_SCAN must be clamav or disabled (got {other:?})"));
                VirusScan::Disabled
            }
        };

        let routing = match (get("ROUTING").as_deref().unwrap_or("estimate"), get("OSRM_URL")) {
            // OSRM runs on the private network next to the API, so plain HTTP.
            ("osrm", Some(url)) if url.starts_with("http://") => RoutingConfig::Osrm(url),
            ("osrm", _) => {
                errors.push("ROUTING=osrm needs OSRM_URL (http://host:port)".into());
                RoutingConfig::Estimate
            }
            ("estimate", _) => RoutingConfig::Estimate,
            (other, _) => {
                errors.push(format!("ROUTING must be osrm or estimate (got {other:?})"));
                RoutingConfig::Estimate
            }
        };

        // SOS texts and Share trip links carry this address: outside local it must be set, and to a
        // real domain, or emergency contacts get a link that opens nothing.
        let trip_link_base = match get("TRIP_LINK_BASE") {
            Some(v) => v,
            None if env.is_some_and(|e| e != Environment::Local) => {
                errors.push("TRIP_LINK_BASE is required outside APP_ENV=local (the website's /t address)".into());
                String::new()
            }
            None => "https://vara.example/t".into(),
        }
        .trim_end_matches('/')
        .to_owned();
        let local_http = env == Some(Environment::Local) && trip_link_base.starts_with("http://");
        if !trip_link_base.is_empty() && !(trip_link_base.starts_with("https://") || local_http) {
            errors.push(format!("TRIP_LINK_BASE must be an https:// URL (got {trip_link_base:?})"));
        }
        if env.is_some_and(|e| e != Environment::Local) && placeholder_host(&trip_link_base) {
            errors.push(format!("TRIP_LINK_BASE uses a placeholder domain outside APP_ENV=local (got {trip_link_base:?})"));
        }

        if env == Some(Environment::Production) {
            if sms_provider == SmsProvider::Fake {
                errors.push("SMS_PROVIDER=fake is not allowed in production".into());
            }
            if matches!(storage, StorageConfig::Local { .. }) {
                errors.push("STORAGE_BACKEND=local is not allowed in production".into());
            }
        }
        if env.is_some_and(|e| e != Environment::Local) && virus_scan == VirusScan::Disabled {
            errors.push("VIRUS_SCAN=disabled is only allowed with APP_ENV=local".into());
        }
        if env.is_some_and(|e| e != Environment::Local) && routing == RoutingConfig::Estimate {
            errors.push("ROUTING=estimate is only allowed with APP_ENV=local".into());
        }

        if !errors.is_empty() {
            return Err(ConfigError(errors));
        }
        Ok(Config {
            env: env.expect("checked above"),
            bind_addr,
            database_url: database_url.into(),
            redis_url: redis_url.into(),
            redis_ca_cert,
            cors_origins,
            trusted_proxy_hops,
            rate_limits_json: get("RATE_LIMITS"),
            hash_secret: hash_secret.into(),
            data_key: data_key.into(),
            jwt_kid,
            jwt_secret: jwt_secret.into(),
            jwt_previous: jwt_previous.map(|(k, s)| (k, s.into())),
            sms_provider,
            storage,
            virus_scan,
            routing,
            trip_link_base,
        })
    }
}

/// Hosts reserved for examples and testing (RFC 2606), which no real link can use.
fn placeholder_host(url: &str) -> bool {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', ':']).next().unwrap_or("").to_ascii_lowercase();
    host == "localhost" || [".example", ".test", ".invalid", ".localhost"].iter().any(|t| host.ends_with(t) || host == t[1..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SECRETS: [(&str, &str); 3] = [
        ("HASH_SECRET", "test-hash-secret-0123456789abcdef0123"),
        ("DATA_KEY", "test-data-key-0123456789abcdef0123456"),
        ("JWT_SECRET", "test-jwt-secret-0123456789abcdef01234"),
    ];

    /// `pairs` plus valid secrets (entries in `pairs` win).
    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        load_raw(&[pairs, &SECRETS].concat())
    }

    fn load_raw(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut m: HashMap<String, String> = HashMap::new();
        for (k, v) in pairs.iter().rev() {
            m.insert(k.to_string(), v.to_string());
        }
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn reports_every_missing_variable() {
        let e = load_raw(&[]).unwrap_err();
        assert_eq!(e.0.len(), 6, "{e}");
    }

    #[test]
    fn secrets_are_not_printed() {
        let c = load(&[("APP_ENV", "local"), ("DATABASE_URL", "postgres://u:hunter2@db/x"), ("REDIS_URL", "redis://r")]).unwrap();
        let printed = format!("{c:?}");
        assert!(!printed.contains("hunter2"));
        for (_, v) in SECRETS {
            assert!(!printed.contains(v));
        }
    }

    #[test]
    fn production_rejects_dev_credentials_plain_redis_and_fakes() {
        let e = load(&[
            ("APP_ENV", "production"),
            ("DATABASE_URL", "postgres://postgres:password@db/x"),
            ("REDIS_URL", "redis://r"),
            ("VIRUS_SCAN", "clamav"),
            ("CLAMAV_ADDR", "clamav:3310"),
        ])
        .unwrap_err();
        // Dev password, plain Redis, fake SMS gateway, local storage, estimated routing, no trip link.
        assert_eq!(e.0.len(), 6, "{e}");
    }

    #[test]
    fn short_or_example_secrets_rejected() {
        let short = load_raw(&[
            ("APP_ENV", "local"),
            ("DATABASE_URL", "x"),
            ("REDIS_URL", "y"),
            ("HASH_SECRET", "short"),
            ("DATA_KEY", "short"),
            ("JWT_SECRET", "short"),
        ]);
        assert_eq!(short.unwrap_err().0.len(), 3);
        let example = load(&[
            ("APP_ENV", "staging"),
            ("DATABASE_URL", "postgres://u:from-vault@db/x"),
            ("REDIS_URL", "rediss://r"),
            ("VIRUS_SCAN", "clamav"),
            ("CLAMAV_ADDR", "clamav:3310"),
            ("ROUTING", "osrm"),
            ("OSRM_URL", "http://osrm:5000"),
            ("TRIP_LINK_BASE", "https://vara.na/t"),
            ("HASH_SECRET", "local-only-0123456789abcdef0123456789"),
        ]);
        assert_eq!(example.unwrap_err().0.len(), 1);
    }

    #[test]
    fn virus_scanning_required_outside_local() {
        let e = load(&[("APP_ENV", "staging"), ("DATABASE_URL", "postgres://u:from-vault@db/x"), ("REDIS_URL", "rediss://r")]).unwrap_err();
        assert!(e.to_string().contains("VIRUS_SCAN"), "{e}");
    }

    #[test]
    fn real_routing_required_outside_local() {
        let staging = [("APP_ENV", "staging"), ("DATABASE_URL", "postgres://u:from-vault@db/x"), ("REDIS_URL", "rediss://r")];
        let e = load(&staging).unwrap_err();
        assert!(e.to_string().contains("ROUTING=estimate"), "{e}");
        let e = load(&[&staging[..], &[("ROUTING", "osrm"), ("OSRM_URL", "https://maps.example.com")]].concat()).unwrap_err();
        assert!(e.to_string().contains("OSRM_URL"), "{e}");
        let local = load(&[("APP_ENV", "local"), ("DATABASE_URL", "x"), ("REDIS_URL", "y")]).unwrap();
        assert_eq!(local.routing, RoutingConfig::Estimate);
    }

    #[test]
    fn redis_ca_cert_needs_pem_and_tls() {
        let base = [("APP_ENV", "local"), ("DATABASE_URL", "x")];
        let pem = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        let c = load(&[&base[..], &[("REDIS_URL", "rediss://10.0.0.3:6378"), ("REDIS_CA_CERT", pem)]].concat()).unwrap();
        assert_eq!(c.redis_ca_cert.as_deref(), Some(pem));
        let e = load(&[&base[..], &[("REDIS_URL", "redis://r"), ("REDIS_CA_CERT", "not a cert")]].concat()).unwrap_err();
        assert_eq!(e.0.len(), 2, "{e}");
    }

    #[test]
    fn trip_link_base_must_be_real_outside_local() {
        let staging = [
            ("APP_ENV", "staging"),
            ("DATABASE_URL", "postgres://u:from-vault@db/x"),
            ("REDIS_URL", "rediss://r"),
            ("VIRUS_SCAN", "clamav"),
            ("CLAMAV_ADDR", "clamav:3310"),
            ("ROUTING", "osrm"),
            ("OSRM_URL", "http://osrm:5000"),
        ];
        let e = load(&staging).unwrap_err();
        assert!(e.to_string().contains("TRIP_LINK_BASE is required"), "{e}");
        for bad in ["https://vara.example/t", "https://localhost/t", "https://staging.vara.test/t", "http://vara.na/t"] {
            assert!(load(&[&staging[..], &[("TRIP_LINK_BASE", bad)]].concat()).is_err(), "{bad}");
        }
        let c = load(&[&staging[..], &[("TRIP_LINK_BASE", "https://staging.vara.na/t/")]].concat()).unwrap();
        assert_eq!(c.trip_link_base, "https://staging.vara.na/t");
        let local = load(&[("APP_ENV", "local"), ("DATABASE_URL", "x"), ("REDIS_URL", "y")]).unwrap();
        assert_eq!(local.trip_link_base, "https://vara.example/t");
    }

    #[test]
    fn wildcard_cors_rejected() {
        assert!(load(&[("APP_ENV", "local"), ("DATABASE_URL", "x"), ("REDIS_URL", "y"), ("CORS_ORIGINS", "*")]).is_err());
    }
}
