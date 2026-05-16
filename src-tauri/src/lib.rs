mod ai;
mod commands;
mod db;
mod errors;
mod models;
mod platform;
mod services;
mod state;

use commands::{
    ai::{
        ai_clear_failed_ocr, ai_diagnose_clipboard_image, ai_diagnose_llm,
        ai_download_embedding_model, ai_download_llm, ai_force_scrub,
        ai_llm_status, ai_recent_failures, ai_set_enabled, ai_set_mode,
        ai_status, ai_unload_llm, ask_recall, ask_recall_cancel,
        refresh_recap_memory,
        ask_recall_delete_session, ask_recall_get_session, ask_recall_list_sessions,
        ask_recall_new_session, ask_recall_rename_session, embed_all_memories,
        build_memory_trail,
        find_related, generate_daily_recap_summary, list_entities_for_memory,
        list_memories_by_entity, ocr_rebuild_index, ocr_run_for_memory,
        proactive_surface_dismiss, proactive_surface_get_current, save_qa_as_memory,
        semantic_search,
    },
    app::{bootstrap_app, get_runtime_info},
    bookmarks::{
        import_bookmarks, list_bookmark_sources, sync_bookmarks_now, x_connection_status,
        x_oauth_disconnect, x_oauth_start, x_sync_bookmarks_now,
    },
    files::{
        add_watched_folder, ingest_path, ingest_paths, list_watched_folders,
        remove_file, remove_folder, remove_watched_folder, suggested_locations,
    },
    license::{activate_license, deactivate_license, get_license_state, validate_license_key},
    memories::{
        create_memory, delete_memory, dismiss_memory_resurface, duplicate_memory, get_memory,
        list_memories, mark_memory_opened, set_memory_resurface, update_memory,
    },
    pairing::{get_pairing_info, reset_pairing},
    pointer::pointer_take_selection,
    platform::{detect_app_context, read_clipboard_text, write_clipboard_text},
    projects::{create_project, delete_project, list_projects, update_project},
    settings::{
        clear_all_data, export_data, get_settings, import_data, list_shortcuts, seed_sample_data,
        update_settings, update_shortcuts,
    },
    windows::{
        close_current_window, open_main_window, open_memory_in_main, open_quick_save_window,
        open_search_overlay, save_widget_position, set_widget_expanded,
    },
};
use db::seed::ensure_seed_data;
use platform::factory::create_platform_services;
use services::{
    clipboard_watcher_service::start_clipboard_watcher,
    screenshot_retention::start_retention_loop,
    screenshot_store::ScreenshotStore,
    shortcut_service::normalize_accelerator,
};
use state::app_state::AppState;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{
    Builder as GlobalShortcutBuilder, GlobalShortcutExt, ShortcutState,
};
use tokio::time::{sleep, Duration};

async fn current_shortcut_bindings(state: &AppState) -> Vec<crate::models::ShortcutBinding> {
    state
        .shortcut_service
        .list(&state.platform.shortcuts.bindings())
        .await
        .unwrap_or_else(|_| state.platform.shortcuts.bindings())
}

/// v0.5.64 — order-independent accelerator comparison.
///
/// `normalize_accelerator` joins tokens in *input order*. The
/// global-shortcut plugin, when it reports a keypress, emits the
/// modifier tokens in its own internal order (commonly
/// `shift+control+…`, not the human "Ctrl+Shift+…" we store).
/// String-equality on the joined form therefore fails for any
/// shortcut with ≥2 modifiers — `"Shift+Ctrl+P" != "Ctrl+Shift+P"`
/// — and the handler silently does nothing. Single-modifier
/// shortcuts (Alt+Space) round-trip fine, which is why this went
/// unnoticed until the first multi-modifier global hotkey
/// (Recall Pointer's Ctrl+Shift+P) was actually exercised.
///
/// Compare as sorted token *sets* instead: split, normalize each
/// part, sort, equate. Modifier order stops mattering; the
/// non-modifier key still has to match exactly.
fn accelerators_equivalent(a: &str, b: &str) -> bool {
    fn tokens(value: &str) -> Vec<String> {
        let mut parts: Vec<String> = normalize_accelerator(value)
            .split('+')
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect();
        parts.sort();
        parts
    }
    !a.trim().is_empty() && tokens(a) == tokens(b)
}

async fn shortcut_action_for_accelerator(state: &AppState, accelerator: &str) -> Option<String> {
    let bindings = state
        .shortcut_service
        .list(&state.platform.shortcuts.bindings())
        .await
        .ok()?;

    // Diagnostic: Windows GUI builds have no visible stderr, but
    // running the .exe from a terminal surfaces this — invaluable
    // for confirming the round-trip when a user reports a dead
    // hotkey.
    let resolved = bindings
        .iter()
        .find(|binding| accelerators_equivalent(&binding.accelerator, accelerator))
        .map(|binding| binding.action.clone());
    eprintln!(
        "[recall][shortcuts] keypress accelerator={accelerator:?} → action={resolved:?}"
    );
    resolved
}

pub(crate) async fn apply_shortcut_bindings(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcuts: &[crate::models::ShortcutBinding],
) -> crate::errors::app_error::AppResult<()> {
    let shortcut_manager = app.global_shortcut();

    let _ = shortcut_manager.unregister_all();

    // v0.5.62 — per-shortcut best-effort registration.
    //
    // Pre-v0.5.62 this was all-or-nothing: a single accelerator
    // that the OS refused (because another running app already
    // holds a system-wide registration for it — Ctrl+Shift+P is
    // a common collision) aborted the *entire* batch and rolled
    // back to the previous set. Net effect: one conflicting
    // shortcut silently disabled a working feature (Recall
    // Pointer's default Ctrl+Shift+P being the motivating bug
    // report). That's the wrong failure mode — a shortcut the OS
    // won't grant should degrade to "that one shortcut doesn't
    // work," not "Recall's shortcuts are broken."
    //
    // Now: register each independently. Collect the failures,
    // log them (no file logging on Windows GUI builds, so
    // eprintln is the diagnostic channel), and return Ok so the
    // shortcuts that *did* register stay live. A future change
    // can surface failed accelerators to the Settings →
    // Shortcuts UI so the user knows exactly which to rebind.
    let mut failed: Vec<(String, String)> = Vec::new();
    for binding in shortcuts {
        if let Err(error) = shortcut_manager.register(binding.accelerator.as_str()) {
            failed.push((binding.accelerator.clone(), error.to_string()));
        }
    }

    if !failed.is_empty() {
        for (accelerator, error) in &failed {
            eprintln!(
                "[recall][shortcuts] `{accelerator}` could not be registered \
                 (likely claimed by another app): {error} — other shortcuts \
                 still active; rebind this one in Settings → Shortcuts."
            );
        }
    }

    Ok(())
}

