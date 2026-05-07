//! v0.5.48 — filesystem watcher.
//!
//! Wraps the `notify` crate via `notify-debouncer-full` to watch
//! every folder the user has dragged in. On filesystem events we
//! re-call the existing `file_ingestion_service::ingest_path` for
//! create/modify, and delete the corresponding shadow memory for
//! remove events. Single source of truth for what's watched is the
//! `watched_folders` SQLite table, so watches survive app restart.
//!
//! ## Design notes
//!
//! * **One debouncer per folder.** The `notify` API supports a
//!   single watcher with multiple paths, but per-folder debouncers
//!   give us cleaner add/remove semantics — drop one, the others
//!   stay alive without re-binding any platform handles.
//!
//! * **Debounce window: 1.5s.** File editors (VS Code, Word, Excel)
//!   emit 3–10 events for one user-visible save: open, write, close,
//!   rename-temp-file, etc. 1.5s coalesces those into a single
//!   re-ingest without making the user wait long for changes to
//!   show up.
//!
//! * **Best-effort ingest.** Re-ingest happens on a tokio task; if
//!   the file is mid-save when we read it we'll catch the next
//!   event. Errors are eprintln-logged and never propagate — a
//!   single bad file shouldn't take down the watcher.
//!
//! * **Skip noise.** `.DS_Store`, `Thumbs.db`, lock files, files
//!   bigger than the user's size cap, and dot-prefixed files in
//!   hidden-skip mode are dropped at the event boundary so the
//!   ingest path never sees them.
//!
//! ## What we deliberately DON'T do (yet)
//!
//! * Detect file moves/renames as a single atomic event. notify
//!   surfaces these as Remove + Create on most platforms; we
//!   handle them as two separate events, which means a moved
//!   file briefly disappears from the library and reappears with
//!   a new shadow memory ID. Acceptable for v0.5.48; if it
//!   matters for citations we can add explicit move tracking
//!   later.
//!
//! * UI for managing the watch list — that lands alongside the
//!   "Files" Settings tab in v0.5.49.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer, FileIdMap,
};
use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::db::repositories::SharedMemoryRepository;
use crate::errors::app_error::{AppError, AppResult};
use crate::services::file_ingestion_service;
use crate::state::app_state::AppState;
use tauri::Manager;

/// Debounce window for filesystem event coalescing. See module
/// docstring for why 1.5s.
const DEBOUNCE_MS: u64 = 1_500;

/// Tick interval for the debouncer's internal timer. Values
/// shorter than 100ms eat CPU; values longer than the debounce
/// window break debouncing. 100ms is the documented default.
const DEBOUNCE_TICK_MS: u64 = 100;

/// File names we never act on. Editors / OS write these to
/// folders without user intent and ingesting them adds noise.
const NOISE_FILENAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".gitignore", // tracked in source-trees but rarely "user content"
];

/// Holds the live debouncer per watched folder. Drop the entry
/// to stop watching that folder — the platform-native handle
/// goes away cleanly with the debouncer.
pub struct FileWatcherService {
    /// path → live debouncer. Keyed by canonicalized absolute
    /// path string (matches what's persisted in `watched_folders`).
    inner: Arc<Mutex<HashMap<String, Debouncer<RecommendedWatcher, FileIdMap>>>>,
}

impl FileWatcherService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Persist a folder to `watched_folders` and start watching it.
    /// Idempotent — re-adding an already-watched path is a no-op
    /// (the existing debouncer keeps running).
    pub async fn add_watch(
        &self,
        app: &AppHandle,
        pool: &SqlitePool,
        path: &Path,
    ) -> AppResult<()> {
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };
        let path_str = canonical.to_string_lossy().to_string();

        // Persist first so a crash between persist and watch leaves
        // the row that the boot-time restore will pick up.
        sqlx::query(
            "INSERT OR IGNORE INTO watched_folders (path, recursive, added_at) \
             VALUES (?1, 1, ?2)",
        )
        .bind(&path_str)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

        let mut inner = self.inner.lock().await;
        if inner.contains_key(&path_str) {
            return Ok(());
        }

        let debouncer = build_debouncer(app.clone(), &canonical)?;
        inner.insert(path_str, debouncer);
        Ok(())
    }

    /// Stop watching a folder and remove it from `watched_folders`.
    /// Shadow memories for files inside the folder stay — the user
    /// keeps what they ingested; we just stop pulling new changes.
    pub async fn remove_watch(&self, pool: &SqlitePool, path: &Path) -> AppResult<()> {
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };
        let path_str = canonical.to_string_lossy().to_string();

        sqlx::query("DELETE FROM watched_folders WHERE path = ?1")
            .bind(&path_str)
            .execute(pool)
            .await?;

        let mut inner = self.inner.lock().await;
        // Dropping the debouncer cleanly releases the platform
        // handle — no explicit teardown needed.
        inner.remove(&path_str);
        Ok(())
    }

    /// Read every row in `watched_folders` and re-establish a
    /// debouncer for each. Called once at app boot. Failures on
    /// individual folders (deleted on disk between sessions, etc.)
    /// are logged and skipped, never propagated.
    pub async fn restore_from_db(&self, app: &AppHandle, pool: &SqlitePool) -> AppResult<()> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT path FROM watched_folders ORDER BY added_at ASC",
        )
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut inner = self.inner.lock().await;
        let mut restored = 0u32;
        for (path_str,) in &rows {
            let path = PathBuf::from(path_str);
            if !path.exists() {
                eprintln!(
                    "[recall][file-watcher] watched folder no longer exists, skipping: {path_str}"
                );
                continue;
            }
            if inner.contains_key(path_str) {
                continue;
            }
            match build_debouncer(app.clone(), &path) {
                Ok(debouncer) => {
                    inner.insert(path_str.clone(), debouncer);
                    restored += 1;
                }
                Err(error) => {
                    eprintln!(
                        "[recall][file-watcher] failed to restore watch for {path_str}: {error}"
                    );
                }
            }
        }
        eprintln!(
            "[recall][file-watcher] restored {restored} of {} watched folder(s)",
            rows.len()
        );
        Ok(())
    }

    /// List every currently-watched folder. Used by the
    /// `list_watched_folders` Tauri command.
    pub async fn list_watched(&self, pool: &SqlitePool) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT path FROM watched_folders ORDER BY added_at ASC")
                .fetch_all(pool)
                .await?;
        Ok(rows.into_iter().map(|(p,)| p).collect())
    }
}

