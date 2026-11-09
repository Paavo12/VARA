//! Chat between the rider and the driver on a trip (PRD T-8). Neither sees the other's number.
//!
//! - **Open** from the driver accepting until the trip ends. A trip blocked by wrong PINs is
//!   closed at once: whoever entered them can't message the passenger.
//! - **One conversation per driver**: when a driver cancels and the ride goes to another, the
//!   next driver starts empty, the rider's view starts over, and the first driver gets 404.
//! - **Delivery**: messages are saved, then announced on commit like every ride change; the
//!   hub loads and pushes them (`chat.message`). Apps refetch after every reconnect.
//! - **Retention**: 90 days unless an incident holds the ride (schema §12). The safety desk
//!   reads the chat through the incident (`ops::incidents::messages`), and that is audited.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use fuber_common::validate::PlainText;
use fuber_db::rides::DRIVER_ACTIVE;
use fuber_domain::chat::{self, QuickReply};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
    routes::rides::idempotency_key,
};

/// The newest messages a thread returns; far more than a trip produces.
const THREAD_LIMIT: i64 = 200;

const SELECT_MESSAGE: &str = "SELECT id, ride_id, sender::text AS sender, body AS text, quick_reply, sent_at FROM ride_messages";

#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct ChatMessage {
    pub id: i64,
    pub ride_id: Uuid,
    /// `rider` or `driver`.
    pub sender: String,
    /// Plain text: render it as text, never as HTML.
    pub text: String,
    /// The quick reply's ID, when it was one.
    pub quick_reply: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuickReplyOption {
    pub id: &'static str,
    pub text: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatThread {
    /// Oldest first: the newest 200 of this conversation.
    pub messages: Vec<ChatMessage>,
    /// False once the trip is over or blocked: the conversation stays readable.
    pub can_send: bool,
    /// The buttons to offer this person.
    pub quick_replies: Vec<QuickReplyOption>,
}

/// Exactly one of the two.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMessage {
    #[schema(value_type = Option<String>, example = "I'm by the entrance")]
    pub text: Option<PlainText<500>>,
    /// An ID from the thread's `quick_replies`.
    #[schema(example = "at_entrance")]
    pub quick_reply: Option<String>,
}

/// Loads one message for the hub to push.
pub(crate) async fn message(db: &PgPool, id: i64) -> Result<Option<ChatMessage>, sqlx::Error> {
    sqlx::query_as(&format!("{SELECT_MESSAGE} WHERE id = $1")).bind(id).fetch_optional(db).await
}

/// The message already saved under this key, if any.
async fn resend(db: &PgPool, ride_id: Uuid, sender: &str, key: &str) -> Result<Option<ChatMessage>, sqlx::Error> {
    sqlx::query_as(&format!("{SELECT_MESSAGE} WHERE ride_id = $1 AND sender = $2::party AND client_key = $3"))
        .bind(ride_id)
        .bind(sender)
        .bind(key)
        .fetch_optional(db)
        .await
}

struct Conversation {
    rider_id: Uuid,
    driver_id: Option<Uuid>,
    can_send: bool,
}

/// The ride as this person sees it, or 404 (another person's ride looks like a missing one).
async fn conversation(db: &PgPool, user: &AuthUser, ride_id: Uuid) -> Result<Conversation, ApiError> {
    let row: Option<(Uuid, Option<Uuid>, String)> =
        sqlx::query_as("SELECT rider_id, driver_id, state::text FROM rides WHERE id = $1").bind(ride_id).fetch_optional(db).await?;
    let (rider_id, driver_id, state) = row.ok_or_else(ApiError::not_found)?;
    let mine = match user.app {
        App::Rider => rider_id == user.user_id,
        App::Driver => driver_id == Some(user.user_id),
    };
    if !mine {
        return Err(ApiError::not_found());
    }
    Ok(Conversation { rider_id, driver_id, can_send: driver_id.is_some() && DRIVER_ACTIVE.contains(&state.as_str()) })
}

fn quick_replies(app: App) -> &'static [QuickReply] {
    match app {
        App::Rider => chat::RIDER_QUICK_REPLIES,
        App::Driver => chat::DRIVER_QUICK_REPLIES,
    }
}

