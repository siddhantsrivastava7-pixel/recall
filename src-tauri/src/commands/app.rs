use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, State, WebviewWindow};

use crate::{
    errors::app_error::{AppError, AppResult},
    models::{BootstrapPayload, RuntimeInfo},
    state::app_state::AppState,
};

async fn run_startup_bookmark_sync_if_needed(
    app: &AppHandle,
    window: &WebviewWindow,
    state: &AppState,
) -> AppResult<()> {
    if window.label() != "main" {
        return Ok(());
    }

    if state
        .startup_bookmark_sync_completed
        .load(Ordering::Acquire)
    {
        return Ok(());
    }

    let settings = state.settings_service.get().await?;
    if !settings.bookmark_auto_sync_enabled || settings.bookmark_sync_browsers.is_empty() {
        state
            .startup_bookmark_sync_completed
            .store(true, Ordering::Release);
        return Ok(());
    }

    let should_sync = state
        .startup_bookmark_sync_completed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();

    if !should_sync {
        return Ok(());
    }

    match state.bookmark_service.sync_selected_browsers(app.clone()).await {
        Ok(summary) => {
            let _ = app.emit("recall://bookmarks-synced", &summary);
        }
        Err(error) => {
            eprintln!("[recall] Startup bookmark sync warning: {error}");
            state
                .startup_bookmark_sync_completed
                .store(false, Ordering::Release);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn bootstrap_app(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<BootstrapPayload> {
    // Surface any startup error (e.g. DB failed to init) directly to the frontend
    if let Some(ref err) = state.init_error {
        return Err(AppError::Invalid(err.clone()));
    }

    run_startup_bookmark_sync_if_needed(&app, &window, &state).await?;

    let runtime = RuntimeInfo {
        platform: state.platform.app_context.platform(),
        current_window_label: window.label().to_string(),
        database_path: state.database_path.display().to_string(),
    };

    Ok(BootstrapPayload {
        runtime,
        settings: state.settings_service.get().await?,
        license: state.license_service.get_state().await?,
        memories: state.memory_service.list().await?,
        projects: state.project_service.list().await?,
        shortcuts: state
            .shortcut_service
            .list(&state.platform.shortcuts.bindings())
            .await?,
    })
}

#[tauri::command]
pub async fn get_runtime_info(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<RuntimeInfo> {
    Ok(RuntimeInfo {
        platform: state.platform.app_context.platform(),
        current_window_label: window.label().to_string(),
        database_path: state.database_path.display().to_string(),
    })
}
