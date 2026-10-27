//! Request extractors that enforce TRD §7.2.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    Json,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::{Extensions, HeaderMap, StatusCode, request::Parts},
};
use serde::de::DeserializeOwned;

use crate::{AppState, error::ApiError};

/// JSON body extractor that answers with our error shape. Combine with request structs marked
/// `#[serde(deny_unknown_fields)]` and built from `fuber_common::validate` types.
pub struct ValidJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(ValidJson(v)),
            Err(JsonRejection::JsonDataError(e)) => Err(ApiError::invalid(data_error_message(&e.body_text()))),
            Err(JsonRejection::JsonSyntaxError(_)) => {
                Err(ApiError::new(StatusCode::BAD_REQUEST, "malformed_json", "Request body is not valid JSON"))
            }
            Err(JsonRejection::MissingJsonContentType(_)) => {
                Err(ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "Content-Type must be application/json"))
            }
            Err(JsonRejection::BytesRejection(e)) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "Request body is too large"))
            }
            Err(e) => Err(ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.body_text())),
        }
    }
}

/// serde's message names the field and the validation failure; drop axum's prefix and cap
/// the length so huge inputs are never echoed back.
fn data_error_message(raw: &str) -> String {
    let msg = raw.strip_prefix("Failed to deserialize the JSON body into the target type: ").unwrap_or(raw);
    msg.chars().take(200).collect()
}

/// The client's IP address. `X-Forwarded-For` is trusted only for the configured number of
/// proxy hops, counted from the right (entries further left can be forged by the client).
pub fn client_ip(req: &Request, trusted_proxy_hops: usize) -> IpAddr {
    client_ip_from(req.headers(), req.extensions(), trusted_proxy_hops)
}

fn client_ip_from(headers: &HeaderMap, extensions: &Extensions, trusted_proxy_hops: usize) -> IpAddr {
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if trusted_proxy_hops > 0 {
        let forwarded: Vec<&str> =
            headers.get_all("x-forwarded-for").iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).map(str::trim).collect();
        if let Some(ip) = forwarded.len().checked_sub(trusted_proxy_hops).and_then(|i| forwarded[i].parse().ok()) {
            return ip;
        }
    }
    // Without connection info every request shares one key: limits stay enforced, if coarse.
    peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// The caller's IP, for rate-limit keys, OTP records, consents and the audit log.
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        Ok(ClientIp(client_ip_from(&parts.headers, &parts.extensions, state.config.trusted_proxy_hops)))
    }
}

/// The optional `X-App-Version` header (e.g. `1.4.2+81`), recorded with sessions and consents.
/// Anything that is not a short version string is ignored rather than stored.
pub struct AppVersion(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for AppVersion {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        let v = parts
            .headers
            .get("x-app-version")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty() && v.len() <= 40 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-_".contains(&b)));
        Ok(AppVersion(v.map(str::to_owned)))
    }
}
