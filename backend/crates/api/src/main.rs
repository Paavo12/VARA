use std::{net::SocketAddr, process::ExitCode};

use fuber_api::{
    AppState,
    rate_limit::{RateLimiter, RateLimits},
};
use fuber_common::{Config, telemetry};

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

    let limits = match RateLimits::from_json(config.rate_limits_json.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "RATE_LIMITS is not valid");
            return ExitCode::FAILURE;
        }
    };
    let (db, limiter) = match (fuber_db::pool(&config.database_url), RateLimiter::new(&config.redis_url)) {
        (Ok(db), Ok(l)) => (db, l),
        (Err(e), _) => {
            tracing::error!(error = %e, "DATABASE_URL is not valid");
            return ExitCode::FAILURE;
        }
        (_, Err(e)) => {
            tracing::error!(error = %e, "REDIS_URL is not valid");
            return ExitCode::FAILURE;
        }
    };

    let addr = config.bind_addr;
    let state = AppState::new(config, db, limiter, limits);
    let app = fuber_api::router(state);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %addr, "cannot bind");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(%addr, "FUber API listening");
    let served =
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown_signal()).await;
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "server error");
            ExitCode::FAILURE
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
