//! Push notices (App Flow §10): the `push.notice` job, and direct sends for the worker's own
//! messages. Every signed-in device of hers in the right app gets the notice; a device whose
//! token is dead is forgotten. "Driver assigned" and "arrived" fall back to SMS when no push
//! gets through (PRD §5.9).

use std::time::Duration;

use fuber_db::{
    push::Notice,
    sqlx::{self},
};
use fuber_integrations::push::{PushError, PushMessage};
use serde_json::Value;
use uuid::Uuid;

use crate::{Ctx, JobError, notify};

/// Ride and safety notices are useless late: they are sent once, never retried.
const RIDE_TTL: Duration = Duration::from_secs(10 * 60);
const RIDECHECK_TTL: Duration = Duration::from_secs(2 * 60);
const ACCOUNT_TTL: Duration = Duration::from_secs(3 * 24 * 3600);

/// Lock-screen text: no names, addresses or plates.
fn message(notice: Notice, ride: Option<Uuid>) -> PushMessage {
    let (title, body, urgent, ttl) = match notice {
        Notice::DriverAssigned => ("Your driver is on the way", "Open VARA to see her car, plate and your PIN.", true, RIDE_TTL),
        Notice::DriverArrived => ("Your driver has arrived", "Check the plate before you get in, then give her your PIN.", true, RIDE_TTL),
        Notice::TripStarted => ("Your trip has started", "SOS and Share trip are on the trip screen.", false, RIDE_TTL),
        Notice::TripCompleted => ("You've arrived", "Thanks for riding with VARA. Rate your trip in the app.", false, RIDE_TTL),
        Notice::ScheduledSearching => ("Finding your driver", "Your scheduled ride is looking for a driver now.", true, RIDE_TTL),
        Notice::RideCheck => ("Are you okay?", "Your ride looks unusual. Open VARA to answer.", true, RIDECHECK_TTL),
        Notice::RiderVerified => ("You're verified", "You can book rides with VARA now.", false, ACCOUNT_TTL),
        Notice::RiderNotVerified => ("Verification needs another look", "Open VARA to see why and try again.", false, ACCOUNT_TTL),
        Notice::DriverApproved => ("You're approved to drive", "Open VARA Driver to go online.", false, ACCOUNT_TTL),
        Notice::DriverNotApproved => ("Your application was reviewed", "Open VARA Driver to see the decision.", false, ACCOUNT_TTL),
    };
    let mut data = vec![("kind", notice.as_str().to_owned())];
    if let Some(ride) = ride {
        data.push(("ride_id", ride.to_string()));
    }
    PushMessage { title: title.into(), body: body.into(), data, urgent, ttl }
}

/// The SMS sent instead when no push reached her.
fn sms_fallback(notice: Notice) -> Option<&'static str> {
    match notice {
        Notice::DriverAssigned => Some("VARA: your driver is on the way. Open the app to see her car, plate and your PIN."),
        Notice::DriverArrived => Some("VARA: your driver has arrived. Check the plate before you get in, then give her your PIN."),
        _ => None,
    }
}

/// Runs a `push.notice` job.
pub async fn notice(ctx: &Ctx, payload: &Value) -> Result<(), JobError> {
    let id = |field: &str| payload.get(field).and_then(Value::as_str).and_then(|v| v.parse::<Uuid>().ok());
    let notice = payload
        .get("notice")
        .and_then(Value::as_str)
        .and_then(Notice::parse)
        .ok_or_else(|| JobError::Other("payload has no known notice".into()))?;
    let ride = id("ride_id");
    let user = match (ride, id("user_id")) {
        (Some(ride), _) => {
            let row: Option<(Uuid, String)> =
                sqlx::query_as("SELECT rider_id, state::text FROM rides WHERE id = $1").bind(ride).fetch_optional(&ctx.db).await?;
            match row {
                Some((_, state)) if state == "CANCELLED" => return Ok(()), // moved on before it went out
                Some((rider, _)) => rider,
                None => return Ok(()),
            }
        }
        (None, Some(user)) => user,
        (None, None) => return Err(JobError::Other("payload has no ride_id or user_id".into())),
    };
    let sent = send(ctx, user, notice.app(), &message(notice, ride)).await?;
    if sent.delivered > 0 {
        return Ok(());
    }
    if let Some(text) = sms_fallback(notice) {
        let phone: Option<String> =
            sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = $1").bind(user).fetch_optional(&ctx.db).await?.flatten();
        notify(ctx, phone, text).await;
        return Ok(());
    }
    // Account notices can wait for the provider to come back; ride notices would be stale.
    if sent.unavailable > 0 && ride.is_none() {
        return Err(JobError::Other("push provider unavailable".into()));
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sent {
    pub delivered: usize,
    pub unavailable: usize,
}

/// Sends `msg` to each of her live sessions in `app` that has a device token.
pub async fn send(ctx: &Ctx, user: Uuid, app: &str, msg: &PushMessage) -> Result<Sent, JobError> {
    let devices: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, push_token FROM sessions
         WHERE user_id = $1 AND app = $2 AND push_token IS NOT NULL AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(user)
    .bind(app)
    .fetch_all(&ctx.db)
    .await?;
    let mut sent = Sent::default();
    for (session, token) in devices {
        match ctx.push.send(&token, msg).await {
            Ok(()) => sent.delivered += 1,
            Err(PushError::Unregistered) => {
                sqlx::query("UPDATE sessions SET push_token = NULL, push_platform = NULL WHERE id = $1 AND push_token = $2")
                    .bind(session)
                    .bind(&token)
                    .execute(&ctx.db)
                    .await?;
            }
            Err(PushError::Unavailable(e)) => {
                tracing::warn!(error = %e, "push failed");
                sent.unavailable += 1;
            }
        }
    }
    Ok(sent)
}

/// A driver's document notice (App Flow §10: push and SMS). Best effort, like the SMS.
pub async fn document(ctx: &Ctx, driver: Uuid, title: &str, body: &str) {
    let msg =
        PushMessage { title: title.into(), body: body.into(), data: vec![("kind", "document".into())], urgent: false, ttl: ACCOUNT_TTL };
    if let Err(e) = send(ctx, driver, "driver", &msg).await {
        tracing::warn!(error = %e, "document push failed");
    }
}
