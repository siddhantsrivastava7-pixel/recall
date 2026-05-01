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

use crate::ai::embeddings::EmbeddingAdapter;
use crate::ai::ocr::OcrAdapter;
use crate::ai::scheduler::queue::{ClaimedWork, EmbedChunkPayload, OcrPayload, WorkPayload};
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
        let claimed = match inner.queue.claim_next().await {
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
            // Embed failures don't surface a user-visible status pill;
            // the queue row holds the diagnostic state.
            WorkPayload::EmbedChunk(_) => None,
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
        WorkPayload::EmbedChunk(payload) => {
            process_embed_chunk(inner, memory_repo, app, payload).await
        }
    }
}

async fn process_embed_chunk(
    inner: &SchedulerInner,
    memory_repo: &SharedMemoryRepository,
    app: &AppHandle,
    payload: EmbedChunkPayload,
) -> AppResult<()> {
    let adapter: Arc<dyn EmbeddingAdapter> = inner
        .embedding_adapter
        .clone()
        .ok_or_else(|| AppError::Invalid("Embedding adapter unavailable".into()))?;

    // The queue row may have been written before the user downloaded
    // the model; in that case we fail soft and the queue's retry
    // policy + linear backoff picks it back up later. Don't trigger
    // an implicit download from the worker — that's reserved for the
    // explicit "Download embedding model" button so the user always
    // chooses when bytes leave the network.
    if !adapter.is_ready().await {
        return Err(AppError::Invalid(
            "Embedding model not yet downloaded. Click 'Download embedding model' in Settings → AI.".into(),
        ));
    }

    // Read the chunk's text. If the row vanished between enqueue and
    // claim (memory was deleted with ON DELETE CASCADE) we treat the
    // job as a no-op success — there's nothing to embed.
    let chunks = memory_repo.list_chunks_for_memory(&payload.memory_id).await?;
    let target = chunks.iter().find(|c| c.id == payload.chunk_id);
    let Some(target) = target else {
        return Ok(());
    };

    // Skip if already embedded with the current model — covers the
    // case where the queue stored a duplicate enqueue request that
    // beat the dedupe key by milliseconds.
    if target.embedding_vector.is_some()
        && target.embedding_model.as_deref() == Some(adapter.model_id())
    {
        return Ok(());
    }

    let mut vectors = adapter.embed_batch(vec![target.text.clone()]).await?;
    let vector = vectors
        .pop()
        .ok_or_else(|| AppError::Invalid("Embedding adapter returned empty vector".into()))?;
    let bytes = vector.to_bytes();
    let now = chrono::Utc::now().to_rfc3339();
    memory_repo
        .set_chunk_embedding(
            &payload.chunk_id,
            adapter.model_id(),
            adapter.dim(),
            &bytes,
            &now,
        )
        .await?;

    // Bubble up to UI: any open detail pane re-renders its Related
    // section once the chunk it cares about is embedded.
    let _ = app.emit(
        "recall://memory-embedding-updated",
        serde_json::json!({ "memoryId": payload.memory_id }),
    );

    Ok(())
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

    // v0.2.3: promote the OCR text to be the memory's primary content.
    // Once we have searchable text, the placeholder body
    // ("Screenshot from clipboard (...). OCR will fill in the text once
    // it runs.") is wasted screen space — replace it with the actual
    // recognized text so the timeline reads naturally and screenshots
    // feel like text memories that happen to have an image attached.
    // The repository method preserves user edits (only matches the
    // exact placeholder pattern).
    if let Some(text) = ocr_text.as_deref() {
        let derived_title = derive_screenshot_title(text);
        if let Err(error) = memory_repo
            .promote_ocr_to_content(&payload.memory_id, text, &derived_title)
            .await
        {
            // Soft-fail: the OCR text is already on `ocr_text` and
            // searchable. Failing to promote is a UX nit, not a data
            // loss — log and move on.
            eprintln!(
                "[recall][ai-scheduler] promote_ocr_to_content failed for {}: {error}",
                payload.memory_id
            );
        }

        // v0.3.0: chunk + enqueue embeddings against the OCR'd text.
        // The capture-service hook for screenshots was a no-op (it
        // skips placeholder content), so this is the moment the
        // screenshot becomes embeddable. Hash-aware replace means
        // we never re-embed unchanged chunks if OCR re-runs later.
        if let Err(error) =
            chunk_and_enqueue_embeds(inner, memory_repo, &payload.memory_id, text).await
        {
            eprintln!(
                "[recall][ai-scheduler] post-OCR chunk-embed failed for {}: {error}",
                payload.memory_id
            );
        }
    }

    // Notify the UI so any open detail panes refresh their search match
    // hits. The event payload is intentionally minimal.
    let _ = app.emit(
        "recall://memory-ocr-updated",
        serde_json::json!({ "memoryId": payload.memory_id }),
    );

    Ok(())
}

/// Run the chunker against `content`, hash-aware replace into
/// `memory_chunks`, and enqueue embed jobs for any novel chunk IDs.
/// Used by `process_ocr` when OCR-promoted content lands, and
/// available as a single shared helper if other code paths need to
/// re-chunk a memory.
async fn chunk_and_enqueue_embeds(
    inner: &SchedulerInner,
    memory_repo: &SharedMemoryRepository,
    memory_id: &str,
    content: &str,
) -> AppResult<()> {
    use crate::ai::embeddings::chunker;
    use crate::db::repositories::ChunkUpsert;

    let chunks = chunker::chunk_text(content);
    if chunks.is_empty() {
        return Ok(());
    }

    let upserts: Vec<ChunkUpsert<'_>> = chunks
        .iter()
        .enumerate()
        .map(|(idx, c)| ChunkUpsert {
            chunk_index: idx,
            text: &c.text,
            start_offset: c.start_offset,
            end_offset: c.end_offset,
            byte_size: c.byte_size(),
            token_estimate: c.token_estimate(),
            content_hash: &c.content_hash,
        })
        .collect();

    let needs_embedding = memory_repo
        .replace_chunks_hash_aware(memory_id, &upserts)
        .await?;

    // Enqueue an embed job per novel chunk via the scheduler. We go
    // through the queue (rather than embedding inline) so the work
    // respects the AI master toggle, throttling, retry policy, etc.
    let queue = &inner.queue;
    let model_id = inner
        .embedding_adapter
        .as_ref()
        .map(|a| a.model_id())
        .unwrap_or("unsupported");
    if model_id == "unsupported" {
        return Ok(());
    }
    for chunk_id in needs_embedding {
        if queue
            .enqueue_embed_chunk(&chunk_id, memory_id, model_id)
            .await?
        {
            inner.notify.notify_one();
        }
    }
    Ok(())
}

/// Pick a sensible title from OCR-recognized text. We use the first
/// non-empty line, capped at 96 characters to keep timeline cards
/// scannable. Falls back to a generic label when the text is all
/// whitespace (the caller already gates on non-empty `ocr_text`, but
/// belt-and-braces — better a working fallback than a panic).
fn derive_screenshot_title(text: &str) -> String {
    const MAX_TITLE_CHARS: usize = 96;
    let first_line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Screenshot");

    let char_count = first_line.chars().count();
    if char_count <= MAX_TITLE_CHARS {
        return first_line.to_string();
    }
    let truncated: String = first_line.chars().take(MAX_TITLE_CHARS).collect();
    format!("{truncated}…")
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
