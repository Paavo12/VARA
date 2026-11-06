//! SOS from either app (PRD S-1, App Flow §6.1): during a ride, or for a driver online
//! between trips.
//!
//! An SOS opens an `sos` incident for the safety desk and texts her emergency contacts a live
//! trip link. The texts leave from the API as soon as the incident is saved, not through the
//! job queue, so they go out well within the 5 s target; any that fail are queued for the
//! worker to retry. The endpoint is never rate-limited (TRD §5). Pressing again within two
//! minutes returns the same incident and texts nobody twice.

use axum::{Json, extract::State};
use fuber_db::{
    jobs,
    rides::{DRIVER_ACTIVE, RIDER_ACTIVE},
    safety::{self, SosText},
};
use fuber_domain::safety::{POLICE_PHONE, SOS_REPEAT_S};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
};

/// Deliberately lenient: an SOS is never refused over its details, so unknown fields are
/// ignored and the location may be anywhere (she may have been driven out of Windhoek).
#[derive(Debug, Deserialize, ToSchema)]
pub struct SosRequest {
    /// Her ride; when absent, her current one (if any).
    pub ride_id: Option<Uuid>,
    /// Where the phone is. When unknown or invalid, a driver's last reported position is used.
    pub location: Option<Point>,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

impl Point {
    fn valid(self) -> Option<Point> {
        ((-90.0..=90.0).contains(&self.lat) && (-180.0..=180.0).contains(&self.lon)).then_some(self)
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SosAlert {
    pub incident_id: Uuid,
    pub ride_id: Option<Uuid>,
    /// Emergency contacts being texted (0 if she has none).
    pub contacts_alerted: usize,
    /// Offered as a one-tap call: Namibian Police.
    #[schema(example = "10111")]
    pub police_phone: &'static str,
    pub message: String,
}

#[utoipa::path(
    post, path = "/v1/safety/sos", tag = "safety", security(("bearer" = [])),
    request_body = SosRequest,
    responses(
        (status = 200, description = "The safety desk has the incident and her contacts are being texted (a repeat within 2 minutes returns the same incident)", body = SosAlert),
        (status = 404, description = "`ride_id` is not her ride", body = ErrorBody),
    )
)]
pub async fn sos(State(s): State<AppState>, user: AuthUser, ValidJson(req): ValidJson<SosRequest>) -> Result<Json<SosAlert>, ApiError> {
    let ride_id = her_ride(&s, &user, req.ride_id).await?;
    let location = req.location.and_then(Point::valid);
    let mut tx = s.db.begin().await?;
    // One SOS at a time per person, so a double tap can't open two incidents.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('sos:' || $1::text, 0))").bind(user.user_id).execute(&mut *tx).await?;
    let repeat: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM safety_incidents
         WHERE raised_by = $1 AND kind = 'sos' AND status <> 'closed' AND ride_id IS NOT DISTINCT FROM $2
           AND created_at > now() - make_interval(secs => $3)
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user.user_id)
    .bind(ride_id)
    .bind(SOS_REPEAT_S as f64)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(id) = repeat {
        if let Some(at) = location {
            sqlx::query("UPDATE safety_incidents SET location = ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography WHERE id = $3")
                .bind(at.lat)
                .bind(at.lon)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        let contacts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM emergency_contacts WHERE user_id = $1").bind(user.user_id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        tracing::warn!(security_event = "sos_repeat", incident_id = %id, user_id = %user.user_id, "SOS pressed again");
        return Ok(Json(alert(id, ride_id, contacts as usize, "Your alert is already with VARA's safety team. They will call you.")));
    }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO safety_incidents (ride_id, raised_by, kind, location)
         VALUES ($3, $4, 'sos', CASE WHEN $1::float8 IS NULL THEN (SELECT location FROM driver_presence WHERE driver_id = $4)
                                     ELSE ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography END)
         RETURNING id",
    )
    .bind(location.map(|a| a.lat))
    .bind(location.map(|a| a.lon))
    .bind(ride_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    let texts = safety::sos_texts(&mut tx, &s.keys.sealer, &s.keys.hasher, &s.config.trip_link_base, id, None).await;
    // A contact that can't be read must not stop the incident reaching the desk.
    let texts = texts.unwrap_or_else(|e| {
        tracing::error!(error = %e, incident_id = %id, "SOS texts could not be prepared");
        vec![]
    });
    tx.commit().await?;
    tracing::warn!(security_event = "sos", incident_id = %id, user_id = %user.user_id, ride_id = ?ride_id, contacts = texts.len(), "SOS raised");

    let n = texts.len();
    let state = s.clone();
    tokio::spawn(async move { send(&state, id, texts).await });
    let message = match n {
        0 => "VARA's safety team has your alert and will call you. Add emergency contacts so we can text them next time.".to_owned(),
        1 => "VARA's safety team has your alert and will call you. We're texting your emergency contact.".to_owned(),
        n => format!("VARA's safety team has your alert and will call you. We're texting your {n} emergency contacts."),
    };
    Ok(Json(alert(id, ride_id, n, &message)))
}

fn alert(incident_id: Uuid, ride_id: Option<Uuid>, contacts_alerted: usize, message: &str) -> SosAlert {
    SosAlert { incident_id, ride_id, contacts_alerted, police_phone: POLICE_PHONE, message: message.into() }
}

/// The ride she named (404 unless it is hers), or her current one. A blocked trip counts:
/// that is exactly when she may need help.
async fn her_ride(s: &AppState, user: &AuthUser, named: Option<Uuid>) -> Result<Option<Uuid>, ApiError> {
    let column = match user.app {
        App::Rider => "rider_id",
        App::Driver => "driver_id",
    };
    if let Some(id) = named {
        let mine: Option<bool> = sqlx::query_scalar(&format!("SELECT {column} IS NOT DISTINCT FROM $2 FROM rides WHERE id = $1"))
            .bind(id)
            .bind(user.user_id)
            .fetch_optional(&s.db)
            .await?;
        return match mine {
            Some(true) => Ok(Some(id)),
            _ => Err(ApiError::not_found()),
        };
    }
    let active: Vec<&str> = match user.app {
        App::Rider => RIDER_ACTIVE.to_vec(),
        App::Driver => DRIVER_ACTIVE.to_vec(),
    };
    let active: Vec<&str> = active.into_iter().chain(["BLOCKED"]).collect();
    Ok(sqlx::query_scalar(&format!(
        "SELECT id FROM rides WHERE {column} = $1 AND state::text = ANY($2) ORDER BY requested_at DESC LIMIT 1"
    ))
    .bind(user.user_id)
    .bind(&active[..])
    .fetch_optional(&s.db)
    .await?)
}

/// Sends the texts; those that fail are queued for the worker to retry.
async fn send(s: &AppState, incident_id: Uuid, texts: Vec<SosText>) {
    let mut failed = vec![];
    for t in texts {
        if let Err(e) = s.sms.send(&t.to, &t.body).await {
            tracing::error!(error = %e, %incident_id, position = t.position, "SOS text failed; queued for retry");
            failed.push(t.position);
        }
    }
    if failed.is_empty() {
        return;
    }
    let payload = json!({ "incident_id": incident_id, "positions": failed });
    if let Err(e) = jobs::enqueue(&s.db, jobs::kind::SOS_SMS, payload, None, Some(&format!("sos:{incident_id}"))).await {
        tracing::error!(error = %e, %incident_id, "SOS text retry could not be queued");
    }
}
