//! Persisted AI work queue.
//!
//! All queue operations are single-statement SQL so a process death between
//! them never corrupts state. The atomic claim uses
//! `UPDATE ... WHERE status='queued' RETURNING ...` (SQLite ≥ 3.35), which
//! both selects and marks-running in one round trip.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::errors::app_error::AppResult;

/// Maximum number of attempts before a failed item stops being re-queued.
pub const MAX_ATTEMPTS: i64 = 3;

/// How long an item can stay in `running` before we consider the worker
/// dead and reclaim it back to `queued`. Generous on purpose: a 4096×4096
/// screenshot through Vision can take ~2s, plus blocking-pool scheduling
/// jitter, plus a battery-saver throttle interval. 5 minutes is plenty.
pub const STALE_RUNNING_AFTER_SECS: i64 = 300;

/// Queue item kinds. Phase 1 only emits `Ocr`; Phase 2+ will add `Embed`
/// and `Resurface` without schema changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Ocr,
}

impl WorkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkKind::Ocr => "ocr",
        }
    }
}

/// Status values stored on `ai_work_queue.status`.
pub mod status {
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
}

/// A claimed work item ready to run.
#[derive(Debug, Clone)]
pub struct ClaimedWork {
    pub id: String,
    pub kind: WorkKind,
    pub payload: WorkPayload,
    pub attempts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkPayload {
    Ocr(OcrPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrPayload {
    pub memory_id: String,
    pub engine: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindCounts {
    pub queued: u64,
    pub running: u64,
    pub failed: u64,
}

/// Database-backed queue. Cheap to clone (just the pool handle).
#[derive(Clone)]
pub struct AiWorkQueue {
    pool: SqlitePool,
}

impl AiWorkQueue {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert an OCR job for a memory. Returns `Ok(true)` if inserted,
    /// `Ok(false)` if a row with the same `dedupe_key` already existed.
    pub async fn enqueue_ocr(&self, memory_id: &str, engine: &str) -> AppResult<bool> {
        let id = Uuid::new_v4().to_string();
        let dedupe_key = ocr_dedupe_key(memory_id, engine);
        let payload = serde_json::to_string(&WorkPayload::Ocr(OcrPayload {
            memory_id: memory_id.to_string(),
            engine: engine.to_string(),
        }))?;
        let scheduled_for = Utc::now().to_rfc3339();

        let result = sqlx::query(
            r#"
            INSERT INTO ai_work_queue
              (id, kind, payload, dedupe_key, status, attempts, scheduled_for)
            VALUES (?, ?, ?, ?, ?, 0, ?)
            ON CONFLICT(dedupe_key) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(WorkKind::Ocr.as_str())
        .bind(&payload)
        .bind(&dedupe_key)
        .bind(status::QUEUED)
        .bind(&scheduled_for)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Bulk-enqueue an OCR job for every memory that's eligible for OCR
    /// today: source_type screenshot/imported_image, no successful OCR
    /// recorded yet, and no live job in the queue. Returns the number of
    /// rows inserted.
    pub async fn enqueue_ocr_backfill(&self, engine: &str) -> AppResult<u64> {
        // We compute candidate memory_ids and dedupe_keys in SQL to avoid a
        // round-trip per row. `ON CONFLICT DO NOTHING` makes this safe even
        // if a duplicate slipped in from a concurrent enqueue.
        let scheduled_for = Utc::now().to_rfc3339();
        let dedupe_prefix = format!("ocr:::");
        let _ = dedupe_prefix; // silence unused; building string per row below.
        let result = sqlx::query(
            r#"
            INSERT INTO ai_work_queue
              (id, kind, payload, dedupe_key, status, attempts, scheduled_for)
            SELECT
              lower(hex(randomblob(16))),
              'ocr',
              json_object('kind', 'ocr', 'memory_id', m.id, 'engine', ?1),
              'ocr:' || m.id || ':' || ?1,
              ?2,
              0,
              ?3
            FROM memories m
            WHERE m.source_type IN ('screenshot', 'imported_image')
              AND (m.ocr_status IS NULL OR m.ocr_status IN ('failed', 'pending'))
            ON CONFLICT(dedupe_key) DO NOTHING
            "#,
        )
        .bind(engine)
        .bind(status::QUEUED)
        .bind(&scheduled_for)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Atomically claim the next eligible queued (or retry-eligible failed)
    /// item, mark it `running`, and return its decoded payload. Returns
    /// `None` when nothing's available.
    pub async fn claim_next(&self, kind: WorkKind) -> AppResult<Option<ClaimedWork>> {
        let now = Utc::now().to_rfc3339();
        let row = sqlx::query(
            r#"
            UPDATE ai_work_queue
            SET status = 'running',
                attempts = attempts + 1,
                started_at = ?1
            WHERE id = (
              SELECT id FROM ai_work_queue
              WHERE kind = ?2
                AND status IN ('queued', 'failed')
                AND attempts < ?3
                AND (scheduled_for IS NULL OR scheduled_for <= ?1)
              ORDER BY scheduled_for ASC, id ASC
              LIMIT 1
            )
            RETURNING id, kind, payload, attempts
            "#,
        )
        .bind(&now)
        .bind(kind.as_str())
        .bind(MAX_ATTEMPTS)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else { return Ok(None) };

        let id: String = row.try_get("id")?;
        let kind_str: String = row.try_get("kind")?;
        let payload_raw: String = row.try_get("payload")?;
        let attempts: i64 = row.try_get("attempts")?;
        let payload: WorkPayload = serde_json::from_str(&payload_raw)?;
        let kind = match kind_str.as_str() {
            "ocr" => WorkKind::Ocr,
            other => {
                return Err(crate::errors::app_error::AppError::Invalid(format!(
                    "Unknown ai_work_queue.kind: {other}"
                )));
            }
        };

        Ok(Some(ClaimedWork {
            id,
            kind,
            payload,
            attempts,
        }))
    }

    pub async fn mark_done(&self, id: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE ai_work_queue
            SET status = 'done',
                last_error = NULL,
                finished_at = ?1
            WHERE id = ?2
            "#,
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark a running item failed. If `attempts < MAX_ATTEMPTS` the item
    /// stays eligible to retry on the next claim cycle (worker re-checks
    /// `attempts < MAX_ATTEMPTS` in the claim query). Otherwise it's
    /// effectively dead-lettered (status stays `failed`, but `attempts`
    /// has reached the cap so it won't be re-claimed).
    pub async fn mark_failed(&self, id: &str, error: &str) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        // Simple linear backoff: next attempt scheduled `attempts * 30s`
        // out from now. Even a flaky engine recovers without thrashing.
        sqlx::query(
            r#"
            UPDATE ai_work_queue
            SET status = 'failed',
                last_error = ?1,
                finished_at = ?2,
                scheduled_for = datetime(?2, '+' || (attempts * 30) || ' seconds')
            WHERE id = ?3
            "#,
        )
        .bind(error)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Revive items that were `running` when the process died. Called once
    /// at startup before any worker spawns, so every claim attempt sees a
    /// consistent table.
    pub async fn reclaim_stale_running(&self) -> AppResult<u64> {
        let now = Utc::now();
        let cutoff = now
            .checked_sub_signed(chrono::Duration::seconds(STALE_RUNNING_AFTER_SECS))
            .unwrap_or(now)
            .to_rfc3339();
        let result = sqlx::query(
            r#"
            UPDATE ai_work_queue
            SET status = 'queued',
                started_at = NULL,
                scheduled_for = ?1
            WHERE status = 'running'
              AND (started_at IS NULL OR started_at <= ?2)
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(&cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn counts_by_kind(&self, kind: WorkKind) -> AppResult<KindCounts> {
        let row = sqlx::query(
            r#"
            SELECT
              SUM(CASE WHEN status = 'queued'  THEN 1 ELSE 0 END) AS queued,
              SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running,
              SUM(CASE WHEN status = 'failed'  AND attempts >= ?2 THEN 1 ELSE 0 END) AS failed
            FROM ai_work_queue
            WHERE kind = ?1
            "#,
        )
        .bind(kind.as_str())
        .bind(MAX_ATTEMPTS)
        .fetch_one(&self.pool)
        .await?;
        Ok(KindCounts {
            queued: row.try_get::<Option<i64>, _>("queued")?.unwrap_or(0).max(0) as u64,
            running: row.try_get::<Option<i64>, _>("running")?.unwrap_or(0).max(0) as u64,
            failed: row.try_get::<Option<i64>, _>("failed")?.unwrap_or(0).max(0) as u64,
        })
    }
}

pub fn ocr_dedupe_key(memory_id: &str, engine: &str) -> String {
    format!("ocr:{memory_id}:{engine}")
}
