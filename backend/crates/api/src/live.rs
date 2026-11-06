//! Real-time updates (TRD §5).
//!
//! - **Socket**: `GET /v1/ws`, one per signed-in app. The first message must be
//!   `{"type":"auth","data":{"token":"<access token>"}}` within 10 s (tokens never go in URLs,
//!   which get logged). Server → client: `ride.updated`, `offer.created`, `offer.expired`,
//!   `driver.location`, `session.revoked`, `pong`. Client → server: `location.update` (driver),
//!   `ping`. Ride actions stay on HTTPS.
//! - **Hub**: every change to a ride or an offer is announced by Postgres (`pg_notify` from the
//!   `ride_events` / `ride_offers` triggers, on commit). Each API instance listens and, for the
//!   sockets it holds, loads the recipient's own view (`views`) and sends it. So a driver's
//!   socket gets exactly what the driver endpoints would return — never the destination early.
//!
//! Apps refetch `GET /v1/rides/active` (rider) or `GET /v1/driver/status` (driver) after every
//! reconnect: a socket is a hint channel, never the only copy of state.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{
        State,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{self, App, AuthUser},
    error::ApiError,
    extract::ClientIp,
    routes::driving::{LocationUpdate, record_location},
    views,
};

pub const CHANNEL: &str = "fuber_events";
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_secs(20);
/// How long a closing socket keeps reading what the client already sent.
const CLOSE_DRAIN: Duration = Duration::from_secs(2);
/// Per connection: 2 messages a second, bursts of 10 (TRD §7.1). Beyond that the socket closes.
const MSG_RATE: f64 = 2.0;
const MSG_BURST: f64 = 10.0;
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
/// Messages queued for one slow socket before it is dropped (the app refetches on reconnect).
const QUEUE: usize = 64;

type Conns = HashMap<(Uuid, App), Vec<(u64, mpsc::Sender<String>)>>;

/// The sockets this instance holds, by person and app.
#[derive(Default)]
pub struct Hub {
    conns: Mutex<Conns>,
    next: Mutex<u64>,
}

impl Hub {
    fn register(&self, user: Uuid, app: App) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let id = {
            let mut n = self.next.lock().expect("hub lock");
            *n += 1;
            *n
        };
        self.conns.lock().expect("hub lock").entry((user, app)).or_default().push((id, tx));
        (id, rx)
    }

    fn unregister(&self, user: Uuid, app: App, id: u64) {
        let mut conns = self.conns.lock().expect("hub lock");
        if let Some(v) = conns.get_mut(&(user, app)) {
            v.retain(|(i, _)| *i != id);
            if v.is_empty() {
                conns.remove(&(user, app));
            }
        }
    }

    fn connected(&self, user: Uuid, app: App) -> bool {
        self.conns.lock().expect("hub lock").contains_key(&(user, app))
    }

    /// Sends to every socket of this person in this app. A full queue drops that socket's
    /// message (the app catches up on reconnect); a closed one is removed.
    pub fn send(&self, user: Uuid, app: App, kind: &str, data: Value) {
        let text = json!({ "type": kind, "id": Uuid::new_v4(), "data": data }).to_string();
        let mut conns = self.conns.lock().expect("hub lock");
        if let Some(v) = conns.get_mut(&(user, app)) {
            v.retain(|(_, tx)| !matches!(tx.try_send(text.clone()), Err(mpsc::error::TrySendError::Closed(_))));
        }
    }

    pub fn connections(&self) -> usize {
        self.conns.lock().expect("hub lock").values().map(Vec::len).sum()
    }
}

#[derive(Debug, Deserialize)]
struct Event {
    kind: String,
    ride_id: Option<Uuid>,
    rider_id: Option<Uuid>,
    driver_id: Option<Uuid>,
    previous_driver_id: Option<Uuid>,
    offer_id: Option<Uuid>,
    response: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    heading: Option<i32>,
}

