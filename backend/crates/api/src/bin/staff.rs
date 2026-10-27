//! Creates and disables ops staff accounts. There is no sign-up for staff by design.
//!
//! ```text
//! FUBER_STAFF_PASSWORD='…' cargo run -p fuber-api --bin staff -- create reviewer@fuber.app "Ndapewa Amutenya" ops_reviewer
//! cargo run -p fuber-api --bin staff -- disable reviewer@fuber.app
//! ```
//!
//! `create` prints an `otpauth://` link once: open it (or its QR code) in an authenticator app.
//! Uses the same DATABASE_URL and DATA_KEY as the API.

use std::process::ExitCode;

use argon2::{Argon2, password_hash::PasswordHasher};
use fuber_common::{
    Config,
    crypto::{Sealer, random_bytes, totp},
    validate::{Email, PersonName},
};

const ROLES: [&str; 4] = ["ops_reviewer", "ops_safety", "ops_support", "ops_admin"];
const TOTP_AAD: &str = "staff_users.totp_secret";

#[tokio::main]
async fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Vec<String>) -> Result<(), String> {
    let config = Config::from_env().map_err(|e| e.to_string())?;
    let db = fuber_db::pool(&config.database_url).map_err(|e| e.to_string())?;
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["create", email, name, roles] => {
            let email = Email::parse(email).map_err(|e| e.0.to_string())?;
            let name = PersonName::parse(name).map_err(|e| e.0.to_string())?;
            let roles: Vec<&str> = roles.split(',').map(str::trim).collect();
            if roles.is_empty() || roles.iter().any(|r| !ROLES.contains(r)) {
                return Err(format!("roles must be a comma-separated list of {}", ROLES.join(", ")));
            }
            let password = std::env::var("FUBER_STAFF_PASSWORD").map_err(|_| "set FUBER_STAFF_PASSWORD to the initial password")?;
            if password.chars().count() < 12 {
                return Err("the password must be at least 12 characters".into());
            }
            let hash = Argon2::default().hash_password(password.as_bytes()).map_err(|e| e.to_string())?.to_string();
            let secret = random_bytes::<20>();
            let sealed = Sealer::new(&config.data_key).seal(TOTP_AAD, &secret);
            sqlx::query(
                "INSERT INTO staff_users (email, full_name, password_hash, totp_secret, roles) VALUES ($1, $2, $3, $4, $5::staff_role[])",
            )
            .bind(email.as_str())
            .bind(name.as_str())
            .bind(hash)
            .bind(sealed)
            .bind(&roles)
            .execute(&db)
            .await
            .map_err(|e| e.to_string())?;
            let label = format!("FUber%20Ops:{}", email.as_str().replace('@', "%40"));
            println!("Created {} ({}).", email.as_str(), roles.join(", "));
            println!("Add this to an authenticator app now; it is not shown again:");
            println!("otpauth://totp/{label}?secret={}&issuer=FUber%20Ops&algorithm=SHA1&digits=6&period=30", totp::base32(&secret));
            Ok(())
        }
        ["disable", email] => {
            let email = Email::parse(email).map_err(|e| e.0.to_string())?;
            let n = sqlx::query("UPDATE staff_users SET disabled_at = now() WHERE email = $1 AND disabled_at IS NULL")
                .bind(email.as_str())
                .execute(&db)
                .await
                .map_err(|e| e.to_string())?
                .rows_affected();
            sqlx::query("UPDATE staff_sessions SET revoked_at = now() WHERE revoked_at IS NULL AND staff_id IN (SELECT id FROM staff_users WHERE email = $1)")
                .bind(email.as_str())
                .execute(&db)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("no active staff account with that email".into());
            }
            println!("Disabled {} and ended their sessions.", email.as_str());
            Ok(())
        }
        _ => Err("usage: staff create <email> <full name> <role[,role]> | staff disable <email>".into()),
    }
}
