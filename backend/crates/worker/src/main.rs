//! VARA background worker process: runs queued jobs, matching, the daily document check and
//! hourly clean-up (see the library for what each does). Several workers can run at once.

use std::{process::ExitCode, time::Duration};

use fuber_common::{
    Config, Environment, RoutingConfig, SmsProvider, VirusScan,
    crypto::{Hasher, Sealer},
    telemetry,
};
use fuber_integrations::{
    routing::Routing,
    sms::{FakeSms, SmsGateway},
    storage::ObjectStore,
    virus::VirusScanner,
};
use fuber_worker::Ctx;
use time::OffsetDateTime;

const POLL: Duration = Duration::from_secs(2);
/// Offers expire after 15 s; checking every second keeps the next driver's wait short.
const DISPATCH: Duration = Duration::from_secs(1);
const HOUR: Duration = Duration::from_secs(3600);

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    telemetry::init(config.env);
    let db = match fuber_db::pool(&config.database_url) {
        Ok(db) => db,
        Err(e) => {
            tracing::error!(error = %e, "DATABASE_URL is not valid");
            return ExitCode::FAILURE;
        }
    };
    let ctx = Ctx {
        db,
        sealer: Sealer::new(&config.data_key),
        hasher: Hasher::new(&config.hash_secret),
        trip_link_base: config.trip_link_base.clone(),
        store: ObjectStore::from_config(&config, Hasher::new(&config.hash_secret)),
        scanner: match &config.virus_scan {
            VirusScan::ClamAv(addr) => VirusScanner::ClamAv(addr.clone()),
            VirusScan::Disabled => {
                tracing::warn!("virus scanning is disabled (local development only)");
                VirusScanner::Disabled
            }
        },
        sms: match config.sms_provider {
            SmsProvider::Fake => SmsGateway::Fake(FakeSms::new(config.env == Environment::Local)),
        },
        routing: match &config.routing {
            RoutingConfig::Osrm(url) => Routing::osrm(url),
            RoutingConfig::Estimate => {
                tracing::warn!("routing uses straight-line estimates (local development only)");
                Routing::Estimate
            }
        },
    };

    tracing::info!("VARA worker started");
    let mut poll = tokio::time::interval(POLL);
    let mut dispatch = tokio::time::interval(DISPATCH);
    dispatch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut hourly = tokio::time::interval(HOUR);
    let mut last_document_check = None;
    loop {
        tokio::select! {
            _ = poll.tick() => {
                if let Err(e) = fuber_worker::drain(&ctx).await {
                    tracing::warn!(error = %e, "job queue unavailable");
                }
            }
            _ = dispatch.tick() => {
                if let Err(e) = fuber_worker::dispatch::tick(&ctx).await {
                    tracing::warn!(error = %e, "matching tick failed");
                }
            }
            _ = hourly.tick() => {
                if let Err(e) = fuber_worker::cleanup(&ctx).await {
                    tracing::warn!(error = %e, "cleanup failed");
                }
                // Once per calendar day (UTC+2 is close enough: checks are by date, not hour).
                let today = OffsetDateTime::now_utc().date();
                if last_document_check != Some(today) {
                    match fuber_worker::check_documents(&ctx, today).await {
                        Ok(()) => last_document_check = Some(today),
                        Err(e) => tracing::warn!(error = %e, "document expiry check failed"),
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    tracing::info!("VARA worker stopped");
    ExitCode::SUCCESS
}
