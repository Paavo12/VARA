//! FUber background work (TRD §2.1), as a library so the API's integration tests can run the
//! same code the worker process runs.
//!
//! - `upload.process`: virus scan, type check and re-encode an upload (TRD §7.2).
//! - `account.erase`: erase a deleted account after its 30-day grace period (PRD A-8).
//! - `safety.sos_sms`: resend SOS texts the API could not send (PRD S-1).
//! - Daily document check: expire documents and send 30/7/1-day warnings (PRD A-5).
//! - Hourly cleanup: OTP challenges, old sessions, abandoned uploads (retention, schema §12).
//! - Every second: matching (see [`dispatch`]).

pub mod dispatch;

use fuber_common::{
    crypto::{Hasher, Sealer},
    telemetry::mask_phone,
    validate::PhoneE164,
};
use fuber_db::{
    drivers::refresh_can_go_online,
    jobs::{self, Job},
    sqlx::{self, PgPool},
};
use fuber_integrations::{
    media,
    routing::Routing,
    sms::SmsGateway,
    storage::ObjectStore,
    virus::{Verdict, VirusScanner},
};
use sha2::{Digest, Sha256};
use time::Date;
use uuid::Uuid;

pub struct Ctx {
    pub db: PgPool,
    /// Opens the 🔒 columns this worker needs (guest and emergency-contact phone numbers).
    pub sealer: Sealer,
    /// Hashes trip-link tokens.
    pub hasher: Hasher,
    /// Base of the live trip links in SOS texts (`Config::trip_link_base`).
    pub trip_link_base: String,
    pub store: ObjectStore,
    pub scanner: VirusScanner,
    pub sms: SmsGateway,
    pub routing: Routing,
}

/// A failure worth retrying (database or scanner down). Problems with the file itself are
/// not errors: they reject the upload and complete the job.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("{0}")]
    Other(String),
}

/// Claims and runs due jobs until none is left. Returns how many ran.
pub async fn drain(ctx: &Ctx) -> Result<usize, sqlx::Error> {
    let mut n = 0;
    while let Some(job) = jobs::claim(&ctx.db).await? {
        n += 1;
        match run(ctx, &job).await {
            Ok(()) => jobs::complete(&ctx.db, job.id).await?,
            Err(e) => {
                tracing::warn!(job = job.id, kind = %job.kind, attempt = job.attempts, error = %e, "job failed; will retry");
                jobs::fail(&ctx.db, &job, &e.to_string()).await?;
            }
        }
    }
    Ok(n)
}

async fn run(ctx: &Ctx, job: &Job) -> Result<(), JobError> {
    let id = |field: &str| {
        job.payload
            .get(field)
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<Uuid>().ok())
            .ok_or_else(|| JobError::Other(format!("payload has no {field}")))
    };
    match job.kind.as_str() {
        jobs::kind::PROCESS_UPLOAD => process_upload(ctx, id("upload_id")?).await,
        jobs::kind::ERASE_ACCOUNT => erase_account(ctx, id("user_id")?).await,
        jobs::kind::GUEST_SMS => guest_sms(ctx, id("ride_id")?).await,
        jobs::kind::SOS_SMS => {
            let positions: Vec<i16> = job
                .payload
                .get("positions")
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .ok_or_else(|| JobError::Other("payload has no positions".into()))?;
            sos_sms(ctx, id("incident_id")?, &positions).await
        }
        other => Err(JobError::Other(format!("unknown job kind {other}"))),
    }
}

// ---------------------------------------------------------------------------------------------

