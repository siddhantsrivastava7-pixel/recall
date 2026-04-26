use tauri::{AppHandle, State};

use crate::{errors::app_error::AppResult, models::AppContextSnapshot, state::app_state::AppState};

#[tauri::command]
pub async fn read_clipboard_text(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    state.platform.clipboard.read_text(&app).await
}

#[tauri::command]
pub async fn write_clipboard_text(
    app: AppHandle,
    text: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.platform.clipboard.write_text(&app, &text).await
}

#[tauri::command]
pub async fn detect_app_context(state: State<'_, AppState>) -> AppResult<AppContextSnapshot> {
    state.platform.app_context.detect_context().await
}
