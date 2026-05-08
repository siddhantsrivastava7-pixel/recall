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
        // v0.5.55 — switched from SQL `LIKE <root>\%` cascade to
        // Rust-side filtering with path normalization. The
        // previous version missed entries whose stored path
        // disagreed with the cascade root on UNC prefix, trailing
        // separator, or `\` vs `/`. User report: "I removed
        // Documents folder, but every folder inside Documents
        // didn't get removed automatically." Root cause was
        // `\\?\C:\Users\...` paths from canonicalize() not
        // matching the LIKE pattern built from the folder
        // shadow's external_id (which lacked the UNC prefix on
        // older ingests).
        //
        // We pull every row and filter in-process. Cost is one
        // full scan of `files` and `folders` per remove — fine
        // for the typical library size and worth the correctness.

        let normalized_root = normalize_for_match(&folder_path);

        // ---- child files ----
        let all_files: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, path, shadow_memory_id FROM files",
        )
        .fetch_all(&state.pool)
        .await?;

        let matched_files: Vec<(String, Option<String>)> = all_files
            .into_iter()
            .filter(|(_, path, _)| is_path_under(path, &normalized_root))
            .map(|(id, _, shadow)| (id, shadow))
            .collect();

        eprintln!(
            "[recall][remove-folder] cascade: {} file(s) matched under {folder_path}",
            matched_files.len()
        );

        for (file_id, shadow_id) in &matched_files {
            if let Some(shadow) = shadow_id {
                let _ = state.memory_service.delete(shadow).await;
            }
            let _ = sqlx::query("DELETE FROM files WHERE id = ?1")
                .bind(file_id)
                .execute(&state.pool)
                .await;
        }

        // ---- child folders ----
        let all_folders: Vec<(String, String)> =
            sqlx::query_as("SELECT id, path FROM folders")
                .fetch_all(&state.pool)
                .await?;

        let matched_folders: Vec<(String, String)> = all_folders
            .into_iter()
            .filter(|(_, path)| is_path_under(path, &normalized_root))
            .collect();

        eprintln!(
            "[recall][remove-folder] cascade: {} folder(s) matched under {folder_path}",
            matched_folders.len()
        );

        for (folder_id, path) in &matched_folders {
            // Folder shadows are keyed on external_id == path. Try
            // both the as-stored path and a few normalization
            // variants so older un-canonicalized shadows still
            // resolve.
            for lookup in path_lookup_variants(path) {
                if let Some(shadow) = state
                    .memory_repository
                    .find_by_external_source("folder", &lookup)
                    .await?
                {
                    let _ = state.memory_service.delete(&shadow.id).await;
                    break;
                }
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

/// v0.5.55 — strip Windows UNC verbatim prefix + trailing
/// separators so two strings that name the same logical
/// directory compare equal. Examples that all normalize to
/// `C:\Users\siddh\Documents`:
///
/// * `\\?\C:\Users\siddh\Documents`
/// * `C:\Users\siddh\Documents\`
/// * `C:\Users\siddh\Documents/`
/// * `\\?\C:\Users\siddh\Documents\`
///
/// Case sensitivity is preserved on purpose — Recall paths are
/// stored case-as-typed, and Windows is case-insensitive at the
/// filesystem layer but case-preserving at the API. Lowercasing
/// would turn the "is_under" check into a probabilistic match.
fn normalize_for_match(path: &str) -> String {
    let trimmed = path.trim_end_matches('/').trim_end_matches('\\');
    // Windows UNC verbatim prefix: \\?\
    if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    trimmed.to_string()
}

/// True when `candidate` is the same logical path as
/// `normalized_root` or a descendant of it. Both forward and
/// back slashes accepted as separators on either side.
fn is_path_under(candidate: &str, normalized_root: &str) -> bool {
    let normalized_candidate = normalize_for_match(candidate);
    if normalized_candidate == normalized_root {
        return true;
    }
    // Prefix must end with a separator so `Documents` doesn't
    // match `Documents2`. Both separator flavors are checked so
    // a stored Unix-style path under a Windows root (or vice
    // versa) still matches.
    normalized_candidate.starts_with(&format!("{normalized_root}/"))
        || normalized_candidate.starts_with(&format!("{normalized_root}\\"))
}

/// v0.5.55 — set of path strings to try when looking up a
/// folder's shadow memory by external_id. Folder shadows from
/// different code paths over the project's lifetime have been
/// stored with various levels of canonicalization; trying a few
/// variants catches all of them.
fn path_lookup_variants(path: &str) -> Vec<String> {
    let mut variants = vec![path.to_string()];
    let normalized = normalize_for_match(path);
    if normalized != path {
        variants.push(normalized.clone());
    }
    let unc = format!(r"\\?\{normalized}");
    if !variants.contains(&unc) {
        variants.push(unc);
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_unc_prefix() {
        assert_eq!(
            normalize_for_match(r"\\?\C:\Users\siddh\Documents"),
            r"C:\Users\siddh\Documents"
        );
    }

    #[test]
    fn normalize_strips_trailing_separator() {
        assert_eq!(
            normalize_for_match(r"C:\Users\siddh\Documents\"),
            r"C:\Users\siddh\Documents"
        );
        assert_eq!(
            normalize_for_match("/Users/siddh/Documents/"),
            "/Users/siddh/Documents"
        );
    }

    #[test]
    fn normalize_handles_unc_with_trailing_slash() {
        assert_eq!(
            normalize_for_match(r"\\?\C:\Users\siddh\Documents\"),
            r"C:\Users\siddh\Documents"
        );
    }

    #[test]
    fn is_under_matches_unc_against_dos() {
        // The exact case the user hit: Documents canonicalizes to
        // \\?\C:\... and a child folder was stored as plain C:\...
        assert!(is_path_under(
            r"C:\Users\siddh\Documents\Foo",
            r"C:\Users\siddh\Documents"
        ));
        assert!(is_path_under(
            r"\\?\C:\Users\siddh\Documents\Foo",
            r"C:\Users\siddh\Documents"
        ));
    }

    #[test]
    fn is_under_does_not_match_sibling_with_shared_prefix() {
        // "Documents" must not match "Documents2" — separator
        // boundary is what distinguishes ancestor from sibling.
        assert!(!is_path_under(
            r"C:\Users\siddh\Documents2\Foo",
            r"C:\Users\siddh\Documents"
        ));
    }

    #[test]
    fn is_under_self_match() {
        assert!(is_path_under(
            r"C:\Users\siddh\Documents",
            r"C:\Users\siddh\Documents"
        ));
    }
}