#[utoipa::path(
    get, path = "/v1/rides/{id}/messages", tag = "chat", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "This conversation; empty until the first message, and for a rider between drivers", body = ChatThread),
        (status = 404, description = "Not this person's ride", body = ErrorBody),
    )
)]
pub async fn thread(State(s): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> Result<Json<ChatThread>, ApiError> {
    let c = conversation(&s.db, &user, id).await?;
    let messages: Vec<ChatMessage> = sqlx::query_as(&format!(
        "SELECT * FROM ({SELECT_MESSAGE} WHERE ride_id = $1 AND driver_id = $2 ORDER BY id DESC LIMIT $3) newest ORDER BY id"
    ))
    .bind(id)
    .bind(c.driver_id)
    .bind(THREAD_LIMIT)
    .fetch_all(&s.db)
    .await?;
    let quick_replies = quick_replies(user.app).iter().map(|q| QuickReplyOption { id: q.id, text: q.text }).collect();
    Ok(Json(ChatThread { messages, can_send: c.can_send, quick_replies }))
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/messages", tag = "chat", security(("bearer" = [])),
    params(
        ("id" = Uuid, Path),
        ("Idempotency-Key" = String, Header, description = "Random per message; a resend with the same key returns the same message and tells nobody twice"),
    ),
    request_body = SendMessage,
    responses(
        (status = 200, description = "Saved and on its way to the other person", body = ChatMessage),
        (status = 404, description = "Not this person's ride", body = ErrorBody),
        (status = 409, description = "`chat_closed`: the trip is over, blocked or has no driver", body = ErrorBody),
        (status = 422, description = "Not exactly one of `text` and `quick_reply`, empty, over 500 characters, or an unknown quick reply", body = ErrorBody),
        (status = 429, description = "Too many messages", body = ErrorBody),
    )
)]
pub async fn send(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    ValidJson(req): ValidJson<SendMessage>,
) -> Result<Json<ChatMessage>, ApiError> {
    s.limiter.check("chat", &user.user_id.to_string(), s.limits.chat).await?;
    let key = idempotency_key(&headers)?;
    let (text, quick_reply) = match (&req.text, &req.quick_reply) {
        (Some(text), None) => (text.as_str(), None),
        (None, Some(reply)) => {
            let reply = quick_replies(user.app).iter().find(|q| q.id == reply).ok_or_else(|| ApiError::invalid("unknown quick reply"))?;
            (reply.text, Some(reply.id))
        }
        _ => return Err(ApiError::invalid("send either text or a quick reply")),
    };
    let c = conversation(&s.db, &user, id).await?;
    let sender = match user.app {
        App::Rider => "rider",
        App::Driver => "driver",
    };
    // Before the closed check: a resend after the trip ended still gets its answer.
    if let Some(earlier) = resend(&s.db, id, sender, &key).await? {
        return Ok(Json(earlier));
    }
    let (Some(driver_id), true) = (c.driver_id, c.can_send) else {
        return Err(ApiError::conflict("chat_closed", "This trip's chat has closed."));
    };
    let mut tx = s.db.begin().await?;
    let saved: Option<ChatMessage> = sqlx::query_as(
        "INSERT INTO ride_messages (ride_id, driver_id, sender, body, quick_reply, client_key)
         VALUES ($1, $2, $3::party, $4, $5, $6)
         ON CONFLICT ON CONSTRAINT one_message_per_client_key DO NOTHING
         RETURNING id, ride_id, sender::text AS sender, body AS text, quick_reply, sent_at",
    )
    .bind(id)
    .bind(driver_id)
    .bind(sender)
    .bind(text)
    .bind(quick_reply)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(saved) = saved else {
        // The same key arrived twice at once; the other request saved and announced it.
        tx.rollback().await?;
        return resend(&s.db, id, sender, &key).await?.map(Json).ok_or_else(ApiError::not_found);
    };
    let event = json!({ "kind": "chat", "ride_id": id, "rider_id": c.rider_id, "driver_id": driver_id, "message_id": saved.id });
    sqlx::query("SELECT pg_notify('fuber_events', $1)").bind(event.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(saved))
}
