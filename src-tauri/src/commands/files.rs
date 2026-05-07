//! v0.5.38 — Tauri commands for file & folder ingestion.
//!
//! Two entry points today:
//!
//!   * `ingest_path` — drag-drop or file-picker hands us an
//!     absolute path. We branch on file vs directory inside
//!     [`file_ingestion_service::ingest_path`].
//!   * `suggested_locations` — read-only helper that returns the
//!     canonical "common starting points" (Desktop, Downloads,
//!     Documents) the Settings UI shows as opt-in checkboxes.
//!
//! Watched-folder list management lands in v0.5.39 alongside the
//! actual filesystem watcher. v0.5.38 is one-shot ingest only.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    errors::app_error::{AppError, AppResult},
    services::file_ingestion_service,
    state::app_state::AppState,
};

/// v0.5.48 — auto-watch any ingested folder so changes flow into
/// Recall without the user having to re-drop. Best-effort: a
/// failed watch registration logs and continues, never blocking
/// the ingest result the user is waiting on.
async fn auto_watch_folder(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: &std::path::Path,
) {
    if !path.is_dir() {
        return;
    }
    if let Err(error) = state
        .file_watcher_service
        .add_watch(app, &state.pool, path)
        .await
    {
        eprintln!(
            "[recall][file-watcher] auto-watch failed for {}: {error}",
            path.display()
        );
    }
}

/// One-shot ingest. Path can be a file or a directory.
/// Settings (caps, hidden-folder skip) read fresh inside the
/// service so UI changes apply on the next call without restart.
#[tauri::command]
pub async fn ingest_path(
    app: AppHandle,
    path: String,
    state: State<'_, AppState>,
) -> AppResult<file_ingestion_service::IngestResult> {
    let path_buf = std::path::PathBuf::from(&path);
    let settings = state.settings_repository.get().await?;
    let result = file_ingestion_service::ingest_path(
        &state.pool,
        &state.memory_repository,
        &state.memory_service,
        &settings,
        &path_buf,
    )
    .await?;
    // v0.5.48: auto-watch directories so future changes flow in
    // without the user having to re-drag the folder. Single files
    // are skipped here — there's no parent-of-our-choice to watch.
    auto_watch_folder(&app, &state, &path_buf).await;
    Ok(result)
}