pub async fn process_upload(ctx: &Ctx, upload_id: Uuid) -> Result<(), JobError> {
    let row: Option<(Uuid, String, i32, String)> =
        sqlx::query_as("SELECT user_id, declared_type, declared_size, status::text FROM uploads WHERE id = $1")
            .bind(upload_id)
            .fetch_optional(&ctx.db)
            .await?;
    let Some((user_id, declared_type, declared_size, status)) = row else { return Ok(()) };
    if status != "processing" {
        return Ok(()); // already handled by an earlier attempt
    }
    let quarantine = format!("quarantine/{upload_id}");
    let bytes = match ctx.store.read(&quarantine).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return reject(ctx, upload_id, "We didn't receive the file. Please try again.").await;
        }
        Err(e) => return Err(JobError::Other(format!("storage read failed: {e}"))),
    };
    if bytes.len() > declared_size as usize {
        return reject(ctx, upload_id, "The file is larger than expected. Please choose it again.").await;
    }
    match ctx.scanner.scan(&bytes).await.map_err(|e| JobError::Other(e.to_string()))? {
        Verdict::Infected(signature) => {
            tracing::warn!(security_event = "upload_infected", %upload_id, %user_id, %signature, "upload failed virus scan");
            return reject(ctx, upload_id, "This file failed our security check. Please take a new photo.").await;
        }
        Verdict::Clean | Verdict::NotScanned => {}
    }
    let clean = match tokio::task::spawn_blocking(move || media::sanitize(&bytes, &declared_type)).await {
        Ok(Ok(c)) => c,
        Ok(Err(media::Rejected(reason))) => return reject(ctx, upload_id, reason).await,
        Err(e) => return Err(JobError::Other(format!("sanitiser crashed: {e}"))),
    };
    let key = format!("clean/{user_id}/{upload_id}.{}", clean.extension);
    ctx.store.write(&key, &clean.bytes).await.map_err(|e| JobError::Other(format!("storage write failed: {e}")))?;
    let sha256: Vec<u8> = Sha256::digest(&clean.bytes).to_vec();

    let mut tx = ctx.db.begin().await?;
    sqlx::query(
        "UPDATE uploads SET status = 'clean', storage_key = $2, mime_type = $3, size_bytes = $4, sha256 = $5, processed_at = now()
         WHERE id = $1 AND status = 'processing'",
    )
    .bind(upload_id)
    .bind(&key)
    .bind(clean.mime)
    .bind(clean.bytes.len() as i32)
    .bind(&sha256)
    .execute(&mut *tx)
    .await?;
    // Documents and verifications submitted while the file was being checked can now be reviewed.
    sqlx::query(
        "UPDATE driver_documents SET status = 'in_review', storage_key = $2, mime_type = $3, size_bytes = $4, sha256 = $5
         WHERE upload_id = $1 AND status = 'uploading'",
    )
    .bind(upload_id)
    .bind(&key)
    .bind(clean.mime)
    .bind(clean.bytes.len() as i32)
    .bind(&sha256)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE identity_verifications SET id_document_key = $2 WHERE id_upload_id = $1")
        .bind(upload_id)
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE identity_verifications SET selfie_key = $2 WHERE selfie_upload_id = $1")
        .bind(upload_id)
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    delete_object(ctx, &quarantine).await;
    tracing::info!(%upload_id, "upload clean");
    Ok(())
}

