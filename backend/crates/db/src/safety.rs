//! SOS incidents and the trip links sent with them (PRD S-1, S-2).

use fuber_common::{
    crypto::{Hasher, Sealer, random_token},
    validate::PhoneE164,
};
use fuber_domain::safety::sos_text;
use sqlx::PgConnection;
use uuid::Uuid;

/// Associated data for the sealed `emergency_contacts.phone` column.
pub const CONTACT_PHONE_AAD: &str = "emergency_contacts.phone";

/// One SOS text, ready to send.
#[derive(Debug, Clone)]
pub struct SosText {
    /// The contact's position (1–3), to retry just the ones that failed.
    pub position: i16,
    pub to: PhoneE164,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SosError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("emergency contact {0} can't be read: {1}")]
    Contact(i16, String),
}

/// Builds the texts for an SOS: a fresh live trip link when there is a ride, and one message
/// per emergency contact (only those at `only`, when given). Empty for an unknown incident or
/// someone with no contacts.
pub async fn sos_texts(
    conn: &mut PgConnection,
    sealer: &Sealer,
    hasher: &Hasher,
    link_base: &str,
    incident_id: Uuid,
    only: Option<&[i16]>,
) -> Result<Vec<SosText>, SosError> {
    let Some(c) = sos_context(conn, incident_id).await? else { return Ok(vec![]) };
    let contacts: Vec<_> = c.contacts.into_iter().filter(|(p, _)| only.is_none_or(|o| o.contains(p))).collect();
    if contacts.is_empty() {
        return Ok(vec![]);
    }
    let link = match c.ride_id {
        Some(ride_id) => {
            let token = random_token();
            create_share(conn, ride_id, &share_hash(hasher, &token), c.party).await?;
            Some(format!("{link_base}/{token}"))
        }
        None => None,
    };
    let body = sos_text(&c.name, link.as_deref(), c.at);
    contacts
        .into_iter()
        .map(|(position, sealed)| {
            let phone = sealer.open_str(CONTACT_PHONE_AAD, &sealed).map_err(|e| SosError::Contact(position, e.to_string()))?;
            let to = PhoneE164::parse(&phone).map_err(|e| SosError::Contact(position, e.0.to_owned()))?;
            Ok(SosText { position, to, body: body.clone() })
        })
        .collect()
}

/// Her emergency contacts' numbers, opened. One that can't be read is logged and skipped.
pub async fn contact_phones(conn: &mut PgConnection, sealer: &Sealer, user_id: Uuid) -> Result<Vec<PhoneE164>, sqlx::Error> {
    let rows: Vec<(i16, Vec<u8>)> = sqlx::query_as("SELECT position, phone FROM emergency_contacts WHERE user_id = $1 ORDER BY position")
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(position, sealed)| {
            let phone = sealer.open_str(CONTACT_PHONE_AAD, &sealed).ok().and_then(|p| PhoneE164::parse(&p).ok());
            if phone.is_none() {
                tracing::error!(%user_id, position, "emergency contact can't be read");
            }
            phone
        })
        .collect())
}

/// How a trip-link token is stored (the website looks links up by this).
pub fn share_hash(hasher: &Hasher, token: &str) -> Vec<u8> {
    hasher.hash("trip-share", token.as_bytes()).to_vec()
}

/// What the emergency texts for one SOS need.
#[derive(Debug, Clone)]
pub struct SosContext {
    /// Her first name as other people see it.
    pub name: String,
    pub ride_id: Option<Uuid>,
    /// `rider` or `driver`: her side of the ride.
    pub party: &'static str,
    pub at: Option<(f64, f64)>,
    /// Position (1–3) and the sealed phone number of each emergency contact.
    pub contacts: Vec<(i16, Vec<u8>)>,
}

/// raised_by, display name, ride, raised by the ride's driver, latitude, longitude.
type IncidentRow = (Uuid, Option<String>, Option<Uuid>, bool, Option<f64>, Option<f64>);

pub async fn sos_context(conn: &mut PgConnection, incident_id: Uuid) -> Result<Option<SosContext>, sqlx::Error> {
    let row: Option<IncidentRow> = sqlx::query_as(
        "SELECT i.raised_by, coalesce(u.display_name, 'Someone'), i.ride_id, r.driver_id IS NOT DISTINCT FROM i.raised_by,
                ST_Y(i.location::geometry), ST_X(i.location::geometry)
         FROM safety_incidents i JOIN users u ON u.id = i.raised_by LEFT JOIN rides r ON r.id = i.ride_id
         WHERE i.id = $1 AND i.kind = 'sos'",
    )
    .bind(incident_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((user_id, name, ride_id, is_driver, lat, lon)) = row else { return Ok(None) };
    let contacts = sqlx::query_as("SELECT position, phone FROM emergency_contacts WHERE user_id = $1 ORDER BY position")
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(Some(SosContext {
        name: name.unwrap_or_default(),
        ride_id,
        party: if ride_id.is_some() && is_driver { "driver" } else { "rider" },
        at: lat.zip(lon),
        contacts,
    }))
}

/// Records a trip link; only the token's hash is stored. The link belongs to the ride's
/// current driver too, so it dies if the ride moves to another (migration 0012).
pub async fn create_share(conn: &mut PgConnection, ride_id: Uuid, token_hash: &[u8], party: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trip_shares (ride_id, token_hash, created_by, driver_id)
         SELECT id, $2, $3::party, driver_id FROM rides WHERE id = $1",
    )
    .bind(ride_id)
    .bind(token_hash)
    .bind(party)
    .execute(&mut *conn)
    .await
    .map(|_| ())
}
