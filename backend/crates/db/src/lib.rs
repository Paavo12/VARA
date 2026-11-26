//! Database access. Queries use sqlx with bound parameters only — never SQL built from strings.

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgPool, postgres::PgPoolOptions};

pub mod drivers;
pub mod jobs;
pub mod presence;
pub mod push;
pub mod rides;
pub mod safety;
pub mod settings;

pub use sqlx;

/// Migrations in `backend/migrations`, embedded in the binary. Production runs them from CI
/// with the `fuber_migrator` role; the API and worker never alter the schema.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Creates a pool without connecting, so the process can start (and report not-ready) while
/// the database is unavailable.
pub fn pool(url: &SecretString) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new().max_connections(20).acquire_timeout(Duration::from_secs(3)).connect_lazy(url.expose_secret())
}

pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}
