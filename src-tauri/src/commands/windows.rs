use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::{errors::app_error::AppResult, state::app_state::AppState};

async fn license_allows_aux_windows(state: &AppState) -> AppResult<bool> {
    Ok(state.license_service.get_state().await?.is_activated)
}

#[tauri::command]
pub async fn open_main_window(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    state.platform.window.open_main(&app).await
}

#[tauri::command]
pub async fn open_search_overlay(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if !license_allows_aux_windows(&state).await? {
        return state.platform.window.open_main(&app).await;
    }
    state.platform.window.open_search_overlay(&app).await
}

#[tauri::command]
pub async fn open_quick_save_window(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if !license_allows_aux_windows(&state).await? {
        return state.platform.window.open_main(&app).await;
    }
    state.platform.window.open_quick_save(&app).await
}

#[tauri::command]
pub async fn close_current_window(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.platform.window.close_window(&window).await
}

#[tauri::command]
pub async fn set_widget_expanded(
    app: AppHandle,
    expanded: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state
        .platform
        .window
        .set_widget_expanded(&app, expanded)
        .await
}

/// Called from the widget frontend when the user finishes dragging.
/// Saves the current physical position of the widget window to the DB
/// so it can be restored on next launch.
#[tauri::command]
pub async fn save_widget_position(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("widget") {
        if let (Ok(pos), Ok(scale)) = (window.outer_position(), window.scale_factor()) {
            let logical_x = pos.x as f64 / scale;
            let logical_y = pos.y as f64 / scale;
            let mut settings = state.settings_service.get().await?;
            settings.widget_position_x = Some(logical_x);
            settings.widget_position_y = Some(logical_y);
            state.settings_service.save(&settings).await?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn open_memory_in_main(
    app: AppHandle,
    memory_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state
        .platform
        .window
        .open_memory_in_main(&app, memory_id)
        .await
}
