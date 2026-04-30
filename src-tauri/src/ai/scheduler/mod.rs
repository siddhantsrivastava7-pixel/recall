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
    pub hardware: HardwareInfo,
    pub settings: SharedSettingsRepository,
    pub enabled: AtomicBool,
    pub notify: Notify,
}

impl AiScheduler {
    pub fn new(
        queue: AiWorkQueue,
        ocr_adapter: Option<Arc<dyn OcrAdapter>>,
        hardware: HardwareInfo,
        settings: SharedSettingsRepository,
        initially_enabled: bool,
    ) -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                queue,
                ocr_adapter,
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

    /// Stats for the AI status command.
    pub async fn status_snapshot(&self) -> AppResult<SchedulerStatus> {
        let counts = self.inner.queue.counts_by_kind(WorkKind::Ocr).await?;
        Ok(SchedulerStatus {
            enabled: self.is_enabled(),
            ocr_queued: counts.queued,
            ocr_running: counts.running,
            ocr_failed: counts.failed,
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
}