async fn reject(ctx: &Ctx, upload_id: Uuid, reason: &str) -> Result<(), JobError> {
    let mut tx = ctx.db.begin().await?;
    sqlx::query("UPDATE uploads SET status = 'rejected', reject_reason = $2, processed_at = now() WHERE id = $1")
        .bind(upload_id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE driver_documents SET status = 'rejected', reject_reason = $2 WHERE upload_id = $1 AND status = 'uploading'")
        .bind(upload_id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    let users: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE identity_verifications SET status = 'rejected', reject_reason = $2
         WHERE (id_upload_id = $1 OR selfie_upload_id = $1) AND status = 'pending' RETURNING user_id",
    )
    .bind(upload_id)
    .bind(reason)
    .fetch_all(&mut *tx)
    .await?;
    for u in users {
        sqlx::query("UPDATE users SET verification = 'rejected' WHERE id = $1 AND verification = 'pending'")
            .bind(u)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    delete_object(ctx, &format!("quarantine/{upload_id}")).await;
    tracing::info!(%upload_id, "upload rejected");
    Ok(())
}

async fn delete_object(ctx: &Ctx, key: &str) {
    if let Err(e) = ctx.store.delete(key).await {
        tracing::warn!(error = %e, "could not delete object; cleanup will retry");
    }
}

// ---------------------------------------------------------------------------------------------

/// Erases a deleted account's personal data. Rides, payments and consents stay, linked to an
/// anonymous user row, for the periods the Privacy Policy allows.
pub async fn erase_account(ctx: &Ctx, user_id: Uuid) -> Result<(), JobError> {
    let due: Option<bool> =
        sqlx::query_scalar("SELECT deleted_at <= now() - interval '30 days' FROM users WHERE id = $1 AND status = 'deleted'")
            .bind(user_id)
            .fetch_optional(&ctx.db)
            .await?
            .flatten();
    if due != Some(true) {
        return Ok(()); // restored by support, or not due (job ran early)
    }
    let mut tx = ctx.db.begin().await?;
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT storage_key FROM uploads WHERE user_id = $1 AND storage_key IS NOT NULL
         UNION SELECT id_document_key FROM identity_verifications WHERE user_id = $1 AND id_document_key IS NOT NULL
         UNION SELECT selfie_key FROM identity_verifications WHERE user_id = $1 AND selfie_key IS NOT NULL
         UNION SELECT storage_key FROM driver_documents WHERE driver_id = $1 AND storage_key IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    let upload_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM uploads WHERE user_id = $1").bind(user_id).fetch_all(&mut *tx).await?;
    for q in [
        "DELETE FROM identity_verifications WHERE user_id = $1",
        "DELETE FROM driver_documents WHERE driver_id = $1",
        "DELETE FROM uploads WHERE user_id = $1",
        "DELETE FROM emergency_contacts WHERE user_id = $1",
        "DELETE FROM saved_places WHERE user_id = $1",
        "DELETE FROM payment_methods WHERE user_id = $1",
        "DELETE FROM sessions WHERE user_id = $1",
        "UPDATE driver_profiles SET payout_account = NULL, active_vehicle_id = NULL, can_go_online = false WHERE user_id = $1",
        "DELETE FROM vehicles v WHERE driver_id = $1 AND NOT EXISTS (SELECT 1 FROM rides r WHERE r.vehicle_id = v.id)",
        "UPDATE users SET phone_e164 = NULL, full_name = NULL, display_name = NULL, email = NULL, date_of_birth = NULL,
                          national_id = NULL, national_id_hash = NULL, photo_key = NULL
         WHERE id = $1",
    ] {
        sqlx::query(q).bind(user_id).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO audit_log (user_id, action, target_type, target_id) VALUES ($1, 'account.erased', 'user', $1::text)")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    for k in keys.iter().chain(upload_ids.iter().map(|id| format!("quarantine/{id}")).collect::<Vec<_>>().iter()) {
        delete_object(ctx, k).await;
    }
    tracing::info!(%user_id, files = keys.len(), "account erased");
    Ok(())
}

// ---------------------------------------------------------------------------------------------

type GuestRow = (Vec<u8>, String, String, Option<String>, Option<String>, String, String, String, String);

/// Texts a guest passenger who is coming to pick her up (PRD B-5). Sent once, after a driver
/// accepts; a ride cancelled before then sends nothing.
pub async fn guest_sms(ctx: &Ctx, ride_id: Uuid) -> Result<(), JobError> {
    let row: Option<GuestRow> = sqlx::query_as(
        "SELECT r.guest_phone, r.guest_name, r.pin, booker.display_name, d.display_name, v.colour, v.make || ' ' || v.model, v.plate, r.code
         FROM rides r
         JOIN users booker ON booker.id = r.rider_id
         JOIN users d ON d.id = r.driver_id
         JOIN vehicles v ON v.id = r.vehicle_id
         WHERE r.id = $1 AND r.guest_phone IS NOT NULL AND r.state::text = ANY($2)",
    )
    .bind(ride_id)
    .bind(&fuber_db::rides::DRIVER_ACTIVE[..])
    .fetch_optional(&ctx.db)
    .await?;
    let Some((phone, guest, pin, booker, driver, colour, car, plate, code)) = row else { return Ok(()) };
    let phone = ctx.sealer.open_str("rides.guest_phone", &phone).map_err(|e| JobError::Other(format!("guest phone: {e}")))?;
    let driver = driver.unwrap_or_else(|| "Your driver".into());
    let booker = booker.unwrap_or_else(|| "A friend".into());
    let body = format!(
        "FUber {code}: Hi {guest}, {booker} booked you a ride. {driver} is coming in a {colour} {car}, plate {plate}.          Your PIN is {pin}: say it to her at pickup, and only get in if the plate matches."
    );
    notify(ctx, Some(phone), &body).await;
    Ok(())
}

/// Resends the SOS texts that failed, each with a fresh trip link. Texts that fail again are
/// retried as a job of their own, so the ones that went through are never sent twice.
pub async fn sos_sms(ctx: &Ctx, incident_id: Uuid, positions: &[i16]) -> Result<(), JobError> {
    let mut tx = ctx.db.begin().await?;
    let texts = fuber_db::safety::sos_texts(&mut tx, &ctx.sealer, &ctx.hasher, &ctx.trip_link_base, incident_id, Some(positions))
        .await
        .map_err(|e| JobError::Other(e.to_string()))?;
    tx.commit().await?;
    let mut failed = vec![];
    for t in texts {
        if let Err(e) = ctx.sms.send(&t.to, &t.body).await {
            tracing::warn!(error = %e, %incident_id, position = t.position, "SOS text failed again");
            failed.push(t.position);
        }
    }
    if failed.len() == positions.len() {
        return Err(JobError::Other("no SOS text could be sent".into()));
    }
    if !failed.is_empty() {
        let payload = serde_json::json!({ "incident_id": incident_id, "positions": failed });
        jobs::enqueue(&ctx.db, jobs::kind::SOS_SMS, payload, None, None).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------

fn document_name(kind: &str) -> &'static str {
    match kind {
        "licence" => "driver's licence",
        "insurance" => "insurance",
        "police_clearance" => "police clearance",
        "transport_permit" => "transport permit",
        _ => "document",
    }
}

async fn notify(ctx: &Ctx, phone: Option<String>, body: &str) {
    let Some(phone) = phone.and_then(|p| PhoneE164::parse(&p).ok()) else { return };
    if let Err(e) = ctx.sms.send(&phone, body).await {
        tracing::warn!(error = %e, to = %mask_phone(phone.as_str()), "notification SMS failed");
    }
}

/// Expires documents past their date (the driver goes offline at once) and sends each
/// warning once. Safe to run more than once a day and from several workers: every change is a
/// conditional UPDATE, so only one run sends a given message.
pub async fn check_documents(ctx: &Ctx, today: Date) -> Result<(), JobError> {
    let expired: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "UPDATE driver_documents d SET status = 'expired' FROM users u
         WHERE u.id = d.driver_id AND d.status = 'approved' AND d.superseded_by IS NULL AND d.expires_on < $1
         RETURNING d.driver_id, d.kind::text, u.phone_e164",
    )
    .bind(today)
    .fetch_all(&ctx.db)
    .await?;
    for (driver, kind, phone) in expired {
        refresh_can_go_online(&ctx.db, driver).await?;
        let name = document_name(&kind);
        notify(
            ctx,
            phone,
            &format!("FUber: your {name} has expired, so you can't go online. Upload the new one in the FUber driver app to drive again."),
        )
        .await;
    }
    // Smallest threshold first, so a document already inside 7 days gets only the 7-day warning.
    for days in [1_i16, 7, 30] {
        let due: Vec<(String, Date, Option<String>)> = sqlx::query_as(
            "UPDATE driver_documents d SET expiry_warned_days = $2 FROM users u
             WHERE u.id = d.driver_id AND d.status = 'approved' AND d.superseded_by IS NULL
               AND d.expires_on >= $1 AND d.expires_on <= $1 + $2::int
               AND (d.expiry_warned_days IS NULL OR d.expiry_warned_days > $2)
             RETURNING d.kind::text, d.expires_on, u.phone_e164",
        )
        .bind(today)
        .bind(days)
        .fetch_all(&ctx.db)
        .await?;
        for (kind, on, phone) in due {
            let name = document_name(&kind);
            notify(
                ctx,
                phone,
                &format!("FUber: your {name} expires on {on}. Upload the new one in the FUber driver app before then to keep driving."),
            )
            .await;
        }
    }
    Ok(())
}

/// Old ID-photo key, old selfie key, and their upload IDs.
type ExpiredImages = (Option<String>, Option<String>, Option<Uuid>, Option<Uuid>);

/// Retention clean-up (docs/05-backend-schema.md §12).
pub async fn cleanup(ctx: &Ctx) -> Result<(), JobError> {
    sqlx::query("DELETE FROM otp_challenges WHERE created_at < now() - interval '24 hours'").execute(&ctx.db).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < now() - interval '7 days' OR revoked_at < now() - interval '30 days'")
        .execute(&ctx.db)
        .await?;
    sqlx::query("DELETE FROM staff_sessions WHERE expires_at < now() - interval '7 days'").execute(&ctx.db).await?;
    let abandoned: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM uploads WHERE status = 'awaiting_upload' AND url_expires_at < now() - interval '1 hour' RETURNING id",
    )
    .fetch_all(&ctx.db)
    .await?;
    for id in abandoned {
        delete_object(ctx, &format!("quarantine/{id}")).await;
    }
    // Rider identity images: kept until the decision + 30 days, then deleted; the decision stays.
    let old: Vec<ExpiredImages> = sqlx::query_as(
        "WITH old AS (
           SELECT id, id_document_key, selfie_key, id_upload_id, selfie_upload_id FROM identity_verifications
           WHERE status IN ('approved', 'rejected') AND reviewed_at < now() - interval '30 days'
             AND (id_document_key IS NOT NULL OR selfie_key IS NOT NULL)
           FOR UPDATE)
         UPDATE identity_verifications iv SET id_document_key = NULL, selfie_key = NULL FROM old WHERE iv.id = old.id
         RETURNING old.id_document_key, old.selfie_key, old.id_upload_id, old.selfie_upload_id",
    )
    .fetch_all(&ctx.db)
    .await?;
    let uploads: Vec<Uuid> = old.iter().flat_map(|(_, _, a, b)| [*a, *b]).flatten().collect();
    sqlx::query("DELETE FROM uploads WHERE id = ANY($1)").bind(&uploads).execute(&ctx.db).await?;
    for key in old.into_iter().flat_map(|(a, b, _, _)| [a, b]).flatten() {
        delete_object(ctx, &key).await;
    }
    Ok(())
}
