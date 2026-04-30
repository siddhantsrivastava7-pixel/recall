//! Worker loop.
//!
//! One worker task per concurrency slot (1 on tier A, 2 on tier B/C). Each
//! worker:
//!
//!   1. Checks the master enabled flag — parks on `Notify` if off.
//!   2. Runs the throttling gate — parks (with a 30s ceiling so AC-state
//!      changes are picked up) if currently disallowed.
//!   3. Claims one queued/retry-eligible item atomically via SQL.
//!   4. Runs the OCR adapter behind `spawn_blocking`, persisting result
//!      text to `memories` and queue status to `ai_work_queue`.
//!
//! Empty-queue parking is `Notify::notified().await` with no timeout, so
//! idle CPU drops to zero. The scheduler calls `notify_one()` from the
//! capture hook and `notify_waiters()` from settings flips and "rebuild
//! index" so workers wake exactly when there's work to do.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};

use crate::ai::ocr::OcrAdapter;
use crate::ai::scheduler::queue::{ClaimedWork, OcrPayload, WorkKind, WorkPayload};
use crate::ai::scheduler::{throttling, SchedulerInner};
use crate::db::repositories::SharedMemoryRepository;
use crate::errors::app_error::{AppError, AppResult};

const PARKED_REEVAL_INTERVAL: Duration = Duration::from_secs(30);

/// Spawn `count` worker tasks. Each runs forever, parking on
/// `Notify::notified()` when there's nothing to do.
pub fn spawn_workers(
    inner: Arc<SchedulerInner>,
    pool: SqlitePool,
    memory_repo: SharedMemoryRepository,
    app: AppHandle,
    count: usize,
) {
    for slot in 0..count {
        let inner = inner.clone();
        let pool = pool.clone();
        let memory_repo = memory_repo.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            run_worker(inner, pool, memory_repo, app, slot).await;
        });
    }
}