fn start_bookmark_sync_loop(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let delay_minutes = {
                let state = app.state::<AppState>();
                match state.settings_service.get().await {
                    Ok(settings) => settings.bookmark_sync_interval_minutes.max(5),
                    Err(_) => 15,
                }
            };
            sleep(Duration::from_secs((delay_minutes as u64) * 60)).await;
            let state = app.state::<AppState>();
            let should_sync = match state.settings_service.get().await {
                Ok(settings) => settings.bookmark_auto_sync_enabled,
                Err(_) => false,
            };
            if !should_sync {
                continue;
            }
            if let Ok(summary) = state
                .bookmark_service
                .sync_selected_browsers(app.clone())
                .await
            {
                let _ = app.emit("recall://bookmarks-synced", &summary);
            }
        }
    });
}

/// v0.5.7 one-shot backfill. Supersedes v0.5.6's first-launch pass.
/// Three jobs per memory:
///   * `replace_auto_tagger_tags` to scrub stale auto-tagger tags
///     (e.g. `license-key` falsely applied by v0.5.5's looser
///     regex to URLs containing UUID-shaped segments). v0.5.6's
///     backfill used `merge_topic_labels` which never removes,
///     so the contamination persisted — this is the fix.
///   * Re-run `is_recall_self_capture` against any memory's
///     `ocr_text`; if it now hits, flip `ocr_engine` to the
///     `+self-capture` suffix so retrieval skips it. v0.5.6's
///     filter only ran inside `process_ocr` for newly-OCR'd
///     screenshots, leaving existing screenshots in the library
///     unmarked.
///   * Re-extract structured entities (idempotent — UNIQUE
///     constraint dedups; safe to run repeatedly).
///
/// Independent flag so it runs even when v0.5.6 backfill is
/// already marked done.
async fn run_v0_5_7_backfill(state: &AppState) -> crate::errors::app_error::AppResult<()> {
    use crate::ai::embeddings::auto_tagger;
    use crate::ai::entities;
    use crate::ai::scheduler::worker;

    let started_at = std::time::Instant::now();
    let memories = state.memory_repository.list().await?;
    let total = memories.len();
    eprintln!("[recall][v0.5.7] backfill starting: {total} memories");

    let mut tags_changed = 0usize;
    let mut self_captures_marked = 0usize;

    for (idx, memory) in memories.iter().enumerate() {
        // 1. Replace the auto-tagger-managed tags wholesale.
        //    `replace_auto_tagger_tags` removes any tag in
        //    MANAGED_TAGS, then adds the freshly detected set.
        //    Other-source tags (link enrichment, classifier) stay.
        let detected_tags = auto_tagger::detect_tags(&memory.content);
        match state
            .memory_repository
            .replace_auto_tagger_tags(&memory.id, auto_tagger::MANAGED_TAGS, &detected_tags)
            .await
        {
            Ok(_) => tags_changed += 1,
            Err(err) => eprintln!(
                "[recall][v0.5.7] replace_auto_tagger_tags failed for {}: {err}",
                memory.id
            ),
        }

        // 2. Self-capture check against existing OCR text. We
        //    only need to update memories whose ocr_engine
        //    hasn't already been flagged.
        if let Some(ocr_text) = memory.ocr_text.as_deref() {
            let already_flagged = memory
                .ocr_engine
                .as_deref()
                .map(|e| e.contains("self-capture"))
                .unwrap_or(false);
            if !already_flagged && worker::is_recall_self_capture_text(ocr_text) {
                let new_engine = format!(
                    "{}+self-capture",
                    memory.ocr_engine.as_deref().unwrap_or("unknown")
                );
                if let Err(err) = state
                    .memory_repository
                    .set_ocr_status(
                        &memory.id,
                        memory.ocr_status.as_deref().unwrap_or("done"),
                        Some(ocr_text),
                        Some(&new_engine),
                        memory.ocr_processed_at.as_deref(),
                    )
                    .await
                {
                    eprintln!(
                        "[recall][v0.5.7] self-capture mark failed for {}: {err}",
                        memory.id
                    );
                } else {
                    self_captures_marked += 1;
                }
            }
        }

        // 3. Refresh extracted entities. Idempotent — same content
        //    + same detectors = same rows.
        let _ = entities::extract_and_persist(
            &state.memory_repository,
            &memory.id,
            &memory.content,
            &[],
        )
        .await;

        if idx % 50 == 49 {
            tokio::task::yield_now().await;
        }
    }

    let mut current = state.settings_service.get().await.unwrap_or_default();
    current.ai_v0_5_7_backfill_done = Some(true);
    // Also mark v0.5.6 done in case we hit a fresh install where
    // both flags are unset — no point running v0.5.6's logic
    // afterwards since v0.5.7's pass strictly subsumes it.
    current.ai_v0_5_6_backfill_done = Some(true);
    let _ = state.settings_service.save(&current).await;

    eprintln!(
        "[recall][v0.5.7] backfill complete: {total} memories scanned, {tags_changed} tag rows touched, {self_captures_marked} self-captures marked, in {:.1}s",
        started_at.elapsed().as_secs_f32()
    );
    Ok(())
}

