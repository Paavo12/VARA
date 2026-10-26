//! FUber background worker: offer timeouts, notifications, document-expiry checks, retention
//! jobs (TRD §2.1). Phase 0 ships the process shell only; jobs arrive with the features that
//! need them, starting in Phase 1.

use std::{process::ExitCode, time::Duration};

use fuber_common::{Config, telemetry};

const HEARTBEAT: Duration = Duration::from_secs(60);

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

    tracing::info!("FUber worker started");
    let mut tick = tokio::time::interval(HEARTBEAT);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if let Err(e) = fuber_db::ping(&db).await {
                    tracing::warn!(error = %e, "database unavailable");
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    tracing::info!("FUber worker stopped");
    ExitCode::SUCCESS
}
