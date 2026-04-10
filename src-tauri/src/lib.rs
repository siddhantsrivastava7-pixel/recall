mod commands;
mod db;
mod errors;
mod models;
mod platform;
mod services;
mod state;

use commands::{
    app::{bootstrap_app, get_runtime_info},
    bookmarks::{import_bookmarks, list_bookmark_sources, sync_bookmarks_now},
    license::{activate_license, deactivate_license, get_license_state, validate_license_key},
    memories::{
        create_memory, delete_memory, duplicate_memory, get_memory, list_memories, update_memory,
    },
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
use services::shortcut_service::normalize_accelerator;
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
                start_bookmark_sync_loop(handle.clone());

                if let Ok(memories) = runtime.block_on(managed.memory_service.list()) {
                    runtime.block_on(
                        managed
                            .link_enrichment_service
                            .resume_incomplete_enrichments(handle.clone(), memories),
                    );
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