/// v0.5.42 — idempotent boot-time backfill that guarantees the
/// "Twitter bookmarks" project exists and that every previously-
/// synced tweet memory lives inside it. v0.5.41 wired the project
/// auto-assign into the sync command itself, but users who updated
/// without immediately re-syncing reported the project never
/// appeared. This boot-time pass closes that gap so the project
/// shows up on the next launch regardless of whether they re-sync.
///
/// Cheap on the steady state: a single `COUNT(1)` early-out skips
/// everything when there are no unassigned tweet memories left, so
/// every boot after the first is essentially free. No settings flag
/// because the SQL itself is the idempotency gate.
async fn run_v0_5_42_twitter_backfill(state: &AppState) -> crate::errors::app_error::AppResult<()> {
    const PROJECT_NAME: &str = "Twitter bookmarks";

    // Short-circuit when there's nothing to do. Without this, every
    // boot would do a pointless `projects.list()` round-trip plus a
    // no-op UPDATE — small but unnecessary.
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(1) FROM memories \
         WHERE source_app = 'twitter' \
           AND (project_id IS NULL OR project_id = '')",
    )
    .fetch_one(&state.pool)
    .await?;
    if row.0 == 0 {
        return Ok(());
    }

    // Resolve (or create) the project. Case-insensitive match so a
    // user who created their own "twitter bookmarks" project by hand
    // earlier reuses theirs instead of getting a duplicate.
    let projects = state.project_repository.list().await?;
    let project_id = if let Some(found) = projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(PROJECT_NAME))
    {
        found.id.clone()
    } else {
        state
            .project_repository
            .create(
                PROJECT_NAME,
                Some("Synced bookmarks from X (Twitter).".to_string()),
            )
            .await?
            .id
    };

    let res = sqlx::query(
        "UPDATE memories \
         SET project_id = ?1 \
         WHERE source_app = 'twitter' \
           AND (project_id IS NULL OR project_id = '')",
    )
    .bind(&project_id)
    .execute(&state.pool)
    .await?;

    eprintln!(
        "[recall][v0.5.42] twitter backfill: {} memories assigned to '{}'",
        res.rows_affected(),
        PROJECT_NAME
    );
    Ok(())
}

/// v0.5.44 — chunk + embed any tweet memories that were inserted
/// pre-v0.5.44 via the raw repository (so they bypassed
/// `capture_service.persist`'s post-save embed hook). Without this,
/// existing tweets stay invisible to Ask Recall even after the
/// route fix lands, because dedup-by-external_id prevents
/// re-creation through the new code path.
///
/// Cheap on the steady state — the `LEFT JOIN ... WHERE c.id IS NULL`
/// query returns zero rows once every tweet has chunks, and
/// `kick_chunk_and_embed`'s hash-aware upsert is itself idempotent.
/// No settings flag because the SQL is the gate; the work runs
/// only when there's work to do.
async fn run_v0_5_44_twitter_chunks_backfill(
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    // Find tweet memory IDs that have no chunk row. Bounded by the
    // user's bookmark count (typically tens to low thousands), so a
    // single fetch_all is fine.
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT m.id FROM memories m \
         LEFT JOIN memory_chunks c ON c.memory_id = m.id \
         WHERE m.source_app = 'twitter' \
           AND c.id IS NULL",
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }
    eprintln!(
        "[recall][v0.5.44] tweet chunks backfill: {} memories pending",
        rows.len()
    );

    // Kick the chunk + embed pipeline per memory. Each call is
    // best-effort and async-spawned internally; we yield between
    // batches so a thousand-bookmark library doesn't monopolize the
    // tokio executor on the boot path.
    let mut kicked = 0u32;
    for (idx, (id,)) in rows.iter().enumerate() {
        if let Some(memory) = state.memory_repository.find(id).await? {
            state.capture_service.kick_chunk_and_embed(&memory);
            kicked += 1;
        }
        if idx % 25 == 24 {
            tokio::task::yield_now().await;
        }
    }
    eprintln!(
        "[recall][v0.5.44] tweet chunks backfill: kicked {} memories",
        kicked
    );
    Ok(())
}

/// v0.5.45 — strip the legacy `Author (@handle)\n\n` header from
/// every tweet body that still has it, then re-trigger the
/// chunk + embed pipeline so the new (header-free) text replaces the
/// stale chunks. Without this pass, existing tweets keep embedding
/// against their author-prefixed text and continue to under-rank in
/// Ask Recall retrieval even after the v0.5.45 sync-time fix lands.
///
/// Idempotent — `strip_legacy_tweet_header` returns `None` for
/// content that's already been cleaned, so the loop short-circuits
/// per-row on subsequent boots. The hash-aware `replace_chunks`
/// inside `kick_chunk_and_embed` is the second layer of idempotency.
async fn run_v0_5_45_twitter_header_strip(
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    use crate::services::x_bookmark_sync::strip_legacy_tweet_header;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, content FROM memories WHERE source_app = 'twitter'",
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let mut stripped_count = 0u32;
    for (id, content) in &rows {
        let Some(cleaned) = strip_legacy_tweet_header(content) else {
            continue;
        };
        // Update the row's body to the cleaner form. We touch only
        // `content` — title, source_window, url, etc. already carry
        // the author info, so the UI loses nothing. `updated_at`
        // intentionally NOT bumped so we don't push every tweet to
        // the top of the All Memories list on first boot.
        sqlx::query("UPDATE memories SET content = ?1 WHERE id = ?2")
            .bind(&cleaned)
            .bind(id)
            .execute(&state.pool)
            .await?;
        // Re-fetch the (now-updated) memory and kick the embed
        // pipeline. The chunker computes new content hashes; the
        // hash-aware replace deletes the stale chunk rows and
        // queues fresh embed jobs for the cleaner text.
        if let Some(memory) = state.memory_repository.find(id).await? {
            state.capture_service.kick_chunk_and_embed(&memory);
        }
        stripped_count += 1;
    }
    if stripped_count > 0 {
        eprintln!(
            "[recall][v0.5.45] twitter header strip: cleaned {} of {} tweet memories",
            stripped_count,
            rows.len()
        );
    }
    Ok(())
}

/// v0.5.47 — chunk + embed any file or folder shadow memories
/// that landed pre-v0.5.47 via the raw repository (so they
/// bypassed `capture_service.persist`'s post-save embed hook).
/// Same shape as v0.5.44's twitter chunks backfill — different
/// `source_app` filter, identical pattern.
///
/// The user reported the same symptom Twitter had: file content
/// invisible to Ask Recall. Root cause was the same: file
/// ingestion called `memory_repo.create()` directly, skipping the
/// chunker and embed-queue enqueue. The route fix (v0.5.47 in
/// file_ingestion_service.rs) handles future ingests; this
/// retroactively chunks every file/folder shadow that's already
/// in the DB without chunks.
///
/// Idempotent — the LEFT JOIN clause skips memories that already
/// have chunks, so subsequent boots are no-ops.
async fn run_v0_5_47_files_chunks_backfill(
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT m.id FROM memories m \
         LEFT JOIN memory_chunks c ON c.memory_id = m.id \
         WHERE m.source_app IN ('file', 'folder') \
           AND c.id IS NULL",
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }
    eprintln!(
        "[recall][v0.5.47] file/folder chunks backfill: {} memories pending",
        rows.len()
    );

    let mut kicked = 0u32;
    for (idx, (id,)) in rows.iter().enumerate() {
        if let Some(memory) = state.memory_repository.find(id).await? {
            state.capture_service.kick_chunk_and_embed(&memory);
            kicked += 1;
        }
        if idx % 25 == 24 {
            tokio::task::yield_now().await;
        }
    }
    eprintln!(
        "[recall][v0.5.47] file/folder chunks backfill: kicked {} memories",
        kicked
    );
    Ok(())
}

