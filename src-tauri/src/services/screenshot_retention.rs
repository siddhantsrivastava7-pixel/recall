//! Screenshot retention garbage collection.
//!
//! Recall captures every clipboard image as a memory + runs OCR on it
//! so the text is searchable later. The image bytes themselves only
//! provide visual context for a window of time — after that, the OCR
//! text is the long-term value. Keeping the original PNG forever
//! quietly accumulates disk: a heavy clipboard user can hit ~10 GB/year
//! of bytes they didn't intentionally save.
//!
//! Policy (v0.2.3, hardcoded): files older than 60 days are deleted.
//! The memory row + OCR text stay; only `memory.url` is cleared so the
//! detail view stops trying to render a missing image. The user's
//! search behavior is unaffected — every searchable bit lives on
//! `memory.ocr_text`, which we don't touch.
//!
//! GC runs:
//!   * Once at startup, after the window opens (deferred so it never
//!     blocks first paint).
//!   * Every 24 hours thereafter.
//!
//! Both passes are best-effort: any IO error is logged and the next
//! pass picks up where this one left off.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager};
use tokio::fs;

use crate::db::repositories::{SharedMemoryRepository, SharedSettingsRepository};
use crate::errors::app_error::{AppError, AppResult};

/// v0.5.32: default days a screenshot file lives on disk before
/// the GC purges it. Used as the fallback when `ai_screenshot_retention_days`
/// is unset in settings. Power users can override this from
/// AI Settings → Screenshot retention. `0` disables the GC entirely
/// — handy for users who value memory completeness over disk usage.
pub const DEFAULT_RETENTION_DAYS: u64 = 60;

/// Subdirectory under `app_data_dir()` where screenshots live. Must
/// match `screenshot_store::SCREENSHOT_SUBDIR`.
const SCREENSHOT_SUBDIR: &str = "screenshots";

/// Filename prefix used by the screenshot store. We refuse to delete
/// files that don't match this prefix so an accidental misconfigured
/// directory can't cause unrelated files to get unlinked.
const SCREENSHOT_PREFIX: &str = "screenshot-";

/// Result summary from one GC pass — useful for logging and (later)
/// surfacing in the AI Settings tab.
#[derive(Debug, Clone, Default)]
pub struct RetentionSummary {
    pub scanned: u64,
    pub deleted: u64,
    pub freed_bytes: u64,
}

/// Run one GC pass over the screenshots directory. Deletes files
/// older than the configured retention window and clears
/// `memory.url` on rows that pointed at them.
///
/// v0.5.32: retention window is now per-user. Reads
/// `settings.ai_screenshot_retention_days` (default 60). A value of
/// `0` disables the pass entirely — power users who'd rather hold
/// disk than lose image previews flip the dropdown to "Never" in
/// AI Settings and the GC becomes a no-op every cycle.
pub async fn run_retention_gc(
    app: &AppHandle,
    memory_repo: &SharedMemoryRepository,
    settings_repo: &SharedSettingsRepository,
) -> AppResult<RetentionSummary> {
    let retention_days = match settings_repo.get().await {
        Ok(s) => s.ai_screenshot_retention_days as u64,
        Err(err) => {
            eprintln!(
                "[recall][screenshot-gc] settings read failed; defaulting to {DEFAULT_RETENTION_DAYS}d: {err}"
            );
            DEFAULT_RETENTION_DAYS
        }
    };
    if retention_days == 0 {
        // Power-user override — keep every screenshot forever. The
        // pass becomes a no-op each cycle until they flip back.
        return Ok(RetentionSummary::default());
    }

    let directory = screenshots_dir(app)?;
    if !fs::try_exists(&directory).await.unwrap_or(false) {
        return Ok(RetentionSummary::default());
    }

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days * 24 * 60 * 60))
        .ok_or_else(|| AppError::Invalid("retention cutoff overflowed".into()))?;

    let mut entries = fs::read_dir(&directory).await.map_err(|err| {
        AppError::Invalid(format!(
            "Failed to read screenshots directory {}: {err}",
            directory.display()
        ))
    })?;

    let mut summary = RetentionSummary::default();
    let mut purged_filenames: Vec<String> = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(|err| {
        AppError::Invalid(format!("Screenshot directory iteration failed: {err}"))
    })? {
        summary.scanned += 1;

        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with(SCREENSHOT_PREFIX) {
            // Defensive: never touch unrelated files even if they
            // somehow ended up in this directory.
            continue;
        }

        let metadata = match entry.metadata().await {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }

        let modified = match metadata.modified() {
            Ok(time) => time,
            Err(_) => continue,
        };
        if modified >= cutoff {
            continue;
        }

        let byte_size = metadata.len();
        let path = entry.path();
        match fs::remove_file(&path).await {
            Ok(()) => {
                summary.deleted += 1;
                summary.freed_bytes = summary.freed_bytes.saturating_add(byte_size);
                purged_filenames.push(name.to_string());
            }
            Err(err) => {
                eprintln!(
                    "[recall][screenshot-gc] failed to delete {}: {err}",
                    path.display()
                );
            }
        }
    }

    if !purged_filenames.is_empty() {
        // Clear `memory.url` on rows pointing at deleted files. The
        // OCR text stays, so the row keeps everything Recall actually
        // needs to recall the memory long-term.
        if let Err(error) = memory_repo
            .clear_url_for_purged_screenshots(&purged_filenames)
            .await
        {
            eprintln!(
                "[recall][screenshot-gc] failed to clear urls for {} purged files: {error}",
                purged_filenames.len()
            );
        }
    }

    Ok(summary)
}

fn screenshots_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::Invalid(format!("app_data_dir unavailable: {err}")))?;
    Ok(base.join(SCREENSHOT_SUBDIR))
}

/// Spawn the recurring GC loop. Runs once immediately so a freshly
/// upgraded install processes any backlog, then sleeps 24h between
/// passes. Errors are logged and don't stop the loop — we'd rather
/// retry tomorrow than die on a transient IO hiccup.
///
/// v0.5.32: takes a settings repo so each pass reads the live
/// `ai_screenshot_retention_days` value. Changes from the AI
/// Settings dropdown apply on the next pass — no restart needed.
pub fn start_retention_loop(
    app: AppHandle,
    memory_repo: SharedMemoryRepository,
    settings_repo: SharedSettingsRepository,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match run_retention_gc(&app, &memory_repo, &settings_repo).await {
                Ok(summary) => {
                    if summary.deleted > 0 {
                        eprintln!(
                            "[recall][screenshot-gc] purged {} screenshot files ({:.1} MB freed)",
                            summary.deleted,
                            (summary.freed_bytes as f64) / (1024.0 * 1024.0)
                        );
                    }
                }
                Err(error) => {
                    eprintln!("[recall][screenshot-gc] pass failed: {error}");
                }
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}
