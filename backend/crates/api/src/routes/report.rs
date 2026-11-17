//! Report an issue (PRD S-5) from either app, during a ride or within 30 days after it.
//!
//! - **Safety reports** (harassment, unsafe driving, wrong person, wrong vehicle) also open a
//!   `report` incident, so the safety desk sees them in its queue next to SOS alerts, and the
//!   ride's track and chat are kept past the 90-day retention until the incident is closed.
//! - **Other reports** (lost item, fare, other) wait for support in `issue_reports`.
//! - Only the ride's rider or its current driver can report; 10 a day per person.

use axum::{
    Json,
    extract::{Path, State},
};
use fuber_common::validate::PlainText;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{App, AuthUser},
    error::{ApiError, ErrorBody},
    extract::ValidJson,
};

/// How long after a ride it can still be reported.
const REPORT_WINDOW_DAYS: i32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    Harassment,
    UnsafeDriving,
    WrongPerson,
    WrongVehicle,
    LostItem,
    Fare,
    Other,
}

impl IssueCategory {
    fn as_str(self) -> &'static str {
        match self {
            IssueCategory::Harassment => "harassment",
            IssueCategory::UnsafeDriving => "unsafe_driving",
            IssueCategory::WrongPerson => "wrong_person",
            IssueCategory::WrongVehicle => "wrong_vehicle",
            IssueCategory::LostItem => "lost_item",
            IssueCategory::Fare => "fare",
            IssueCategory::Other => "other",
        }
    }

    /// Goes to the safety desk, not only to support.
    pub fn is_safety(self) -> bool {
        matches!(self, IssueCategory::Harassment | IssueCategory::UnsafeDriving | IssueCategory::WrongPerson | IssueCategory::WrongVehicle)
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueReport {
    pub category: IssueCategory,
    /// What happened, in her words (1–2000 characters, plain text).
    #[schema(value_type = String)]
    pub text: PlainText<2000>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IssueReceipt {
    pub id: Uuid,
    /// The safety desk has it as an incident.
    pub safety: bool,
    pub message: String,
}

#[utoipa::path(
    post, path = "/v1/rides/{id}/report", tag = "safety", security(("bearer" = [])),
    params(("id" = Uuid, Path)),
    request_body = IssueReport,
    responses(
        (status = 201, description = "Recorded; safety categories also reach the safety desk", body = IssueReceipt),
        (status = 401, description = "Not signed in", body = ErrorBody),
        (status = 404, description = "Not her ride, or more than 30 days ago", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
        (status = 429, description = "Too many reports today", body = ErrorBody),
    )
)]
pub async fn report(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    ValidJson(req): ValidJson<IssueReport>,
) -> Result<(axum::http::StatusCode, Json<IssueReceipt>), ApiError> {
    s.limiter.check("reports", &user.user_id.to_string(), s.limits.reports).await?;
    let column = match user.app {
        App::Rider => "rider_id",
        App::Driver => "driver_id",
    };
    let mine: Option<bool> = sqlx::query_scalar(&format!(
        "SELECT true FROM rides WHERE id = $1 AND {column} = $2 AND requested_at > now() - make_interval(days => $3)"
    ))
    .bind(id)
    .bind(user.user_id)
    .bind(REPORT_WINDOW_DAYS)
    .fetch_optional(&s.db)
    .await?;
    if mine.is_none() {
        return Err(ApiError::not_found());
    }

    let safety = req.category.is_safety();
    let mut tx = s.db.begin().await?;
    let incident_id: Option<Uuid> = if safety {
        Some(
            sqlx::query_scalar("INSERT INTO safety_incidents (ride_id, raised_by, kind) VALUES ($1, $2, 'report') RETURNING id")
                .bind(id)
                .bind(user.user_id)
                .fetch_one(&mut *tx)
                .await?,
        )
    } else {
        None
    };
    let report_id: Uuid = sqlx::query_scalar(
        "INSERT INTO issue_reports (ride_id, reporter_id, category, body, incident_id) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(id)
    .bind(user.user_id)
    .bind(req.category.as_str())
    .bind(req.text.as_str())
    .bind(incident_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    if safety {
        tracing::warn!(security_event = "safety_report", report_id = %report_id, ride_id = %id, category = req.category.as_str(), "safety report");
    } else {
        tracing::info!(report_id = %report_id, ride_id = %id, category = req.category.as_str(), "issue reported");
    }

    let message = if safety {
        "Thank you for telling us. VARA's safety team will look into it and contact you. If you are in danger now, press SOS or call 10111."
    } else if req.category == IssueCategory::LostItem {
        "Thank you. VARA support will contact you about your lost item."
    } else {
        "Thank you. VARA support will contact you."
    };
    Ok((axum::http::StatusCode::CREATED, Json(IssueReceipt { id: report_id, safety, message: message.into() })))
}