/// v0.5.49 — re-extract content for any existing .docx / .xlsx
/// (and .docm / .xlsm) file rows that landed pre-v0.5.49 with
/// empty extracted text. Pre-v0.5.49 these formats fell to the
/// "[Recall did not extract text from this file...]" placeholder
/// because Office formats weren't in the extractor's switch. The
/// route fix in `file_ingestion_service::extract_text_for_path`
/// handles future ingests; this catches files already in the DB.
///
/// Idempotent — the WHERE clause skips rows that already have
/// text. Skipped rows for files that disappeared from disk
/// between sessions get logged and continue, never propagated.
async fn run_v0_5_49_office_formats_backfill(
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    use crate::services::file_ingestion_service;

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT path FROM files \
         WHERE LOWER(extension) IN ('docx', 'docm', 'xlsx', 'xlsm') \
           AND (extracted_text IS NULL OR extracted_text = '')",
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }
    eprintln!(
        "[recall][v0.5.49] office formats backfill: {} files pending",
        rows.len()
    );

    let settings = state.settings_repository.get().await?;
    let mut re_extracted = 0u32;
    for (idx, (path_str,)) in rows.iter().enumerate() {
        let path = std::path::PathBuf::from(path_str);
        if !path.exists() {
            // Source file is gone — leave the row alone; the
            // watcher's remove path or a future GC pass cleans
            // orphaned shadows.
            continue;
        }
        // Re-call the full ingest path. ingest_path's ON CONFLICT
        // logic upserts the file row with fresh extracted_text +
        // re-routes through MemoryService::update which fires the
        // chunk + embed pipeline. One call handles everything.
        match file_ingestion_service::ingest_path(
            &state.pool,
            &state.memory_repository,
            &state.memory_service,
            &settings,
            &path,
        )
        .await
        {
            Ok(_) => re_extracted += 1,
            Err(error) => {
                eprintln!(
                    "[recall][v0.5.49] re-ingest failed for {}: {error}",
                    path.display()
                );
            }
        }
        if idx % 10 == 9 {
            tokio::task::yield_now().await;
        }
    }
    eprintln!(
        "[recall][v0.5.49] office formats backfill: re-extracted {} files",
        re_extracted
    );
    Ok(())
}

/// v0.5.54 — back-fill watched_folders from the `folders` table.
/// v0.5.48 introduced auto-watch on ingest, but folders imported
/// before that release have rows in `folders` and no entry in
/// `watched_folders`.
///
/// v0.5.56 — only add **roots** (folders with no ancestor in the
/// `folders` table). The walker creates a row per subfolder
/// during recursive ingest, so the original v0.5.54 behavior
/// (add every row) flooded the management panel with hundreds
/// of redundant entries — and watching them is wasted work
/// because notify is already recursive. A user with one
/// `Documents` ingest now sees one entry; before this fix they
/// saw the entire subtree.
async fn run_v0_5_54_watched_folders_backfill(
    app: &tauri::AppHandle,
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    use crate::commands::files::{is_path_under, normalize_for_match};

    let folder_paths: Vec<(String,)> =
        sqlx::query_as("SELECT path FROM folders ORDER BY indexed_at ASC")
            .fetch_all(&state.pool)
            .await?;

    if folder_paths.is_empty() {
        return Ok(());
    }

    // Build (raw, normalized) pairs once; the inner is_path_under
    // re-normalizes but the outer comparison loop short-circuits
    // by string equality so the cost is bounded.
    let normalized: Vec<(String, String)> = folder_paths
        .iter()
        .map(|(p,)| (p.clone(), normalize_for_match(p)))
        .collect();

    // Roots: folders whose normalized form has no other folder's
    // normalized form as an ancestor. is_path_under returns true
    // when paths are equal *or* descendant; the `n != other_n`
    // guard rejects the equal case so only true ancestors count.
    let roots: Vec<&String> = normalized
        .iter()
        .filter(|(_, n)| {
            !normalized
                .iter()
                .any(|(_, other_n)| n != other_n && is_path_under(n, other_n))
        })
        .map(|(p, _)| p)
        .collect();

    let now = chrono::Utc::now().to_rfc3339();
    let mut added = 0u32;
    for path_str in &roots {
        let path = std::path::PathBuf::from(path_str);
        if !path.exists() {
            continue; // folder gone from disk; skip the row
        }
        let result = sqlx::query(
            "INSERT OR IGNORE INTO watched_folders (path, recursive, added_at) \
             VALUES (?1, 1, ?2)",
        )
        .bind(*path_str)
        .bind(&now)
        .execute(&state.pool)
        .await?;
        if result.rows_affected() > 0 {
            added += 1;
        }
    }

    if added > 0 {
        eprintln!(
            "[recall][v0.5.54] watched-folders backfill: added {added} root folder(s) from existing ingests"
        );
        if let Err(error) = state
            .file_watcher_service
            .restore_from_db(app, &state.pool)
            .await
        {
            eprintln!(
                "[recall][v0.5.54] watcher restore (post-backfill) failed: {error}"
            );
        }
    }
    Ok(())
}

/// v0.5.56 — dedupe `watched_folders` down to roots. The
/// v0.5.54 backfill (pre-v0.5.56) inserted every row of the
/// `folders` table into `watched_folders`, including
/// subfolders the walker recorded during recursive ingest.
/// Notify already watches recursively, so each subfolder entry
/// is redundant — and the user got hundreds of redundant rows
/// in the management panel.
///
/// This sweep keeps an entry only when no other watched-folder
/// entry is a strict ancestor of it. Removed entries go through
/// `file_watcher_service.remove_watch` so the platform handle
/// gets released alongside the row delete. Idempotent — runs
/// every boot, no-ops once the set is already at roots-only.
async fn run_v0_5_56_dedupe_watched_folders(
    state: &AppState,
) -> crate::errors::app_error::AppResult<()> {
    use crate::commands::files::{is_path_under, normalize_for_match};

    let all_watched = state.file_watcher_service.list_watched(&state.pool).await?;
    if all_watched.len() < 2 {
        return Ok(());
    }

    let normalized: Vec<(String, String)> = all_watched
        .iter()
        .map(|p| (p.clone(), normalize_for_match(p)))
        .collect();

    let mut redundant: Vec<String> = Vec::new();
    for (raw, n) in &normalized {
        let has_ancestor = normalized
            .iter()
            .any(|(_, other_n)| n != other_n && is_path_under(n, other_n));
        if has_ancestor {
            redundant.push(raw.clone());
        }
    }

    if redundant.is_empty() {
        return Ok(());
    }

    eprintln!(
        "[recall][v0.5.56] dedupe: removing {} redundant watched-folder(s) (kept ancestors)",
        redundant.len()
    );
    for raw in &redundant {
        let path = std::path::PathBuf::from(raw);
        if let Err(err) = state
            .file_watcher_service
            .remove_watch(&state.pool, &path)
            .await
        {
            eprintln!(
                "[recall][v0.5.56] remove_watch failed for {raw}: {err}"
            );
        }
    }
    Ok(())
}