/// Multi-path variant — drag-drop typically yields a Vec of
/// paths even for a single file. Caller can either iterate
/// `ingest_path` or call this once for the whole batch.
/// Aggregates counts across each path.
#[tauri::command]
pub async fn ingest_paths(
    app: AppHandle,
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<file_ingestion_service::IngestResult> {
    let settings = state.settings_repository.get().await?;
    let mut combined = file_ingestion_service::IngestResult::default();
    for path in paths {
        let path_buf = std::path::PathBuf::from(&path);
        match file_ingestion_service::ingest_path(
            &state.pool,
            &state.memory_repository,
            &state.memory_service,
            &settings,
            &path_buf,
        )
        .await
        {
            Ok(result) => {
                // v0.5.48: same auto-watch behavior as the single-
                // path command — folders flowing through batch
                // drops also get watched.
                auto_watch_folder(&app, &state, &path_buf).await;
                combined.files_seen += result.files_seen;
                combined.files_imported += result.files_imported;
                combined.files_skipped_size += result.files_skipped_size;
                combined.files_skipped_hidden += result.files_skipped_hidden;
                combined.files_skipped_error += result.files_skipped_error;
                combined.folders_imported += result.folders_imported;
                combined.stopped_at_count_cap |= result.stopped_at_count_cap;
                combined.stopped_at_depth_cap |= result.stopped_at_depth_cap;
            }
            Err(error) => {
                eprintln!("[recall][file-ingest] path failed: {error}");
                combined.files_skipped_error += 1;
            }
        }
    }
    combined.message = describe_combined(&combined);
    Ok(combined)
}

fn describe_combined(result: &file_ingestion_service::IngestResult) -> String {
    if result.files_imported == 0 && result.folders_imported == 0 {
        return "Nothing imported.".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    if result.files_imported > 0 {
        parts.push(format!(
            "{} file{}",
            result.files_imported,
            if result.files_imported == 1 { "" } else { "s" }
        ));
    }
    if result.folders_imported > 0 {
        parts.push(format!(
            "{} folder{}",
            result.folders_imported,
            if result.folders_imported == 1 { "" } else { "s" }
        ));
    }
    let mut msg = format!("Imported {}", parts.join(" + "));
    if result.files_skipped_size + result.files_skipped_error > 0 {
        msg.push_str(&format!(
            " · skipped {}",
            result.files_skipped_size + result.files_skipped_error
        ));
    }
    if result.stopped_at_count_cap {
        msg.push_str(" · hit file-count cap");
    }
    msg
}

/// Common starting points the Settings UI shows as opt-in
/// checkboxes. Returns absolute paths only when the directory
/// actually exists on this host (avoids surfacing broken
/// suggestions on machines without a Desktop folder, etc.).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedLocation {
    /// User-facing label ("Desktop", "Downloads", "Documents").
    pub label: String,
    pub path: String,
    /// Best-effort file count (1-level deep) so the UI can
    /// surface "~143 files" alongside the checkbox without
    /// committing to a full walk.
    pub approx_file_count: u32,
}

#[tauri::command]
pub async fn suggested_locations(
    app: AppHandle,
) -> AppResult<Vec<SuggestedLocation>> {
    let mut suggestions: Vec<SuggestedLocation> = Vec::new();

    // Tauri's path resolver gives us the right per-OS paths.
    if let Ok(p) = app.path().desktop_dir() {
        if p.exists() {
            suggestions.push(SuggestedLocation {
                label: "Desktop".to_string(),
                approx_file_count: shallow_file_count(&p),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    if let Ok(p) = app.path().download_dir() {
        if p.exists() {
            suggestions.push(SuggestedLocation {
                label: "Downloads".to_string(),
                approx_file_count: shallow_file_count(&p),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    if let Ok(p) = app.path().document_dir() {
        if p.exists() {
            suggestions.push(SuggestedLocation {
                label: "Documents".to_string(),
                approx_file_count: shallow_file_count(&p),
                path: p.to_string_lossy().to_string(),
            });
        }
    }

    Ok(suggestions)
}

/// Cheap one-level file count. Caps at a small ceiling so we
/// don't accidentally count 50,000 files in a giant Documents
/// folder just to render "Documents · 9999+".
fn shallow_file_count(path: &std::path::Path) -> u32 {
    let mut count: u32 = 0;
    if let Ok(read) = std::fs::read_dir(path) {
        for entry in read.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                count = count.saturating_add(1);
                if count >= 9999 {
                    break;
                }
            }
        }
    }
    count
}

/// v0.5.48 — manually add a folder to the watch list. Useful
/// when the user wants to watch a folder they haven't ingested
/// (rare today; common once the v0.5.49 management UI lands).
/// Idempotent — re-adding already-watched paths is a no-op.
#[tauri::command]
pub async fn add_watched_folder(
    app: AppHandle,
    path: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        return Err(AppError::Invalid(format!(
            "Path does not exist: {}",
            path_buf.display()
        )));
    }
    if !path_buf.is_dir() {
        return Err(AppError::Invalid(format!(
            "Watch target must be a directory: {}",
            path_buf.display()
        )));
    }
    state
        .file_watcher_service
        .add_watch(&app, &state.pool, &path_buf)
        .await
}

/// v0.5.48 — stop watching a folder. Existing shadow memories
/// for files already inside the folder stay (the user kept
/// what they ingested), we just stop pulling new changes.
#[tauri::command]
pub async fn remove_watched_folder(
    path: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let path_buf = std::path::PathBuf::from(&path);
    state
        .file_watcher_service
        .remove_watch(&state.pool, &path_buf)
        .await
}

/// v0.5.48 — list the absolute paths of every currently-watched
/// folder. Source of truth is the `watched_folders` SQLite
/// table, not the in-memory map (so the UI sees the same view
/// the post-restart watchers will).
#[tauri::command]
pub async fn list_watched_folders(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.file_watcher_service.list_watched(&state.pool).await
}

/// Wired up so the Tauri command surface compiles even before
/// the watched-folder feature lands. Used only for typed
/// imports in lib.rs today.
#[allow(dead_code)]
pub fn _placeholder() -> AppResult<()> {
    let _: Result<(), AppError> = Ok(());
    Ok(())
}
