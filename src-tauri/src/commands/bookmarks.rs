use tauri::{AppHandle, Emitter, State};

use crate::{
    errors::app_error::AppResult,
    models::{BookmarkBrowser, BookmarkSourceStatus, BookmarkSyncSummary},
    state::app_state::AppState,
};

#[tauri::command]
pub async fn list_bookmark_sources(
    state: State<'_, AppState>,
) -> AppResult<Vec<BookmarkSourceStatus>> {
    state.bookmark_service.list_sources().await
}

#[tauri::command]
pub async fn import_bookmarks(
    app: AppHandle,
    browsers: Vec<BookmarkBrowser>,
    state: State<'_, AppState>,
) -> AppResult<BookmarkSyncSummary> {
    let summary = state
        .bookmark_service
        .import_browsers(app.clone(), browsers)
        .await?;
    app.emit("recall://bookmarks-synced", &summary)?;
    Ok(summary)
}

#[tauri::command]
pub async fn sync_bookmarks_now(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BookmarkSyncSummary> {
    let summary = state
        .bookmark_service
        .sync_selected_browsers(app.clone())
        .await?;
    app.emit("recall://bookmarks-synced", &summary)?;
    Ok(summary)
}
