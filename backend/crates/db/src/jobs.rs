//! Postgres-backed job queue (TRD §2.1): the API enqueues, the worker claims with
//! `FOR UPDATE SKIP LOCKED`, so several workers can run without double-processing.

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;

pub mod kind {
    /// `{"upload_id": uuid}`: check, scan and re-encode an upload.
    pub const PROCESS_UPLOAD: &str = "upload.process";
    /// `{"user_id": uuid}`: erase a deleted account after its grace period.
    pub const ERASE_ACCOUNT: &str = "account.erase";
}

/// How long a claimed job is hidden from other workers before it is retried.
const LEASE_SECONDS: i64 = 300;

#[derive(Debug, sqlx::FromRow)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload: Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

/// Queues a job. With a `dedupe_key`, a second job with the same key is ignored while the
/// first is still pending.
pub async fn enqueue<'e>(
    db: impl PgExecutor<'e>,
    kind: &str,
    payload: Value,
    run_at: Option<OffsetDateTime>,
    dedupe_key: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO jobs (kind, payload, run_at, dedupe_key) VALUES ($1, $2, coalesce($3, now()), $4)
         ON CONFLICT (dedupe_key) WHERE done_at IS NULL AND dedupe_key IS NOT NULL DO NOTHING",
    )
    .bind(kind)
    .bind(payload)
    .bind(run_at)
    .bind(dedupe_key)
    .execute(db)
    .await
    .map(|_| ())
}

/// Claims the next due job, leasing it for a few minutes. A worker that dies mid-job leaves
/// the lease to expire and the job is retried.
pub async fn claim(db: &PgPool) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs SET attempts = attempts + 1, locked_until = now() + make_interval(secs => $1)
         WHERE id = (
           SELECT id FROM jobs
           WHERE done_at IS NULL AND run_at <= now() AND attempts < max_attempts
             AND (locked_until IS NULL OR locked_until < now())
           ORDER BY run_at
           LIMIT 1
           FOR UPDATE SKIP LOCKED)
         RETURNING id, kind, payload, attempts, max_attempts",
    )
    .bind(LEASE_SECONDS as f64)
    .fetch_optional(db)
    .await
}

pub async fn complete(db: &PgPool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE jobs SET done_at = now(), locked_until = NULL, last_error = NULL WHERE id = $1")
        .bind(id)
        .execute(db)
        .await
        .map(|_| ())
}

/// Records a failure and schedules a retry with exponential back-off (30 s, 1 min, 2 min, …
/// capped at 1 h). After `max_attempts` the job stays in the table, undone, for investigation.
pub async fn fail(db: &PgPool, job: &Job, error: &str) -> Result<(), sqlx::Error> {
    let backoff_s = (30_i64 << (job.attempts.clamp(1, 8) - 1)).min(3600);
    sqlx::query("UPDATE jobs SET last_error = $2, locked_until = NULL, run_at = now() + make_interval(secs => $3) WHERE id = $1")
        .bind(job.id)
        .bind(error.chars().take(500).collect::<String>())
        .bind(backoff_s as f64)
        .execute(db)
        .await
        .map(|_| ())
}