/// Boot the AI scheduler after the main window has opened. Two
/// reasons this lives in its own helper rather than inline in `setup()`:
///
///   * Tightly scoping the runtime borrow makes the lifetime story easy
///     to reason about.
///   * It mirrors `start_bookmark_sync_loop` / `start_clipboard_watcher`
///     — same shape, same deferred-start contract, same "background
///     services after first paint" guarantee.
///
/// The scheduler handle is stored on `AppState`; workers only spawn when
/// a native OCR adapter is available *and* the master flag is on. The
/// adapter probe is cheap (a single WinRT `TryCreateFromUserProfileLanguages`
/// or Vision availability check) so we run it eagerly and cache.
fn start_ai_scheduler(
    handle: &tauri::AppHandle,
    runtime: &tokio::runtime::Runtime,
    settings: &crate::models::AppSettings,
) {
    use ai::embeddings::fastembed_adapter::FastembedAdapter;
    use ai::llm::{qwen2_llama, registry as llm_registry};
    use ai::ocr::default_adapter;
    use ai::scheduler::{queue::AiWorkQueue, worker, AiScheduler};

    let state = handle.state::<AppState>();
    let pool = state.pool.clone();

    let queue = AiWorkQueue::new(pool.clone());

    // Reclaim any rows stranded in `running` from a prior crash. Cheap —
    // single UPDATE — and only runs once per app launch.
    if let Err(error) = runtime.block_on(queue.reclaim_stale_running()) {
        eprintln!("[recall][ai-scheduler] reclaim_stale_running failed: {error}");
    }

    let hardware = ai::hardware::detect();
    let ocr_adapter = default_adapter();
    // v0.3.0: embedding adapter. fastembed-rs handles its own model
    // download lazily; we always install the adapter so the worker can
    // claim embed jobs once the user opts in via the AI Settings tab.
    // v0.3.3: model size is picked from the detected tier (A→small,
    // B/C→base). Existing chunks embedded under a different model_id
    // get re-embedded automatically by the worker (mismatch check)
    // once the user clicks "Embed all memories".
    let embedding_adapter: Option<std::sync::Arc<dyn ai::embeddings::EmbeddingAdapter>> =
        Some(std::sync::Arc::new(FastembedAdapter::for_tier(
            handle.clone(),
            hardware.tier,
        )));
    let scheduler = AiScheduler::new(
        queue,
        ocr_adapter.clone(),
        embedding_adapter.clone(),
        hardware.clone(),
        state.settings_repository.clone(),
        settings.ai_enabled,
    );

    // Spawn workers when *either* adapter is available — the dispatcher
    // decides per-job whether to run OCR or embed work. The shared
    // worker pool means we don't statically partition concurrency
    // between kinds.
    if ocr_adapter.is_some() || embedding_adapter.is_some() {
        let max_jobs = hardware.tier.max_ocr_jobs();
        worker::spawn_workers(
            scheduler.inner(),
            pool,
            state.memory_repository.clone(),
            handle.clone(),
            max_jobs,
        );
    }

    state.install_ai_scheduler(scheduler.clone());
    // Install on capture_service so post-save OCR enqueue picks up
    // memories committed from this point onwards.
    state.capture_service.install_ai_scheduler(scheduler);

    // v0.4.0: install the tier-aware Ask Recall LLM adapter. We
    // always install one so commands can answer "is the model
    // ready / which one would I get?" — actual download + load
    // is opt-in via the AI Settings tab.
    //
    // v0.5.21: tier override. The user can pin a specific tier
    // from the AI Settings tab (e.g. force the 1.5B model on a
    // 32 GB machine to keep idle RAM lower, or force the 7B on
    // a marginal-tier-A machine if they're willing to swap).
    // The override is read at boot and used to pick the LLM
    // entry; switching it requires a restart because reloading
    // the adapter live would mean unloading the in-flight model
    // mid-call. The Settings UI is explicit about the restart.
    let effective_tier = settings.ai_tier_override.unwrap_or(hardware.tier);
    if let Some(override_tier) = settings.ai_tier_override {
        eprintln!(
            "[recall][ai] tier override active: detected={} override={} effective={}",
            hardware.tier.label(),
            override_tier.label(),
            effective_tier.label()
        );
    }
    let llm_entry = llm_registry::entry_for_tier(effective_tier);

    // v0.5.22: model GC. Scan the LLM cache directory and remove any
    // `.gguf` files that don't match a current registry entry. Runs
    // before adapter init so a switched-tier user's old model file
    // is freed even if the new one isn't downloaded yet. Best-effort
    // — failures log and don't block boot.
    if let Err(err) = qwen2_llama::gc_orphan_models(handle) {
        eprintln!("[recall][llm-gc] pass failed: {err}");
    }
    // v0.5.0: boxed() now returns AppResult because llama.cpp's
    // backend init can fail on unsupported CPUs. We log + skip
    // installation rather than panic — the rest of the AI subsystem
    // (OCR, embeddings) keeps working without Ask Recall.
    match qwen2_llama::boxed(handle.clone(), llm_entry) {
        Ok(adapter) => {
            state.install_llm_adapter(adapter.clone());
            // v0.5.13: idle reaper. The 7B Q4_K_M LLM is ~3.5 GB
            // resident once loaded; for users who run a turn or
            // two and walk away, that's a lot of RAM sitting idle.
            // Background tick checks every 60s — if `last_used_at`
            // is more than the configured threshold old AND the
            // model is still loaded, call unload(). Next ask_recall
            // pays the ~5-10s cold reload cost which is acceptable
            // for a fresh question.
            //
            // v0.5.21: threshold is now configurable via
            // `settings.ai_llm_idle_minutes` (1 / 5 / 15 / 30 / 60
            // minutes, or `0` = never unload). Read per-tick so
            // changes from the Settings tab take effect within
            // ~60 seconds without a restart. `0` skips the unload
            // check entirely so users who want the model resident
            // permanently get exactly that.
            const TICK_SECS: u64 = 60;
            let reaper_adapter = adapter;
            let reaper_settings = state.settings_repository.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
                // First tick fires immediately — skip it; we don't
                // want to unload before the user even has a chance
                // to use the LLM on this app launch.
                interval.tick().await;
                loop {
                    interval.tick().await;
                    // Read the current threshold each tick. Falls
                    // back to 5 minutes if the settings query fails
                    // (transient SQLite contention) — same as the
                    // pre-v0.5.21 hardcoded default.
                    let threshold_minutes = match reaper_settings.get().await {
                        Ok(s) => s.ai_llm_idle_minutes,
                        Err(err) => {
                            eprintln!(
                                "[recall][llm-reaper] settings read failed; using 5min default: {err}"
                            );
                            5
                        }
                    };
                    if threshold_minutes == 0 {
                        // "Never unload" — the user has explicitly
                        // pinned the model resident.
                        continue;
                    }
                    let threshold_secs = (threshold_minutes as u64).saturating_mul(60);
                    let Some(last) = reaper_adapter.last_used_at().await else {
                        // Either unloaded or never used; nothing to do.
                        continue;
                    };
                    let idle = std::time::SystemTime::now()
                        .duration_since(last)
                        .unwrap_or_default()
                        .as_secs();
                    if idle >= threshold_secs {
                        eprintln!(
                            "[recall][llm-reaper] model idle for {idle}s (threshold {threshold_secs}s); unloading"
                        );
                        if let Err(err) = reaper_adapter.unload().await {
                            eprintln!("[recall][llm-reaper] unload failed: {err}");
                        }
                    }
                }
            });
        }
        Err(err) => {
            eprintln!("[recall][ai-scheduler] LLM adapter init failed: {err}");
        }
    }
}

