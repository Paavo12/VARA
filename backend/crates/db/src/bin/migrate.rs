//! Applies the embedded migrations and exits. Staging and production run it as a one-off job
//! before each new API and worker version starts, with the migrator's DATABASE_URL (the API
//! and worker connect as a role that cannot alter the schema).

use std::process::ExitCode;

use secrecy::SecretString;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let Some(url) = std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty()) else {
        eprintln!("DATABASE_URL is required");
        return ExitCode::FAILURE;
    };
    let pool = match fuber_db::pool(&SecretString::from(url)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("DATABASE_URL is not valid: {e}");
            return ExitCode::FAILURE;
        }
    };
    match fuber_db::MIGRATOR.run(&pool).await {
        Ok(()) => {
            let applied = fuber_db::MIGRATOR.iter().count();
            println!("migrations up to date ({applied} known)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("migrations failed: {e}");
            ExitCode::FAILURE
        }
    }
}
