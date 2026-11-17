//! The driver's weekly statements (PRD P-6): what each week came to and whether it is settled.
//! The statements are made by the worker (`fuber_worker::settlement`) and settled by ops.

use axum::{Json, extract::State};
use fuber_domain::Cents;
use serde::Serialize;
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::DriverUser,
    error::{ApiError, ErrorBody},
};

#[derive(Debug, Serialize, ToSchema)]
pub struct Statement {
    pub id: Uuid,
    /// Monday to Sunday, Windhoek time.
    #[schema(value_type = String, format = Date)]
    pub period_start: String,
    #[schema(value_type = String, format = Date)]
    pub period_end: String,
    /// Positive: VARA pays her. Negative: she pays VARA (service fees on cash trips).
    pub amount: Cents,
    /// `pending`, `sent` (VARA paid her) or `received` (VARA received her payment).
    pub status: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub settled_at: Option<OffsetDateTime>,
}

#[utoipa::path(
    get, path = "/v1/driver/statements", tag = "earnings", security(("bearer" = [])),
    responses(
        (status = 200, description = "Her weekly statements, newest first (the last 26 weeks)", body = Vec<Statement>),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "Driver app only", body = ErrorBody),
    )
)]
pub async fn statements(State(s): State<AppState>, DriverUser(user): DriverUser) -> Result<Json<Vec<Statement>>, ApiError> {
    let rows: Vec<(Uuid, Date, Date, i64, String, Option<OffsetDateTime>)> = sqlx::query_as(
        "SELECT id, period_start, period_end, amount_cents, status, settled_at FROM payouts
         WHERE driver_id = $1 ORDER BY period_start DESC LIMIT 26",
    )
    .bind(user.user_id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, start, end, amount, status, settled_at)| Statement {
                id,
                period_start: start.to_string(),
                period_end: end.to_string(),
                amount: Cents(amount),
                status,
                settled_at,
            })
            .collect(),
    ))
}