pub fn run() {
    // Build the tokio runtime explicitly so we can use block_on safely
    // outside of any existing async context.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            GlobalShortcutBuilder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let accelerator = shortcut.to_string();
                    tauri::async_runtime::spawn({
                        let app = app.clone();
                        async move {
                            let state = app.state::<AppState>();
                            let license_active = state
                                .license_service
                                .get_state()
                                .await
                                .map(|l| l.is_activated)
                                .unwrap_or(false);
                            match shortcut_action_for_accelerator(&state, &accelerator)
                                .await
                                .as_deref()
                            {
                                Some("open-search") => {
                                    let _ = if license_active {
                                        state.platform.window.open_search_overlay(&app).await
                                    } else {
                                        state.platform.window.open_main(&app).await
                                    };
                                }
                                Some("open-quick-save") => {
                                    let _ = if license_active {
                                        state.platform.window.open_quick_save(&app).await
                                    } else {
                                        state.platform.window.open_main(&app).await
                                    };
                                }
                                Some("open-main-app") => {
                                    let _ = state.platform.window.open_main(&app).await;
                                }
                                Some("open-pointer") => {
                                    // v0.5.61 — Recall Pointer. Read the
                                    // clipboard, resolve app context,
                                    // stash a PointerSelection, open the
                                    // search-overlay window, render
                                    // Pointer mode.
                                    //
                                    // v0.5.62 — ALWAYS open the panel,
                                    // even with an empty clipboard. The
                                    // pre-v0.5.62 silent no-op made the
                                    // feature undiscoverable: a user
                                    // pressing the hotkey to "see what
                                    // it does" got nothing and concluded
                                    // it was broken. Empty text now
                                    // renders a one-line "copy something
                                    // first" hint instead of a dead key.
                                    if !license_active {
                                        let _ = state.platform.window.open_main(&app).await;
                                    } else {
                                        // v0.5.65 — auto-copy the
                                        // live selection. The
                                        // foreground app is still
                                        // focused here (Recall's
                                        // window isn't shown yet),
                                        // so a synthetic Ctrl+C
                                        // lands in it. Windows
                                        // only + permission-free;
                                        // no-op false on macOS,
                                        // which keeps the v1
                                        // copy-first behavior. On
                                        // success wait a beat for
                                        // the target app to write
                                        // the clipboard before we
                                        // read it.
                                        if crate::platform::try_synthesize_copy() {
                                            tokio::time::sleep(
                                                std::time::Duration::from_millis(130),
                                            )
                                            .await;
                                        }
                                        let text = state
                                            .platform
                                            .clipboard
                                            .read_text(&app)
                                            .await
                                            .ok()
                                            .flatten()
                                            .map(|t| t.trim().to_string())
                                            .unwrap_or_default();
                                        let ctx = state
                                            .platform
                                            .app_context
                                            .detect_context()
                                            .await
                                            .ok();
                                        let selection =
                                            crate::models::PointerSelection {
                                                text,
                                                source_app: ctx
                                                    .as_ref()
                                                    .and_then(|c| c.source_app.clone()),
                                                source_window: ctx
                                                    .as_ref()
                                                    .and_then(|c| {
                                                        c.source_window.clone()
                                                    }),
                                                captured_at: chrono::Utc::now()
                                                    .to_rfc3339(),
                                            };
                                        {
                                            let mut slot = state
                                                .pointer_selection
                                                .lock()
                                                .await;
                                            *slot = Some(selection);
                                        }
                                        let _ = state
                                            .platform
                                            .window
                                            .open_search_overlay(&app)
                                            .await;
                                        // The overlay's frontend pulls
                                        // the stash via
                                        // pointer_take_selection on this
                                        // event; the event also covers
                                        // the case where the window was
                                        // already open.
                                        if let Some(win) = app
                                            .get_webview_window("search-overlay")
                                        {
                                            let _ = win.emit(
                                                "recall://pointer-activate",
                                                (),
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
                })
                .build(),
        )
        .setup(move |app| {
            let handle = app.handle().clone();

            // Run all async DB init synchronously using our own runtime.
            // This avoids the "cannot block inside async context" panic that
            // occurs when using tauri::async_runtime::block_on inside setup().
            let (database, init_error) = runtime.block_on(async {
                match db::initialize_database(&handle).await {
                    Ok(db) => (db, None),
                    Err(e) => {
                        eprintln!("[recall] Database init failed: {e}");
                        // Fall back to in-memory so manage() always runs
                        match db::initialize_fallback_database().await {
                            Ok(fallback) => (fallback, Some(e.to_string())),
                            Err(e2) => {
                                // Extremely unlikely — :memory: SQLite failed
                                panic!("[recall] Both real and fallback DB failed: {e2}");
                            }
                        }
                    }
                }
            });

            if init_error.is_none() {
                if let Err(e) = runtime.block_on(ensure_seed_data(&database.pool)) {
                    eprintln!("[recall] Seed data warning (non-fatal): {e}");
                }
            }

            let platform = create_platform_services();
            let settings_repository = database.settings_repository.clone();
            let mut state = AppState::new(
                database.pool,
                database.path,
                database.memory_repository,
                database.project_repository,
                settings_repository,
                database.license_repository,
                database.ask_recall_session_repository,
                database.proactive_surface_repository,
                platform,
            );
            state.init_error = init_error;

            // manage() is unconditionally reached
            app.manage(state);

            // Post-init: show widget if licensed, register shortcuts, start sync loop
            let managed = app.state::<AppState>();
            if managed.init_error.is_none() {
                let settings = runtime
                    .block_on(managed.settings_service.get())
                    .unwrap_or_default();
                let license_activated = runtime
                    .block_on(managed.license_service.get_state())
                    .map(|l| l.is_activated)
                    .unwrap_or(false);

                if let Err(e) = runtime.block_on(
                    managed
                        .platform
                        .startup
                        .apply_launch_on_startup(&handle, settings.launch_on_startup_enabled),
                ) {
                    eprintln!("[recall] Launch-on-startup sync warning: {e}");
                }

                if settings.floating_widget_enabled && license_activated {
                    let saved_pos = settings.widget_position_x.zip(settings.widget_position_y);
                    if let Err(e) =
                        runtime.block_on(managed.platform.window.ensure_widget(&handle, saved_pos))
                    {
                        eprintln!("[recall] Widget init warning: {e}");
                    }
                }

                if let Some(main_window) = app.get_webview_window("main") {
                    let _ = main_window.set_title("Recall");
                }

                let shortcuts = runtime.block_on(current_shortcut_bindings(&managed));
                if let Err(e) =
                    runtime.block_on(apply_shortcut_bindings(&handle, &managed, &shortcuts))
                {
                    eprintln!("[recall] Shortcut registration warning: {e}");
                }
                // Install screenshot store *before* the clipboard
                // watcher starts so its first tick can already write
                // image captures. It only needs the AppHandle. The
                // memory service holds its own clone so deletes can
                // unlink the on-disk file alongside the row.
                let screenshot_store = ScreenshotStore::new(handle.clone());
                managed.install_screenshot_store(screenshot_store.clone());
                managed
                    .memory_service
                    .install_screenshot_store(screenshot_store);

                // v0.2.3: 60-day screenshot retention GC. Runs once
                // immediately (so a freshly upgraded install processes
                // any backlog) then sleeps 24h between passes. Best-
                // effort — IO errors are logged and skipped.
                //
                // v0.5.32: window is per-user. Reads
                // `settings.ai_screenshot_retention_days` each pass
                // so the AI Settings dropdown applies live; `0`
                // disables the GC entirely for power users who
                // value memory completeness over disk usage.
                start_retention_loop(
                    handle.clone(),
                    managed.memory_repository.clone(),
                    managed.settings_repository.clone(),
                );

                start_bookmark_sync_loop(handle.clone());
                start_clipboard_watcher(handle.clone());
                managed.receiver_service.start(handle.clone());

                // ─── v0.2.0: AI subsystem boot (off by default) ──────────
                // The scheduler is constructed unconditionally so the
                // AI Settings tab can read hardware tier + OCR engine
                // even with AI disabled. Workers only spawn when there's
                // a usable native OCR adapter — otherwise the scheduler
                // is a read-only handle and `ocr_engine = "unsupported"`.
                start_ai_scheduler(&handle, &runtime, &settings);

                if let Ok(memories) = runtime.block_on(managed.memory_service.list()) {
                    runtime.block_on(
                        managed
                            .link_enrichment_service
                            .resume_incomplete_enrichments(handle.clone(), memories),
                    );
                }

                // v0.5.6: one-shot backfill pass. Re-runs the
                // auto-tagger and entity extractor against every
                // existing memory. Two reasons:
                //   * The auto-tagger's URL/UUID exclusion shipped
                //     in v0.5.6 — without backfill, existing
                //     URL bookmarks would still carry false-positive
                //     license-key tags from earlier versions.
                //   * The entity tables are empty until extraction
                //     runs. Without backfill, entity-pivot retrieval
                //     and the memory-detail entity chips would only
                //     work for memories saved on v0.5.6+, which
                //     would feel like the feature is broken.
                //
                // Spawned in the background after the window opens
                // so it never blocks first paint or the AI scheduler.
                // Settings flag prevents repeat passes — once it
                // completes, the flag is set and subsequent boots
                // skip it.
                // v0.5.7: independent backfill flag because the
                // v0.5.6 pass had two bugs (tag-merge couldn't
                // remove stale entries, self-capture filter never
                // ran on existing memories). Memories that already
                // saw v0.5.6 backfill still need the v0.5.7 pass
                // to scrub the contamination v0.5.6 left behind.
                let v57_done = settings
                    .ai_v0_5_7_backfill_done
                    .unwrap_or(false);
                if !v57_done {
                    let app_handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app_handle.state::<AppState>();
                        if let Err(err) =
                            run_v0_5_7_backfill(&state).await
                        {
                            eprintln!(
                                "[recall][v0.5.7] backfill failed: {err}"
                            );
                        }
                    });
                }

                // v0.5.42: ensure the "Twitter bookmarks" project
                // exists and every tweet memory lives inside it.
                // Idempotent — see helper comment for details. Lives
                // here (not gated on a settings flag) because the
                // helper's COUNT short-circuit is cheaper than the
                // settings round-trip.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_42_twitter_backfill(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.42] twitter backfill failed: {err}"
                        );
                    }
                });

                // v0.5.44: chunk + embed any tweet memories that
                // landed pre-v0.5.44 via the raw repository and so
                // never triggered the embed pipeline. Without this,
                // existing tweets stay missing from Ask Recall
                // context even after the route fix lands. Same
                // SQL-as-gate idempotency pattern as v0.5.42.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_44_twitter_chunks_backfill(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.44] tweet chunks backfill failed: {err}"
                        );
                    }
                });

                // v0.5.45: strip the legacy author header from
                // existing tweet bodies and re-embed. Pre-v0.5.45
                // syncs stamped "Author (@handle)\n\n" at the top
                // of `content`, which (a) duplicated the author
                // already in the title and (b) dragged the chunk
                // embedding toward "person mentions on twitter" and
                // away from the actual tweet topic. Idempotent —
                // strip_legacy_tweet_header returns None on already-
                // clean content, so subsequent boots are a no-op.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_45_twitter_header_strip(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.45] twitter header strip failed: {err}"
                        );
                    }
                });

                // v0.5.47: chunk + embed file/folder shadow
                // memories. Same shape as v0.5.44 for tweets —
                // pre-v0.5.47 file ingest hit the raw repository
                // directly so shadow rows landed without chunks.
                // The route fix in file_ingestion_service handles
                // future ingests; this catches rows already in the
                // DB. SQL-as-gate, no settings flag needed.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_47_files_chunks_backfill(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.47] file/folder chunks backfill failed: {err}"
                        );
                    }
                });

                // v0.5.48: re-establish filesystem watchers for
                // every folder in `watched_folders`. Spawned (not
                // inline) so the boot path never blocks on
                // watcher init. Each watcher is its own debouncer
                // so a single failed watch (folder gone offline,
                // permissions change) doesn't take down the rest.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) = state
                        .file_watcher_service
                        .restore_from_db(&app_handle, &state.pool)
                        .await
                    {
                        eprintln!(
                            "[recall][v0.5.48] watcher restore failed: {err}"
                        );
                    }
                });

                // v0.5.49: re-extract content for any pre-v0.5.49
                // .docx / .xlsx files where the placeholder text
                // landed because Office formats weren't in the
                // extractor's switch yet. Spawned + best-effort,
                // SQL-as-gate (skips files with non-empty text).
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_49_office_formats_backfill(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.49] office formats backfill failed: {err}"
                        );
                    }
                });

                // v0.5.54: back-fill watched_folders from the
                // `folders` table so pre-v0.5.48 ingests show up
                // in the management panel + start being watched.
                // v0.5.56: backfill now adds only roots, then
                // dedupe runs to clean up any redundant entries
                // left behind by the original v0.5.54 logic. Both
                // are sequenced inside one task so the dedupe
                // sees the post-backfill state.
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    if let Err(err) =
                        run_v0_5_54_watched_folders_backfill(&app_handle, &state).await
                    {
                        eprintln!(
                            "[recall][v0.5.54] watched-folders backfill failed: {err}"
                        );
                    }
                    if let Err(err) =
                        run_v0_5_56_dedupe_watched_folders(&state).await
                    {
                        eprintln!(
                            "[recall][v0.5.56] watched-folders dedupe failed: {err}"
                        );
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ai_status,
            ai_set_enabled,
            ai_set_mode,
            ai_diagnose_clipboard_image,
            ai_download_embedding_model,
            ai_download_llm,
            ai_llm_status,
            ai_unload_llm,
            ai_diagnose_llm,
            ai_force_scrub,
            ai_recent_failures,
            ai_clear_failed_ocr,
            refresh_recap_memory,
            ask_recall,
            ask_recall_cancel,
            ask_recall_new_session,
            ask_recall_get_session,
            ask_recall_list_sessions,
            ask_recall_delete_session,
            ask_recall_rename_session,
            generate_daily_recap_summary,
            save_qa_as_memory,
            proactive_surface_get_current,
            proactive_surface_dismiss,
            list_entities_for_memory,
            list_memories_by_entity,
            embed_all_memories,
            build_memory_trail,
            find_related,
            semantic_search,
            ocr_run_for_memory,
            ocr_rebuild_index,
            bootstrap_app,
            get_runtime_info,
            list_bookmark_sources,
            import_bookmarks,
            sync_bookmarks_now,
            x_connection_status,
            x_oauth_start,
            x_sync_bookmarks_now,
            x_oauth_disconnect,
            ingest_path,
            ingest_paths,
            suggested_locations,
            add_watched_folder,
            remove_watched_folder,
            list_watched_folders,
            remove_file,
            remove_folder,
            list_memories,
            get_memory,
            create_memory,
            update_memory,
            delete_memory,
            duplicate_memory,
            mark_memory_opened,
            set_memory_resurface,
            dismiss_memory_resurface,
            list_projects,
            create_project,
            update_project,
            delete_project,
            get_settings,
            update_settings,
            list_shortcuts,
            update_shortcuts,
            export_data,
            import_data,
            clear_all_data,
            seed_sample_data,
            validate_license_key,
            get_license_state,
            activate_license,
            deactivate_license,
            get_pairing_info,
            reset_pairing,
            pointer_take_selection,
            open_main_window,
            open_search_overlay,
            open_quick_save_window,
            close_current_window,
            set_widget_expanded,
            save_widget_position,
            open_memory_in_main,
            read_clipboard_text,
            write_clipboard_text,
            detect_app_context
        ])
        .run(tauri::generate_context!())
        .expect("error while running Recall");
}

#[cfg(test)]
mod shortcut_match_tests {
    use super::accelerators_equivalent;

    #[test]
    fn modifier_order_does_not_matter() {
        // The exact failure: plugin reports shift-first, we store
        // ctrl-first. Must still match.
        assert!(accelerators_equivalent("Shift+Ctrl+P", "Ctrl+Shift+P"));
        assert!(accelerators_equivalent("ctrl+shift+s", "Ctrl+Shift+S"));
        assert!(accelerators_equivalent("Control+Shift+KeyP", "Ctrl+Shift+P"));
    }

    #[test]
    fn single_modifier_still_matches() {
        assert!(accelerators_equivalent("alt+space", "Alt+Space"));
    }

    #[test]
    fn different_key_does_not_match() {
        assert!(!accelerators_equivalent("Ctrl+Shift+P", "Ctrl+Shift+S"));
    }

    #[test]
    fn missing_modifier_does_not_match() {
        // Ctrl+P must not resolve a Ctrl+Shift+P binding.
        assert!(!accelerators_equivalent("Ctrl+P", "Ctrl+Shift+P"));
    }

    #[test]
    fn empty_input_never_matches() {
        assert!(!accelerators_equivalent("", "Ctrl+Shift+P"));
    }
}
