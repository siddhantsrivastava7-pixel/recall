//! AI work scheduler.
//!
//! Phase 1 only handles `kind = "ocr"`. The queue table, worker loop, and
//! throttling logic are general enough that Phase 2's `embed` + Phase 3's
//! `resurface` jobs slot in without schema or worker-shape churn.
//!
//! Hard guarantees this module enforces:
//!
//! * Idle CPU near zero — workers `Notify::notified().await` on an empty
//!   queue, never busy-poll.
//! * Save path is never blocked — `enqueue_ocr_for_memory` does a single
//!   `INSERT OR IGNORE` and returns; processing happens later off-thread.
//! * Crash-safe — every claim/complete is a single SQL statement; dropping
//!   the process mid-OCR leaves the row in `running` and the next launch
//!   re-queues stale-running rows in [`AiWorkQueue::reclaim_stale_running`].
//! * Idempotent — `dedupe_key UNIQUE` prevents enqueueing the same OCR job
//!   twice; the `INSERT OR IGNORE` returns `Ok(false)` when a duplicate is
//!   silently skipped.

pub mod queue;
pub mod throttling;
pub mod worker;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::Notify;

use crate::ai::embeddings::EmbeddingAdapter;
use crate::ai::hardware::HardwareInfo;
use crate::ai::ocr::OcrAdapter;
use crate::db::repositories::SharedSettingsRepository;
use crate::errors::app_error::AppResult;

use queue::{AiWorkQueue, WorkKind};

/// Long-lived handle held on `AppState`. Cheap to clone (everything inside
/// is `Arc` / atomic) so commands and the capture-service hook can each
/// hold their own copy.
#[derive(Clone)]
pub struct AiScheduler {
    inner: Arc<SchedulerInner>,
}

pub(crate) struct SchedulerInner {
    pub queue: AiWorkQueue,
    pub ocr_adapter: Option<Arc<dyn OcrAdapter>>,
    pub embedding_adapter: Option<Arc<dyn EmbeddingAdapter>>,
    pub hardware: HardwareInfo,
    pub settings: SharedSettingsRepository,
    pub enabled: AtomicBool,
    pub notify: Notify,
}

impl AiScheduler {
    pub fn new(
        queue: AiWorkQueue,
        ocr_adapter: Option<Arc<dyn OcrAdapter>>,
        embedding_adapter: Option<Arc<dyn EmbeddingAdapter>>,
        hardware: HardwareInfo,
        settings: SharedSettingsRepository,
        initially_enabled: bool,
    ) -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                queue,
                ocr_adapter,
                embedding_adapter,
                hardware,
                settings,
                enabled: AtomicBool::new(initially_enabled),
                notify: Notify::new(),
            }),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::Relaxed)
    }

    /// Toggle the master switch. Setting `true` wakes idle workers; setting
    /// `false` lets in-flight items finish but stops claiming new work.
    pub fn set_enabled(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::Relaxed);
        // Wake every parked worker so they re-evaluate the enabled flag.
        self.inner.notify.notify_waiters();
    }

    /// Currently-detected hardware tier, exposed in the AI settings tab.
    pub fn hardware(&self) -> &HardwareInfo {
        &self.inner.hardware
    }

    /// OCR engine label exposed in Settings, or `"unsupported"` when no
    /// native engine is available on this host.
    pub fn ocr_engine_label(&self) -> &'static str {
        self.inner
            .ocr_adapter
            .as_ref()
            .map(|adapter| adapter.engine())
            .unwrap_or("unsupported")
    }

    /// Enqueue an OCR job for a memory. Returns `Ok(true)` when a new row
    /// was inserted, `Ok(false)` when a duplicate (same `dedupe_key`)
    /// already exists.
    pub async fn enqueue_ocr_for_memory(&self, memory_id: &str) -> AppResult<bool> {
        let Some(adapter) = self.inner.ocr_adapter.as_ref() else {
            return Ok(false); // OCR unavailable on this host — silent skip.
        };
        let inserted = self
            .inner
            .queue
            .enqueue_ocr(memory_id, adapter.engine())
            .await?;
        if inserted {
            self.inner.notify.notify_one();
        }
        Ok(inserted)
    }

    /// Re-enqueue OCR for every memory currently eligible (used by the
    /// "Run OCR rebuild" settings button).
    pub async fn rebuild_ocr_index(&self) -> AppResult<u64> {
        let Some(adapter) = self.inner.ocr_adapter.as_ref() else {
            return Ok(0);
        };
        let count = self
            .inner
            .queue
            .enqueue_ocr_backfill(adapter.engine())
            .await?;
        if count > 0 {
            self.inner.notify.notify_waiters();
        }
        Ok(count)
    }

    /// Embedding model id label (e.g. `"bge-small-en-v1.5"`) or
    /// `"unsupported"` when no embedding adapter is wired.
    pub fn embedding_model_label(&self) -> &'static str {
        self.inner
            .embedding_adapter
            .as_ref()
            .map(|adapter| adapter.model_id())
            .unwrap_or("unsupported")
    }

    /// True when the embedding adapter exists *and* its model file is
    /// already on-disk locally. Used by the AI Settings tab to flip
    /// the "Download embedding model" button into a green check.
    pub async fn embedding_is_ready(&self) -> bool {
        match self.inner.embedding_adapter.as_ref() {
            Some(adapter) => adapter.is_ready().await,
            None => false,
        }
    }

    /// Trigger embedding model download/preparation. Returns once the
    /// adapter is ready to embed.
    pub async fn prepare_embedding_model(&self) -> AppResult<()> {
        let Some(adapter) = self.inner.embedding_adapter.as_ref() else {
            return Err(crate::errors::app_error::AppError::Invalid(
                "Embedding adapter not configured on this host.".into(),
            ));
        };
        adapter.prepare().await
    }

    /// Enqueue an embedding job for a chunk. Idempotent via dedupe_key.
    pub async fn enqueue_embed_chunk(
        &self,
        chunk_id: &str,
        memory_id: &str,
    ) -> AppResult<bool> {
        let Some(adapter) = self.inner.embedding_adapter.as_ref() else {
            return Ok(false);
        };
        let inserted = self
            .inner
            .queue
            .enqueue_embed_chunk(chunk_id, memory_id, adapter.model_id())
            .await?;
        if inserted {
            self.inner.notify.notify_one();
        }
        Ok(inserted)
    }

    /// Stats for the AI status command.
    pub async fn status_snapshot(&self) -> AppResult<SchedulerStatus> {
        let ocr = self.inner.queue.counts_by_kind(WorkKind::Ocr).await?;
        let embed = self.inner.queue.counts_by_kind(WorkKind::EmbedChunk).await?;
        Ok(SchedulerStatus {
            enabled: self.is_enabled(),
            ocr_queued: ocr.queued,
            ocr_running: ocr.running,
            ocr_failed: ocr.failed,
            embed_queued: embed.queued,
            embed_running: embed.running,
            embed_failed: embed.failed,
        })
    }

    pub(crate) fn inner(&self) -> Arc<SchedulerInner> {
        self.inner.clone()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerStatus {
    pub enabled: bool,
    pub ocr_queued: u64,
    pub ocr_running: u64,
    pub ocr_failed: u64,
    pub embed_queued: u64,
    pub embed_running: u64,
    pub embed_failed: u64,
}
