//! Driver eligibility rules shared by the API (ops decisions) and the worker (expiry job).

use sqlx::PgExecutor;
use uuid::Uuid;

/// Documents every driver needs approved and unexpired before going online (PRD A-4).
/// `transport_permit` joins this list if counsel confirms permits are required (PRD §10.5).
pub const REQUIRED_DOCUMENTS: [&str; 5] = ["identity", "selfie", "licence", "insurance", "police_clearance"];

/// Documents that carry an expiry date (PRD A-5).
pub const EXPIRING_DOCUMENTS: [&str; 4] = ["licence", "insurance", "police_clearance", "transport_permit"];

/// Recomputes `driver_profiles.can_go_online` for one driver: application approved, an
/// approved active vehicle, at least one approved tier, and every required document approved
/// and unexpired. Returns the new value.
pub async fn refresh_can_go_online<'e>(db: impl PgExecutor<'e>, driver_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE driver_profiles dp SET can_go_online = (
            dp.application_status = 'approved'
            AND EXISTS (SELECT 1 FROM vehicles v WHERE v.id = dp.active_vehicle_id AND v.approved)
            AND EXISTS (SELECT 1 FROM driver_tiers t WHERE t.driver_id = dp.user_id)
            AND (SELECT count(DISTINCT d.kind) FROM driver_documents d
                 WHERE d.driver_id = dp.user_id AND d.superseded_by IS NULL AND d.status = 'approved'
                   AND d.kind::text = ANY($2) AND (d.expires_on IS NULL OR d.expires_on >= current_date)) = cardinality($2)
          )
         WHERE dp.user_id = $1
         RETURNING can_go_online",
    )
    .bind(driver_id)
    .bind(&REQUIRED_DOCUMENTS[..])
    .fetch_optional(db)
    .await
    .map(|v| v.unwrap_or(false))
}
