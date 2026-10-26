//! Logging setup and redaction helpers (TRD §7.6).
//!
//! Rules: never log request or response bodies, tokens, OTPs, PINs, full phone numbers or
//! exact addresses. Log the matched route (not the raw URI, which can carry share tokens).

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::Environment;

pub fn init(env: Environment) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn"));
    let registry = tracing_subscriber::registry().with(filter);
    match env {
        Environment::Local => registry.with(fmt::layer().compact()).init(),
        _ => registry.with(fmt::layer().json().flatten_event(true).with_current_span(false)).init(),
    }
}

/// `+264811234567` → `+264 81 *** **67`. Use whenever a phone number has to appear in a log.
pub fn mask_phone(e164: &str) -> String {
    let digits: Vec<char> = e164.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 7 || !e164.starts_with("+264") {
        return "***".into();
    }
    let n = digits.len();
    format!("+264 {}{} *** **{}{}", digits[3], digits[4], digits[n - 2], digits[n - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_phone() {
        assert_eq!(mask_phone("+264811234567"), "+264 81 *** **67");
        assert_eq!(mask_phone("12"), "***");
    }
}
