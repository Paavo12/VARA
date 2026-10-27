//! Append-only audit log (TRD §7.6): every ops action and every view of an identity image.

use std::net::IpAddr;

use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

pub struct Entry<'a> {
    pub staff_id: Option<Uuid>,
    /// The person the action concerns, if any.
    pub user_id: Option<Uuid>,
    /// Dotted verb, e.g. `identity.view`, `driver_document.approve`, `staff.login_failed`.
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: String,
    pub detail: Option<Value>,
    pub ip: Option<IpAddr>,
}

pub async fn record<'e>(db: impl PgExecutor<'e>, e: Entry<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log (staff_id, user_id, action, target_type, target_id, detail, ip) VALUES ($1, $2, $3, $4, $5, $6, $7::inet)",
    )
    .bind(e.staff_id)
    .bind(e.user_id)
    .bind(e.action)
    .bind(e.target_type)
    .bind(e.target_id)
    .bind(e.detail)
    .bind(e.ip.map(|ip| ip.to_string()))
    .execute(db)
    .await
    .map(|_| ())
}
