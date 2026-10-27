//! Outgoing SMS: OTP codes now; guest PINs and emergency alerts in later phases.

use std::{collections::VecDeque, sync::Mutex};

use fuber_common::{telemetry::mask_phone, validate::PhoneE164};

/// SMS bodies are capped so a user-supplied value can never turn one message into many.
pub const MAX_BODY_CHARS: usize = 320;

#[derive(Debug, thiserror::Error)]
pub enum SmsError {
    #[error("SMS provider unavailable: {0}")]
    Unavailable(String),
}

pub enum SmsGateway {
    Fake(FakeSms),
}

impl SmsGateway {
    pub async fn send(&self, to: &PhoneE164, body: &str) -> Result<(), SmsError> {
        let body: String = body.chars().take(MAX_BODY_CHARS).collect();
        match self {
            SmsGateway::Fake(f) => {
                f.record(to, body);
                Ok(())
            }
        }
    }

    /// Messages the fake gateway has "sent", newest last. Empty for real providers.
    pub fn outbox(&self) -> Vec<SentSms> {
        match self {
            SmsGateway::Fake(f) => f.outbox.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentSms {
    pub to: String,
    pub body: String,
}

/// Keeps the last 100 messages in memory. When `print_bodies` is set (local development
/// only, never staging or production) it also logs each message in full so a developer can
/// read the OTP; otherwise only the masked number is logged.
pub struct FakeSms {
    outbox: Mutex<VecDeque<SentSms>>,
    print_bodies: bool,
}

const OUTBOX_LEN: usize = 100;

impl FakeSms {
    pub fn new(print_bodies: bool) -> Self {
        FakeSms { outbox: Mutex::new(VecDeque::new()), print_bodies }
    }

    fn record(&self, to: &PhoneE164, body: String) {
        if self.print_bodies {
            tracing::warn!(target: "fuber::dev_sms", to = %mask_phone(to.as_str()), %body, "fake SMS (local development only)");
        } else {
            tracing::info!(to = %mask_phone(to.as_str()), "fake SMS sent");
        }
        let mut q = self.outbox.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() == OUTBOX_LEN {
            q.pop_front();
        }
        q.push_back(SentSms { to: to.as_str().to_owned(), body });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_records_and_caps_length() {
        let g = SmsGateway::Fake(FakeSms::new(false));
        let to = PhoneE164::parse("+264811234567").unwrap();
        g.send(&to, &"x".repeat(1000)).await.unwrap();
        let out = g.outbox();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to, "+264811234567");
        assert_eq!(out[0].body.chars().count(), MAX_BODY_CHARS);
    }
}
