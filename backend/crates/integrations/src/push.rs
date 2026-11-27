//! Push notifications (App Flow §10) through Firebase Cloud Messaging's HTTP v1 API, which
//! reaches Android directly and iPhones through APNs. The service authenticates as itself
//! (metadata server); the apps hold only their public Firebase config.
//!
//! Texts are written for a lock screen: no names, addresses or plates, since anyone near
//! the phone can read them. The app shows the details once she opens it.

use std::{collections::VecDeque, sync::Mutex, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::google::MetadataToken;

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    /// The device token is no longer valid (app uninstalled, token rotated): forget it.
    #[error("device token is no longer registered")]
    Unregistered,
    #[error("push provider unavailable: {0}")]
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushMessage {
    pub title: String,
    pub body: String,
    /// Read by the app to open the right screen (`kind`, `ride_id`).
    pub data: Vec<(&'static str, String)>,
    /// Safety and pickup messages: delivered at once, shown on the safety channel.
    pub urgent: bool,
    /// Dropped by the provider if it cannot be delivered in time (a stale "driver arrived"
    /// is worse than none).
    pub ttl: Duration,
}

pub enum PushGateway {
    Fake(FakePush),
    Fcm(Box<Fcm>),
}

impl PushGateway {
    pub async fn send(&self, token: &str, msg: &PushMessage) -> Result<(), PushError> {
        match self {
            PushGateway::Fake(f) => f.record(token, msg),
            PushGateway::Fcm(f) => f.send(token, msg).await,
        }
    }

    /// Messages the fake gateway has "sent", newest last. Empty for real providers.
    pub fn outbox(&self) -> Vec<SentPush> {
        match self {
            PushGateway::Fake(f) => f.outbox.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect(),
            PushGateway::Fcm(_) => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentPush {
    pub token: String,
    pub message: PushMessage,
}

/// Keeps the last 100 messages in memory and logs only that one was sent. A token starting
/// with `unregistered-` is refused as no longer registered, so local runs and tests can
/// exercise forgetting a dead token.
#[derive(Default)]
pub struct FakePush {
    outbox: Mutex<VecDeque<SentPush>>,
}

const OUTBOX_LEN: usize = 100;

impl FakePush {
    fn record(&self, token: &str, msg: &PushMessage) -> Result<(), PushError> {
        if token.starts_with("unregistered-") {
            return Err(PushError::Unregistered);
        }
        tracing::info!(kind = msg.data.iter().find(|(k, _)| *k == "kind").map(|(_, v)| v.as_str()), "fake push sent");
        let mut q = self.outbox.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() == OUTBOX_LEN {
            q.pop_front();
        }
        q.push_back(SentPush { token: token.to_owned(), message: msg.clone() });
        Ok(())
    }
}

/// Where Google's services are. Tests point these at a local fake.
#[derive(Debug, Clone)]
pub struct FcmEndpoints {
    pub fcm: String,
    pub metadata: String,
}

impl Default for FcmEndpoints {
    fn default() -> Self {
        FcmEndpoints { fcm: "https://fcm.googleapis.com".into(), metadata: "http://metadata.google.internal".into() }
    }
}

pub struct Fcm {
    project: String,
    at: FcmEndpoints,
    http: reqwest::Client,
    token: MetadataToken,
}

impl Fcm {
    pub fn new(project: &str, at: FcmEndpoints) -> Self {
        let http = crate::http_client(TIMEOUT);
        let token = MetadataToken::new(&at.metadata, http.clone());
        Fcm { project: project.to_owned(), at, http, token }
    }

    async fn send(&self, device: &str, msg: &PushMessage) -> Result<(), PushError> {
        let auth = self.token.get().await.map_err(|e| PushError::Unavailable(format!("token: {e}")))?;
        let url = format!("{}/v1/projects/{}/messages:send", self.at.fcm, self.project);
        let res = self
            .http
            .post(url)
            .bearer_auth(auth)
            .json(&body(device, msg))
            .send()
            .await
            .map_err(|e| PushError::Unavailable(e.to_string()))?;
        let status = res.status();
        if status.is_success() {
            return Ok(());
        }
        let err: Value = res.json().await.unwrap_or(Value::Null);
        Err(classify(status.as_u16(), &err))
    }
}

/// The FCM v1 `messages:send` body.
fn body(device: &str, msg: &PushMessage) -> Value {
    let data: serde_json::Map<String, Value> = msg.data.iter().map(|(k, v)| ((*k).to_owned(), Value::String(v.clone()))).collect();
    json!({
        "message": {
            "token": device,
            "notification": { "title": msg.title, "body": msg.body },
            "data": data,
            "android": {
                "priority": if msg.urgent { "HIGH" } else { "NORMAL" },
                "ttl": format!("{}s", msg.ttl.as_secs()),
                "notification": { "channel_id": if msg.urgent { "safety" } else { "rides" } },
            },
            "apns": {
                "headers": {
                    "apns-priority": if msg.urgent { "10" } else { "5" },
                    "apns-expiration": (time::OffsetDateTime::now_utc().unix_timestamp() + msg.ttl.as_secs() as i64).to_string(),
                },
                "payload": { "aps": {
                    "sound": "default",
                    "interruption-level": if msg.urgent { "time-sensitive" } else { "active" },
                } },
            },
        }
    })
}

/// Which failures mean the token is dead. Anything else (quota, outage, our own bad request)
/// leaves the token alone: wiping every rider's token over a bug in one message would be worse.
fn classify(status: u16, err: &Value) -> PushError {
    #[derive(Deserialize)]
    struct Detail {
        #[serde(rename = "errorCode")]
        error_code: Option<String>,
    }
    let codes: Vec<String> = err["error"]["details"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| serde_json::from_value::<Detail>(d.clone()).ok()?.error_code)
        .collect();
    if status == 404 || codes.iter().any(|c| c == "UNREGISTERED" || c == "SENDER_ID_MISMATCH") {
        PushError::Unregistered
    } else {
        let message = err["error"]["message"].as_str().unwrap_or("");
        PushError::Unavailable(format!("FCM HTTP {status} {}: {message}", codes.join(",")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };

    use super::*;

    fn message(urgent: bool) -> PushMessage {
        PushMessage {
            title: "Are you okay?".into(),
            body: "Open VARA to answer.".into(),
            data: vec![("kind", "ridecheck".into()), ("ride_id", "r-1".into())],
            urgent,
            ttl: Duration::from_secs(120),
        }
    }

    #[derive(Default)]
    struct Fake {
        tokens_issued: StdMutex<u32>,
        sent: StdMutex<Vec<(String, Value)>>,
    }

    async fn fake() -> (Fcm, Arc<Fake>) {
        let state = Arc::new(Fake::default());
        let app = Router::new()
            .route(
                "/computeMetadata/v1/instance/service-accounts/default/token",
                get(|State(f): State<Arc<Fake>>, h: HeaderMap| async move {
                    assert_eq!(h["metadata-flavor"], "Google");
                    *f.tokens_issued.lock().unwrap() += 1;
                    Json(json!({ "access_token": "tok-1", "expires_in": 3599, "token_type": "Bearer" }))
                }),
            )
            .route(
                "/v1/projects/{project}/{action}",
                post(|State(f): State<Arc<Fake>>, Path((project, action)): Path<(String, String)>, h: HeaderMap, Json(v): Json<Value>| async move {
                    assert_eq!((project.as_str(), action.as_str()), ("vara-staging", "messages:send"));
                    assert_eq!(h["authorization"], "Bearer tok-1");
                    let device = v["message"]["token"].as_str().unwrap().to_owned();
                    if device == "gone" {
                        let err = json!({ "error": { "code": 404, "message": "Requested entity was not found.", "status": "NOT_FOUND",
                            "details": [{ "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "UNREGISTERED" }] } });
                        return (StatusCode::NOT_FOUND, Json(err));
                    }
                    if device == "busy" {
                        let err = json!({ "error": { "code": 429, "message": "Quota exceeded.", "status": "RESOURCE_EXHAUSTED",
                            "details": [{ "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "QUOTA_EXCEEDED" }] } });
                        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
                    }
                    f.sent.lock().unwrap().push((device, v));
                    (StatusCode::OK, Json(json!({ "name": "projects/vara-staging/messages/1" })))
                }),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Fcm::new("vara-staging", FcmEndpoints { fcm: base.clone(), metadata: base }), state)
    }

    #[tokio::test]
    async fn sends_with_one_token_and_marks_urgent_messages() {
        let (fcm, fake) = fake().await;
        fcm.send("device-a", &message(true)).await.unwrap();
        fcm.send("device-b", &message(false)).await.unwrap();
        assert_eq!(*fake.tokens_issued.lock().unwrap(), 1, "the access token is reused");

        let sent = fake.sent.lock().unwrap();
        let (device, urgent) = &sent[0];
        assert_eq!(device, "device-a");
        let m = &urgent["message"];
        assert_eq!(m["notification"], json!({ "title": "Are you okay?", "body": "Open VARA to answer." }));
        assert_eq!(m["data"], json!({ "kind": "ridecheck", "ride_id": "r-1" }));
        assert_eq!((m["android"]["priority"].as_str(), m["android"]["ttl"].as_str()), (Some("HIGH"), Some("120s")));
        assert_eq!(m["android"]["notification"]["channel_id"], "safety");
        assert_eq!(m["apns"]["headers"]["apns-priority"], "10");
        assert_eq!(m["apns"]["payload"]["aps"]["interruption-level"], "time-sensitive");

        let normal = &sent[1].1["message"];
        assert_eq!(normal["android"]["priority"], "NORMAL");
        assert_eq!(normal["apns"]["headers"]["apns-priority"], "5");
    }

    #[tokio::test]
    async fn only_dead_tokens_are_reported_unregistered() {
        let (fcm, _) = fake().await;
        assert!(matches!(fcm.send("gone", &message(false)).await, Err(PushError::Unregistered)));
        match fcm.send("busy", &message(false)).await {
            Err(PushError::Unavailable(e)) => assert!(e.contains("429") && e.contains("QUOTA_EXCEEDED"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn fake_records_and_refuses_unregistered_tokens() {
        let g = PushGateway::Fake(FakePush::default());
        g.send("device-a", &message(false)).await.unwrap();
        assert!(matches!(g.send("unregistered-1", &message(false)).await, Err(PushError::Unregistered)));
        assert_eq!(g.outbox().len(), 1);
        assert_eq!(g.outbox()[0].token, "device-a");
    }
}
