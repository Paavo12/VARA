//! Signed-URL file transfer for the local storage backend. Behaves like a pre-signed S3
//! request: the URL alone authorises exactly one operation on one key until it expires.
//! Not part of the OpenAPI document: clients only ever follow URLs the API gave them.

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use fuber_integrations::storage::Op;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{AppState, error::ApiError};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    op: String,
    exp: i64,
    max: u64,
    sig: String,
}

fn denied() -> ApiError {
    ApiError::forbidden("signature_invalid", "This link is invalid or has expired.")
}

pub async fn upload(
    State(s): State<AppState>,
    Path(key): Path<String>,
    Query(q): Query<Signed>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let local = s.store.local().ok_or_else(ApiError::not_found)?;
    let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    if q.op != "put" {
        return Err(denied());
    }
    let grant = local.verify(Op::Put, &key, ct, q.max, q.exp, &q.sig, now).ok_or_else(denied)?;
    if body.is_empty() || body.len() as u64 > grant.max_bytes {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "The file is larger than declared."));
    }
    s.store.write(&grant.key, &body).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn download(State(s): State<AppState>, Path(key): Path<String>, Query(q): Query<Signed>) -> Result<Response, ApiError> {
    let local = s.store.local().ok_or_else(ApiError::not_found)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    if q.op != "get" {
        return Err(denied());
    }
    let grant = local.verify(Op::Get, &key, "", q.max, q.exp, &q.sig, now).ok_or_else(denied)?;
    // Only sanitised files are ever served; quarantined uploads are never readable by URL.
    let mime = if grant.key.ends_with(".jpg") {
        "image/jpeg"
    } else if grant.key.ends_with(".pdf") {
        "application/pdf"
    } else {
        return Err(ApiError::not_found());
    };
    let bytes = s.store.read(&grant.key).await.map_err(|_| ApiError::not_found())?;
    let mut res = bytes.into_response();
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    // PDFs download rather than render in the browser's PDF viewer.
    let disposition = if mime == "application/pdf" { "attachment" } else { "inline" };
    res.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static(disposition));
    Ok(res)
}