/// Listens for change notifications and pushes them to this instance's sockets. Runs for the
/// life of the process; reconnects to Postgres with back-off.
pub async fn listen(db: PgPool, hub: Arc<Hub>) {
    let mut backoff = Duration::from_millis(500);
    loop {
        match PgListener::connect_with(&db).await {
            Ok(mut l) => match l.listen(CHANNEL).await {
                Ok(()) => {
                    backoff = Duration::from_millis(500);
                    tracing::info!("listening for ride events");
                    loop {
                        match l.recv().await {
                            Ok(n) => match serde_json::from_str::<Event>(n.payload()) {
                                Ok(e) => deliver(&db, &hub, e).await,
                                Err(err) => tracing::warn!(error = %err, "unreadable ride event"),
                            },
                            Err(e) => {
                                tracing::warn!(error = %e, "ride event listener lost its connection");
                                break;
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "cannot LISTEN for ride events"),
            },
            Err(e) => tracing::warn!(error = %e, "ride event listener cannot connect"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(15));
    }
}

async fn deliver(db: &PgPool, hub: &Hub, e: Event) {
    if let Err(err) = try_deliver(db, hub, e).await {
        tracing::warn!(error = %err, "could not push ride event");
    }
}

async fn try_deliver(db: &PgPool, hub: &Hub, e: Event) -> Result<(), sqlx::Error> {
    match e.kind.as_str() {
        "ride" => {
            let Some(ride_id) = e.ride_id else { return Ok(()) };
            if let Some(rider) = e.rider_id.filter(|r| hub.connected(*r, App::Rider))
                && let Some(view) = views::rider_ride(db, rider, ride_id).await?
            {
                hub.send(rider, App::Rider, "ride.updated", json!(view));
            }
            if let Some(driver) = e.driver_id.filter(|d| hub.connected(*d, App::Driver))
                && let Some(view) = views::driver_ride(db, driver, ride_id).await?
            {
                hub.send(driver, App::Driver, "ride.updated", json!(view));
            }
            // A driver who just lost the ride hears only that it is gone, nothing more about it.
            if let Some(prev) = e.previous_driver_id.filter(|p| Some(*p) != e.driver_id) {
                hub.send(prev, App::Driver, "ride.removed", json!({ "ride_id": ride_id }));
            }
        }
        "offer" => {
            let (Some(driver), Some(offer_id)) = (e.driver_id, e.offer_id) else { return Ok(()) };
            if !hub.connected(driver, App::Driver) {
                return Ok(());
            }
            match e.response.as_deref() {
                Some("pending") => {
                    if let Some(card) = views::pending_offer(db, driver).await?.filter(|c| c.offer_id == offer_id) {
                        hub.send(driver, App::Driver, "offer.created", json!(card));
                    }
                }
                Some(response) => hub.send(driver, App::Driver, "offer.expired", json!({ "offer_id": offer_id, "response": response })),
                None => {}
            }
        }
        "location" => {
            if let (Some(rider), Some(ride_id), Some(lat), Some(lon)) = (e.rider_id, e.ride_id, e.lat, e.lon) {
                hub.send(rider, App::Rider, "driver.location", json!({ "ride_id": ride_id, "lat": lat, "lon": lon, "heading": e.heading }));
            }
        }
        other => tracing::debug!(kind = other, "ignored ride event"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthData {
    token: String,
}

/// `GET /v1/ws` (upgrade). Rate limited per IP before the upgrade.
pub async fn upgrade(State(s): State<AppState>, ClientIp(ip): ClientIp, ws: WebSocketUpgrade) -> Result<Response, ApiError> {
    s.limiter.check("ws_connect", &ip.to_string(), s.limits.ws_connect).await?;
    Ok(ws.max_message_size(MAX_MESSAGE_BYTES).max_frame_size(MAX_MESSAGE_BYTES).on_upgrade(move |socket| serve(s, socket)))
}

fn text(kind: &str, data: Value) -> Message {
    Message::Text(Utf8Bytes::from(json!({ "type": kind, "id": Uuid::new_v4(), "data": data }).to_string()))
}

fn error(code: &str, message: &str) -> Message {
    text("error", json!({ "code": code, "message": message }))
}

async fn serve(s: AppState, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // 1. Authenticate with the first message.
    let first = tokio::time::timeout(AUTH_TIMEOUT, stream.next()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<ClientMessage>(&t)
            .ok()
            .filter(|m| m.kind == "auth")
            .and_then(|m| serde_json::from_value::<AuthData>(m.data).ok())
            .map(|a| a.token),
        _ => None,
    };
    let user = match token {
        Some(t) => auth::authenticate(&s, &t).await.ok(),
        None => None,
    };
    let Some(user) = user else {
        let _ = sink.send(error("unauthorized", "Please sign in again.")).await;
        let _ = sink.close().await;
        return;
    };
    let (conn_id, mut outbox) = s.hub.register(user.user_id, user.app);
    let _ = sink.send(text("ready", json!({ "app": user.app }))).await;

    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.tick().await;
    let mut session_check = tokio::time::interval(Duration::from_secs(60));
    session_check.tick().await;
    let mut tokens = MSG_BURST;
    let mut refilled = Instant::now();
    let mut awaiting_pong = false;

    loop {
        tokio::select! {
            msg = outbox.recv() => match msg {
                Some(m) => if sink.send(Message::Text(Utf8Bytes::from(m))).await.is_err() { break },
                None => break,
            },
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                match msg {
                    Message::Text(t) => {
                        let now = Instant::now();
                        tokens = (tokens + now.duration_since(refilled).as_secs_f64() * MSG_RATE).min(MSG_BURST);
                        refilled = now;
                        if tokens < 1.0 {
                            tracing::warn!(security_event = "ws_flood", user_id = %user.user_id, "WebSocket message rate exceeded; closing");
                            let _ = sink.send(error("rate_limited", "Too many messages.")).await;
                            break;
                        }
                        tokens -= 1.0;
                        if let Some(reply) = handle(&s, &user, &t).await
                            && sink.send(reply).await.is_err()
                        {
                            break;
                        }
                    }
                    Message::Pong(_) => awaiting_pong = false,
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                if awaiting_pong { break }
                awaiting_pong = true;
                if sink.send(Message::Ping(Default::default())).await.is_err() { break }
            }
            _ = session_check.tick() => {
                // Sign-out, suspension or a sign-in on another phone ends the socket too.
                if !auth::session_live(&s, &user).await.unwrap_or(true) {
                    let _ = sink.send(text("session.revoked", json!({}))).await;
                    break;
                }
            }
        }
    }
    s.hub.unregister(user.user_id, user.app, conn_id);
    let _ = sink.close().await;
    // Closing with unread messages makes TCP reset the connection, which can destroy our last
    // message (`rate_limited`) before the client reads it.
    let _ = tokio::time::timeout(CLOSE_DRAIN, async { while let Some(Ok(_)) = stream.next().await {} }).await;
}

async fn handle(s: &AppState, user: &AuthUser, raw: &str) -> Option<Message> {
    let Ok(m) = serde_json::from_str::<ClientMessage>(raw) else {
        return Some(error("bad_message", "Messages are JSON: {\"type\": ..., \"data\": ...}"));
    };
    match m.kind.as_str() {
        "ping" => Some(text("pong", json!({ "id": m.id }))),
        "location.update" if user.app == App::Driver => {
            let update = match serde_json::from_value::<LocationUpdate>(m.data) {
                Ok(u) => u,
                Err(e) => return Some(error("invalid_request", &e.to_string().chars().take(200).collect::<String>())),
            };
            let at = match update.position() {
                Ok(p) => p,
                Err(e) => return Some(error(e.code, &e.message)),
            };
            match record_location(s, user, at).await {
                Ok(true) => None,
                Ok(false) => Some(error("offline", "You're offline. Go online to receive requests.")),
                Err(e) => Some(error(e.code, &e.message)),
            }
        }
        _ => Some(error("bad_message", "Unknown message type")),
    }
}