async fn run_worker(
    inner: Arc<SchedulerInner>,
    pool: SqlitePool,
    memory_repo: SharedMemoryRepository,
    app: AppHandle,
    slot: usize,
) {
    let _ = pool; // reserved for future direct-SQL ops
    loop {
        // 1. Master enabled gate.
        if !inner.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            inner.notify.notified().await;
            continue;
        }

        // 2. Throttling (battery / AC-only).
        match throttling::can_run_now(&inner.settings).await {
            Ok(true) => {}
            Ok(false) => {
                // Park, but re-evaluate after a ceiling so AC plug-in is
                // picked up without explicit notification.
                tokio::select! {
                    _ = inner.notify.notified() => {}
                    _ = tokio::time::sleep(PARKED_REEVAL_INTERVAL) => {}
                }
                continue;
            }
            Err(error) => {
                eprintln!(
                    "[recall][ai-scheduler] slot {slot}: throttling settings read failed: {error}"
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }

        // 3. Claim next item (or park).
        let claimed = match inner.queue.claim_next(WorkKind::Ocr).await {
            Ok(Some(item)) => item,
            Ok(None) => {
                inner.notify.notified().await;
                continue;
            }
            Err(error) => {
                eprintln!(
                    "[recall][ai-scheduler] slot {slot}: claim failed: {error} — backing off 5s"
                );
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        // 4. Process. Capture diagnostic fields before `claimed` is moved
        // into `process_item` so we can persist failure status on the
        // memory row if processing returns an error.
        let id = claimed.id.clone();
        let attempts = claimed.attempts;
        let memory_id_for_status = match &claimed.payload {
            WorkPayload::Ocr(payload) => Some(payload.memory_id.clone()),
        };
        match process_item(&inner, &memory_repo, &app, claimed).await {
            Ok(()) => {
                if let Err(error) = inner.queue.mark_done(&id).await {
                    eprintln!(
                        "[recall][ai-scheduler] slot {slot}: mark_done failed for {id}: {error}"
                    );
                }
            }
            Err(error) => {
                let message = error.to_string();
                eprintln!(
                    "[recall][ai-scheduler] slot {slot}: item {id} failed (attempt {attempts}): {message}"
                );
                if let Err(persist_err) = inner.queue.mark_failed(&id, &message).await {
                    eprintln!(
                        "[recall][ai-scheduler] slot {slot}: mark_failed failed for {id}: {persist_err}"
                    );
                }
                // Best-effort: surface the failure on the memory row so the
                // UI can show "OCR failed". Ignore errors — the queue row
                // is the source of truth for retry; this is just a hint.
                if let Some(memory_id) = memory_id_for_status {
                    let now = Utc::now().to_rfc3339();
                    let _ = memory_repo
                        .set_ocr_status(&memory_id, "failed", Some(&message), None, Some(&now))
                        .await;
                }
            }
        }
    }
}

async fn process_item(
    inner: &SchedulerInner,
    memory_repo: &SharedMemoryRepository,
    app: &AppHandle,
    item: ClaimedWork,
) -> AppResult<()> {
    match item.payload {
        WorkPayload::Ocr(payload) => process_ocr(inner, memory_repo, app, payload).await,
    }
}

async fn process_ocr(
    inner: &SchedulerInner,
    memory_repo: &SharedMemoryRepository,
    app: &AppHandle,
    payload: OcrPayload,
) -> AppResult<()> {
    let adapter: Arc<dyn OcrAdapter> = inner
        .ocr_adapter
        .clone()
        .ok_or_else(|| AppError::Invalid("OCR adapter unavailable".into()))?;

    // Mark in-progress on the memory row for UI hints.
    memory_repo
        .set_ocr_status(&payload.memory_id, "running", None, None, None)
        .await?;

    // Phase 1 source path: memories store screenshot bytes externally as a
    // file path on `extracted_text` was reserved for HTML; image binaries
    // are stored at a path recorded on the memory's `url` field (file://
    // URI scheme) by the capture pipeline. Resolving that path is left to
    // a future refinement — for v0.2.0 we accept that OCR runs only when
    // we can read the source bytes, and return a clear error otherwise.
    let memory = memory_repo
        .find(&payload.memory_id)
        .await?
        .ok_or_else(|| AppError::Invalid(format!("Memory {} not found", payload.memory_id)))?;

    let image_bytes = read_image_bytes_for_memory(&memory).await?;
    let result = adapter.recognize_bytes(image_bytes).await?;

    let ocr_text = if result.text.trim().is_empty() {
        None
    } else {
        Some(result.text.clone())
    };
    let now = Utc::now().to_rfc3339();
    memory_repo
        .set_ocr_status(
            &payload.memory_id,
            "done",
            ocr_text.as_deref(),
            Some(&payload.engine),
            Some(&now),
        )
        .await?;

    // Notify the UI so any open detail panes refresh their search match
    // hits. The event payload is intentionally minimal.
    let _ = app.emit(
        "recall://memory-ocr-updated",
        serde_json::json!({ "memoryId": payload.memory_id }),
    );

    Ok(())
}

/// Read the raw image bytes for a memory eligible for OCR.
///
/// Phase 1 supports the two known carriers in the live schema:
///   * `url` set to a `file://` URI pointing at an on-disk image
///   * `extracted_text` already populated with raw image bytes — extremely
///     unlikely; included only for robustness.
///
/// When neither is present we return an error and the queue records it on
/// `last_error` for later diagnosis.
async fn read_image_bytes_for_memory(
    memory: &crate::models::Memory,
) -> AppResult<Vec<u8>> {
    if let Some(url) = memory.url.as_deref() {
        if let Some(path) = file_url_to_path(url) {
            return tokio::fs::read(&path)
                .await
                .map_err(|err| AppError::Invalid(format!("OCR could not read {path}: {err}")));
        }
    }
    Err(AppError::Invalid(
        "Memory does not carry an image path Recall can read; OCR skipped.".into(),
    ))
}

fn file_url_to_path(url: &str) -> Option<String> {
    let stripped = url.strip_prefix("file://")?;
    // Windows file URLs of form `file:///C:/...` end up with a leading `/`
    // before the drive letter; trim it so std::path is happy.
    #[cfg(target_os = "windows")]
    {
        let trimmed = stripped.trim_start_matches('/');
        return Some(trimmed.to_string());
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Some(stripped.to_string());
    }
}
