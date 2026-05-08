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

/// v0.5.50 — fully remove a file from Recall. Drops the shadow
/// memory + the `files` row, leaving the actual file on disk
/// untouched. The file remains visible in the parent folder's
/// watcher; if the user later edits it, the watcher's Modify
/// event re-ingests it. That's the right behavior — explicit
/// removal "for good" is what `remove_folder` does (stops
/// watching), and a user who keeps editing a removed file
/// probably wants it back. If a "stay removed" semantic ever
/// becomes important, a `dismissed_files` table is the natural
/// next step.
#[tauri::command]
pub async fn remove_file(
    memory_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    // Locate the file row via the shadow_memory_id pointer. Some
    // older shadows (pre-v0.5.47) may not have the link populated
    // — fall back to looking up the memory's external_id (the
    // file row's UUID) and using that.
    let file_row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, path FROM files WHERE shadow_memory_id = ?1",
    )
    .bind(&memory_id)
    .fetch_optional(&state.pool)
    .await?;

    let (file_id, _file_path) = match file_row {
        Some(row) => row,
        None => {
            // Fall back: look up via the memory's external_id.
            let memory = state.memory_repository.find(&memory_id).await?;
            let external_id = memory.and_then(|m| m.external_id);
            match external_id {
                Some(ext) => {
                    let alt: Option<(String, String)> = sqlx::query_as(
                        "SELECT id, path FROM files WHERE id = ?1",
                    )
                    .bind(&ext)
                    .fetch_optional(&state.pool)
                    .await?;
                    match alt {
                        Some(row) => row,
                        None => {
                            // No file row to clean up — just delete the memory and return.
                            return state.memory_service.delete(&memory_id).await;
                        }
                    }
                }
                None => {
                    return state.memory_service.delete(&memory_id).await;
                }
            }
        }
    };

    // Drop file row first so the memory delete doesn't leave a
    // dangling shadow_memory_id pointer if the second step fails.
    sqlx::query("DELETE FROM files WHERE id = ?1")
        .bind(&file_id)
        .execute(&state.pool)
        .await?;
    state.memory_service.delete(&memory_id).await?;
    Ok(())
}

/// v0.5.50 — fully remove a folder from Recall. Stops the
/// filesystem watcher, drops `watched_folders` + `folders`
/// rows, and (when `keep_children` is false) cascades through
/// every `files` row + shadow memory under the folder's path.
/// The folder + its files on disk stay where they are.
///
/// "Cascade by path prefix" is intentional: we use the
/// canonicalized folder path as the prefix and match `files.path`
/// against `<folder>/...`. This catches every descendant
/// regardless of folder depth without needing recursive parent
/// traversal.
///
/// v0.5.51 — `keep_children` lets the user remove just the
/// folder + watcher while keeping every file memory the folder
/// produced. Useful when the user added a folder for one-shot
/// indexing and wants to stop the watcher but doesn't want to
/// re-find dozens of file memories. When true: stop the
/// watcher, drop watched_folders, drop folder shadow + folder
/// row only. Subfolder rows + every file under the path are
/// left intact (each becomes a free-standing file memory).
#[tauri::command]
pub async fn remove_folder(
    memory_id: String,
    keep_children: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    // Look up the folder row via the memory's external_id. For
    // folder shadows, external_id is the absolute path string
    // (per file_ingestion_service::upsert_folder_row).
    let memory = state.memory_repository.find(&memory_id).await?;
    let folder_path = match memory.and_then(|m| m.external_id) {
        Some(p) => p,
        None => {
            // No folder mapping — just drop the memory and return.
            return state.memory_service.delete(&memory_id).await;
        }
    };

    // Stop the watcher first so no in-flight events trigger a
    // re-ingest of files we're about to delete.
    let path_buf = std::path::PathBuf::from(&folder_path);
    if let Err(error) = state
        .file_watcher_service
        .remove_watch(&state.pool, &path_buf)
        .await
    {
        eprintln!(
            "[recall][file-remove] watcher stop failed for {folder_path}: {error}"
        );
    }

    if !keep_children {
        // Cascade through child files. Match either the exact folder
        // path (covers the rare "file row whose path is the folder
        // itself" case) or anything under it. The trailing slash
        // patterns differ per OS so we handle both forward + back
        // slash separators.
        let prefix_fwd = format!("{}/%", folder_path.trim_end_matches('/').trim_end_matches('\\'));
        let prefix_bwd = format!("{}\\%", folder_path.trim_end_matches('/').trim_end_matches('\\'));

        let child_files: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, shadow_memory_id FROM files \
             WHERE path = ?1 OR path LIKE ?2 OR path LIKE ?3",
        )
        .bind(&folder_path)
        .bind(&prefix_fwd)
        .bind(&prefix_bwd)
        .fetch_all(&state.pool)
        .await?;

        // Drop child shadow memories first, then file rows. A failed
        // delete partway through leaves the row referencing a now-
        // missing shadow — better than leaving a shadow without a row
        // (the latter looks like a normal memory but its source is
        // gone).
        for (file_id, shadow_id) in &child_files {
            if let Some(shadow) = shadow_id {
                let _ = state.memory_service.delete(shadow).await;
            }
            let _ = sqlx::query("DELETE FROM files WHERE id = ?1")
                .bind(file_id)
                .execute(&state.pool)
                .await;
        }

        // Cascade through subfolder rows + their shadows.
        let child_folders: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, path FROM folders \
             WHERE path = ?1 OR path LIKE ?2 OR path LIKE ?3",
        )
        .bind(&folder_path)
        .bind(&prefix_fwd)
        .bind(&prefix_bwd)
        .fetch_all(&state.pool)
        .await?;

        for (folder_id, path) in &child_folders {
            if let Some(shadow) = state
                .memory_repository
                .find_by_external_source("folder", path)
                .await?
            {
                let _ = state.memory_service.delete(&shadow.id).await;
            }
            let _ = sqlx::query("DELETE FROM folders WHERE id = ?1")
                .bind(folder_id)
                .execute(&state.pool)
                .await;
        }
    } else {
        // keep_children path — drop only the folder row + the
        // shadow the user clicked Delete on. File memories
        // become free-standing (their `folder_path` field still
        // holds the parent dir, which is fine for navigation but
        // no longer corresponds to a folder shadow).
        let _ = sqlx::query("DELETE FROM folders WHERE path = ?1")
            .bind(&folder_path)
            .execute(&state.pool)
            .await;
    }

    // The user-facing folder shadow itself. In the cascade path
    // it was already deleted via the loop when path == folder_path;
    // in the keep-children path it's still around. Safe to call
    // either way — the delete is a no-op if the row's already
    // gone.
    let _ = state.memory_service.delete(&memory_id).await;

    Ok(())
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