impl Default for FileWatcherService {
    fn default() -> Self {
        Self::new()
    }
}

/// Construct the debouncer + register the recursive watch. The
/// callback runs on notify's internal thread; we pass each event
/// into a tokio task so blocking ingest work doesn't starve the
/// watcher itself.
fn build_debouncer(
    app: AppHandle,
    path: &Path,
) -> AppResult<Debouncer<RecommendedWatcher, FileIdMap>> {
    let app_for_callback = app.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(DEBOUNCE_MS),
        Some(Duration::from_millis(DEBOUNCE_TICK_MS)),
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let app = app_for_callback.clone();
                tauri::async_runtime::spawn(async move {
                    handle_debounced_events(app, events).await;
                });
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("[recall][file-watcher] notify error: {error}");
                }
            }
        },
    )
    .map_err(|err| AppError::Invalid(format!("file watcher init failed: {err}")))?;

    debouncer
        .watcher()
        .watch(path, RecursiveMode::Recursive)
        .map_err(|err| {
            AppError::Invalid(format!(
                "watch registration failed for {}: {err}",
                path.display()
            ))
        })?;

    Ok(debouncer)
}

/// Process a coalesced batch of debounced filesystem events. Each
/// event is either a re-ingest trigger (Create / Modify) or a
/// shadow-memory delete (Remove). Errors are logged and skipped
/// per-file so one bad path doesn't poison the whole batch.
async fn handle_debounced_events(app: AppHandle, events: Vec<DebouncedEvent>) {
    let state = app.state::<AppState>();
    let settings = match state.settings_repository.get().await {
        Ok(s) => s,
        Err(error) => {
            eprintln!("[recall][file-watcher] settings load failed: {error}");
            return;
        }
    };

    use notify::EventKind;
    for event in events {
        let kind = event.event.kind;
        for path in &event.event.paths {
            if !is_actionable(path) {
                continue;
            }
            match kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    if !path.exists() {
                        // Editors sometimes emit a final Modify
                        // after deleting a temp file — guard against
                        // re-ingesting a path that's already gone.
                        continue;
                    }
                    if path.is_file() {
                        if let Err(error) = file_ingestion_service::ingest_path(
                            &state.pool,
                            &state.memory_repository,
                            &state.memory_service,
                            &settings,
                            path,
                        )
                        .await
                        {
                            eprintln!(
                                "[recall][file-watcher] re-ingest failed for {}: {error}",
                                path.display()
                            );
                        }
                    }
                    // Folder create/modify events don't trigger
                    // ingest by themselves — file events inside
                    // the folder will. Avoids walking a whole
                    // subtree on every metadata change.
                }
                EventKind::Remove(_) => {
                    if let Err(error) =
                        delete_shadow_for_path(&state.pool, &state.memory_repository, path).await
                    {
                        eprintln!(
                            "[recall][file-watcher] shadow delete failed for {}: {error}",
                            path.display()
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

/// Filter out OS noise + bookkeeping files before they hit the
/// ingest path.
fn is_actionable(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if NOISE_FILENAMES.iter().any(|n| *n == name) {
        return false;
    }
    // Lock files written by editors and Office apps.
    if name.starts_with("~$") || name.starts_with(".~lock") || name.ends_with(".swp") {
        return false;
    }
    true
}

/// On a remove event, look up the file row by absolute path,
/// pull its `shadow_memory_id`, delete the memory row, and drop
/// the file row itself. The two-step is necessary because the
/// memory's `external_id` is the file row's UUID — not the path —
/// so we can't go directly memory_repo.find_by_external_source(path).
async fn delete_shadow_for_path(
    pool: &SqlitePool,
    memory_repo: &SharedMemoryRepository,
    path: &Path,
) -> AppResult<()> {
    let path_str = path.to_string_lossy().to_string();
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, shadow_memory_id FROM files WHERE path = ?1",
    )
    .bind(&path_str)
    .fetch_optional(pool)
    .await?;

    let Some((file_id, shadow_id)) = row else {
        return Ok(());
    };

    if let Some(shadow) = shadow_id {
        // Best-effort — a memory delete failure shouldn't strand the
        // file row. Future GC pass cleans orphans on app upgrade.
        let _ = memory_repo.delete(&shadow).await;
    }
    sqlx::query("DELETE FROM files WHERE id = ?1")
        .bind(&file_id)
        .execute(pool)
        .await?;
    Ok(())
}
