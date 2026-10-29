//! Shared building blocks: configuration and secrets, validated input types, telemetry.

pub mod config;
pub mod crypto;
pub mod telemetry;
pub mod validate;

pub use config::{Config, ConfigError, Environment, RoutingConfig, SmsProvider, StorageConfig, VirusScan};
