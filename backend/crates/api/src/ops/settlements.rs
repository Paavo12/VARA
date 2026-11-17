//! Weekly settlement by hand (PRD P-6) until a payout provider exists. The worker makes one
//! statement per driver per week (`fuber_worker::settlement`); an admin records each one as
//! settled with the bank or deposit reference. That books a `payout` entry in her ledger which
//! cancels the statement's amount, and goes in the audit log.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use fuber_common::validate::PlainText;
use fuber_domain::Cents;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Date, OffsetDateTime};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::Staff;
use crate::{
    AppState, audit,
    error::{ApiError, ErrorBody},
    extract::{ClientIp, ValidJson},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct SettlementItem {
    pub id: Uuid,
    pub driver_id: Uuid,
    pub driver_name: Option<String>,
    pub driver_phone: Option<String>,
    /// Monday to Sunday, Windhoek time.
    #[schema(value_type = String, format = Date)]
    pub period_start: String,
    #[schema(value_type = String, format = Date)]
    pub period_end: String,
    /// Positive: VARA pays her. Negative: she pays VARA (service fees on cash trips).
    pub amount: Cents,
    /// `pending`, `sent` (VARA paid her) or `received` (she paid VARA).
    pub status: String,
    pub reference: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub settled_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SettlementQuery {
    /// `pending` (default) or `settled`.
    #[serde(default)]
    pub status: Option<String>,
}

type Row = (Uuid, Uuid, Option<String>, Option<String>, Date, Date, i64, String, Option<String>, Option<OffsetDateTime>);

const SELECT: &str = "SELECT p.id, p.driver_id, u.display_name, u.phone_e164, p.period_start, p.period_end, p.amount_cents,
                             p.status, p.reference, p.settled_at
                      FROM payouts p JOIN users u ON u.id = p.driver_id";

fn item(
    (id, driver_id, driver_name, driver_phone, period_start, period_end, amount, status, reference, settled_at): Row,
) -> SettlementItem {
    SettlementItem {
        id,
        driver_id,
        driver_name,
        driver_phone,
        period_start: period_start.to_string(),
        period_end: period_end.to_string(),
        amount: Cents(amount),
        status,
        reference,
        settled_at,
    }
}

#[utoipa::path(
    get, path = "/v1/ops/settlements", operation_id = "settlement_queue", tag = "ops", security(("staff" = [])),
    params(SettlementQuery),
    responses(
        (status = 200, description = "Weekly statements: pending (oldest first) or the last 200 settled", body = Vec<SettlementItem>),
        (status = 403, description = "Admins only", body = ErrorBody),
    )
)]
pub async fn queue(
    State(s): State<AppState>,
    staff: Staff,
    Query(q): Query<SettlementQuery>,
) -> Result<Json<Vec<SettlementItem>>, ApiError> {
    staff.require_admin()?;
    let sql = match q.status.as_deref() {
        None | Some("pending") => format!("{SELECT} WHERE p.status = 'pending' ORDER BY p.period_start, u.display_name LIMIT 500"),
        Some("settled") => format!("{SELECT} WHERE p.status <> 'pending' ORDER BY p.settled_at DESC LIMIT 200"),
        Some(_) => return Err(ApiError::invalid("status must be pending or settled")),
    };
    let rows: Vec<Row> = sqlx::query_as(&sql).fetch_all(&s.db).await?;
    Ok(Json(rows.into_iter().map(item).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Settle {
    /// The bank transfer or deposit slip reference.
    #[schema(value_type = String, example = "FNB 4471 2026-11-17")]
    pub reference: PlainText<100>,
}

#[utoipa::path(
    post, path = "/v1/ops/settlements/{id}/settle", tag = "ops", security(("staff" = [])),
    params(("id" = Uuid, Path)),
    request_body = Settle,
    responses(
        (status = 200, description = "Recorded as settled; her balance is reduced by the amount", body = SettlementItem),
        (status = 403, description = "Admins only", body = ErrorBody),
        (status = 404, description = "No such statement", body = ErrorBody),
        (status = 409, description = "Already settled (`already_settled`)", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
    )
)]
pub async fn settle(
    State(s): State<AppState>,
    staff: Staff,
    ClientIp(ip): ClientIp,
    Path(id): Path<Uuid>,
    ValidJson(body): ValidJson<Settle>,
) -> Result<Json<SettlementItem>, ApiError> {
    staff.require_admin()?;
    let mut tx = s.db.begin().await?;
    let row: Option<(Uuid, i64, String)> = sqlx::query_as("SELECT driver_id, amount_cents, status FROM payouts WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let (driver_id, amount, status) = row.ok_or_else(ApiError::not_found)?;
    if status != "pending" {
        return Err(ApiError::conflict("already_settled", "This statement is already settled."));
    }
    let new_status = if amount > 0 { "sent" } else { "received" };
    sqlx::query("UPDATE payouts SET status = $2, reference = $3, settled_at = now(), settled_by = $4 WHERE id = $1")
        .bind(id)
        .bind(new_status)
        .bind(body.reference.as_str())
        .bind(staff.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO driver_ledger (driver_id, payout_id, kind, amount_cents, note) VALUES ($1, $2, 'payout', $3, $4)")
        .bind(driver_id)
        .bind(id)
        .bind(-amount)
        .bind(if amount > 0 { "weekly payout sent" } else { "weekly service fees received" })
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut *tx,
        audit::Entry {
            staff_id: Some(staff.id),
            user_id: Some(driver_id),
            action: "settlement.settle",
            target_type: "payout",
            target_id: id.to_string(),
            detail: Some(json!({ "amount_cents": amount, "status": new_status, "reference": body.reference.as_str() })),
            ip: Some(ip),
        },
    )
    .await?;
    let row: Row = sqlx::query_as(&format!("{SELECT} WHERE p.id = $1")).bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(item(row)))
}
