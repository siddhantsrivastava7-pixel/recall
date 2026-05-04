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
        ai_diagnose_clipboard_image, ai_diagnose_llm, ai_download_embedding_model,
        ai_download_llm, ai_force_scrub, ai_llm_status, ai_set_enabled, ai_set_mode,
        ai_status, ai_unload_llm, ask_recall, ask_recall_cancel,
        ask_recall_delete_session, ask_recall_get_session, ask_recall_list_sessions,
        ask_recall_new_session, ask_recall_rename_session, embed_all_memories,
        find_related, generate_daily_recap_summary, list_entities_for_memory,
        list_memories_by_entity, ocr_rebuild_index, ocr_run_for_memory,
        save_qa_as_memory, semantic_search,
    },
    app::{bootstrap_app, get_runtime_info},
    bookmarks::{import_bookmarks, list_bookmark_sources, sync_bookmarks_now},
    license::{activate_license, deactivate_license, get_license_state, validate_license_key},
    memories::{
        create_memory, delete_memory, dismiss_memory_resurface, duplicate_memory, get_memory,
        list_memories, mark_memory_opened, set_memory_resurface, update_memory,
    },
    pairing::{get_pairing_info, reset_pairing},
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

async fn shortcut_action_for_accelerator(state: &AppState, accelerator: &str) -> Option<String> {
    let normalized = normalize_accelerator(accelerator);
    let bindings = state
        .shortcut_service
        .list(&state.platform.shortcuts.bindings())
        .await
        .ok()?;

    bindings
        .into_iter()
        .find(|binding| normalize_accelerator(&binding.accelerator) == normalized)
        .map(|binding| binding.action)
}

pub(crate) async fn apply_shortcut_bindings(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcuts: &[crate::models::ShortcutBinding],
) -> crate::errors::app_error::AppResult<()> {
    let shortcut_manager = app.global_shortcut();
    let previous_bindings = current_shortcut_bindings(state).await;

    let previous_accelerators = previous_bindings
        .iter()
        .map(|binding| binding.accelerator.clone())
        .collect::<Vec<_>>();

    let _ = shortcut_manager.unregister_all();

    let mut registered = Vec::new();
    for binding in shortcuts {
        if let Err(error) = shortcut_manager.register(binding.accelerator.as_str()) {
            let _ = shortcut_manager.unregister_all();
            for accelerator in previous_accelerators {
                let _ = shortcut_manager.register(accelerator.as_str());
            }
            return Err(crate::errors::app_error::AppError::Invalid(format!(
                "shortcut `{}` could not be registered: {}",
                binding.accelerator, error
            )));
        }
        registered.push(binding.accelerator.clone());
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
                start_retention_loop(handle.clone(), managed.memory_repository.clone());

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
            ask_recall,
            ask_recall_cancel,
            ask_recall_new_session,
            ask_recall_get_session,
            ask_recall_list_sessions,
            ask_recall_delete_session,
            ask_recall_rename_session,
            generate_daily_recap_summary,
            save_qa_as_memory,
            list_entities_for_memory,
            list_memories_by_entity,
            embed_all_memories,
            find_related,
            semantic_search,
            ocr_run_for_memory,
            ocr_rebuild_index,
            bootstrap_app,
            get_runtime_info,
            list_bookmark_sources,
            import_bookmarks,
            sync_bookmarks_now,
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
