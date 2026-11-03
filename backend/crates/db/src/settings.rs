//! Matching and booking parameters editable by ops without a release (PRD D-6). Missing or
//! malformed values fall back to the launch defaults, so a bad edit never stops dispatch.

use sqlx::PgExecutor;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dispatch {
    pub radius_m: f64,
    pub offer_ms: i64,
    pub arrive_m: f64,
    pub location_stale_s: i64,
    pub max_candidates: i64,
    pub fuzz_min_m: f64,
    pub fuzz_max_m: f64,
    pub quote_ttl_s: i64,
    pub schedule_min_s: i64,
    pub schedule_max_s: i64,
    pub schedule_lead_s: i64,
    pub lockout: LockoutRules,
}

/// How long a driver gets no requests after a PIN lockout (PRD T-3, App Flow §7). The hold
/// ends by itself so a slow review never leaves her without work; only a pattern of lockouts
/// waits for the safety desk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LockoutRules {
    pub hold_s: i64,
    /// This many lockouts within `window_days` (the latest included) hold her until reviewed.
    pub repeat: i64,
    pub window_days: i64,
}

impl Default for Dispatch {
    fn default() -> Self {
        Dispatch {
            radius_m: 3000.0,
            offer_ms: 15_000,
            arrive_m: 50.0,
            location_stale_s: 60,
            max_candidates: 25,
            fuzz_min_m: 100.0,
            fuzz_max_m: 200.0,
            quote_ttl_s: 300,
            schedule_min_s: 1800,
            schedule_max_s: 604_800,
            schedule_lead_s: 900,
            lockout: LockoutRules { hold_s: 900, repeat: 3, window_days: 7 },
        }
    }
}

impl Dispatch {
    pub async fn load<'e>(db: impl PgExecutor<'e>) -> Result<Dispatch, sqlx::Error> {
        let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT key, value FROM settings WHERE key LIKE 'dispatch.%' OR key LIKE 'booking.%' OR key LIKE 'safety.lockout_%'",
        )
        .fetch_all(db)
        .await?;
        let mut d = Dispatch::default();
        for (key, value) in rows {
            let Some(n) = value.as_f64().filter(|n| n.is_finite() && *n >= 0.0) else {
                tracing::warn!(%key, "setting is not a non-negative number; using the default");
                continue;
            };
            match key.as_str() {
                "dispatch.radius_m" => d.radius_m = n,
                "dispatch.offer_ms" => d.offer_ms = n as i64,
                "dispatch.arrive_m" => d.arrive_m = n,
                "dispatch.location_stale_s" => d.location_stale_s = n as i64,
                "dispatch.max_candidates" => d.max_candidates = n as i64,
                "dispatch.fuzz_min_m" => d.fuzz_min_m = n,
                "dispatch.fuzz_max_m" => d.fuzz_max_m = n,
                "booking.quote_ttl_s" => d.quote_ttl_s = n as i64,
                "booking.schedule_min_s" => d.schedule_min_s = n as i64,
                "booking.schedule_max_s" => d.schedule_max_s = n as i64,
                "booking.schedule_lead_s" => d.schedule_lead_s = n as i64,
                "safety.lockout_hold_s" => d.lockout.hold_s = n as i64,
                "safety.lockout_repeat" => d.lockout.repeat = n as i64,
                "safety.lockout_window_days" => d.lockout.window_days = n as i64,
                _ => {}
            }
        }
        Ok(d)
    }
}
