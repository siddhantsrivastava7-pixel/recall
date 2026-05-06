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

// ─── v0.5.37: X (Twitter) bookmark sync ──────────────────────────

use crate::services::{x_bookmark_sync, x_oauth_repository};
use tauri_plugin_opener::OpenerExt;

/// v0.5.37 — public read of the current X connection state.
/// Frontend uses this to render "Connect X" vs "Connected as
/// @username · 412 bookmarks synced" copy in the Settings panel.
/// Returns null when no row exists in `x_oauth_tokens`.
#[tauri::command]
pub async fn x_connection_status(
    state: State<'_, AppState>,
) -> AppResult<Option<x_bookmark_sync::XOAuthRow>> {
    x_oauth_repository::current(&state.pool).await
}

/// v0.5.37 — start the OAuth 2.0 PKCE flow. Generates the PKCE
/// pair, opens the user's system browser to X's authorize page,
/// waits up to 5 minutes for the loopback redirect, exchanges the
/// auth code for tokens, fetches the user's profile, and
/// persists the token row.
///
/// Blocking on the loopback wait inside the command is the
/// simplest UX — the frontend shows a spinner while this resolves.
/// The 5-minute timeout in `wait_for_callback` ensures we don't
/// park forever if the user closes the browser tab.
#[tauri::command]
pub async fn x_oauth_start(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<x_bookmark_sync::XOAuthRow> {
    let pkce = x_bookmark_sync::start_oauth_flow()?;

    // Open the X authorize URL in the user's default browser.
    // Tauri's opener plugin handles platform differences.
    if let Err(error) = app.opener().open_url(&pkce.authorize_url, None::<&str>) {
        return Err(crate::errors::app_error::AppError::Invalid(format!(
            "Could not open browser for X authorization: {error}"
        )));
    }

    // Wait for X's redirect to the loopback URL. Returns the
    // auth code (and verifies state matches CSRF token).
    let callback = x_bookmark_sync::wait_for_callback(&pkce.state).await?;

    // Trade the auth code for tokens + populate username.
    let row = x_bookmark_sync::exchange_code_for_tokens(
        &callback.code,
        &pkce.code_verifier,
    )
    .await?;

    x_oauth_repository::upsert_token(&state.pool, &row).await?;
    Ok(row)
}

/// v0.5.37 — manually-triggered bookmark sync. Pulls every
/// bookmarked tweet for the connected user and creates memory
/// rows for new ones. Idempotent (dedup by tweet_id stored as
/// `external_id` on the memory row).
#[tauri::command]
pub async fn x_sync_bookmarks_now(
    state: State<'_, AppState>,
) -> AppResult<x_bookmark_sync::BookmarkSyncResult> {
    let token = x_oauth_repository::current(&state.pool)
        .await?
        .ok_or_else(|| {
            crate::errors::app_error::AppError::Invalid(
                "Not connected to X. Connect from Settings → Bookmarks first.".into(),
            )
        })?;
    let result = x_bookmark_sync::sync_bookmarks(&token, &state.memory_repository).await?;
    x_oauth_repository::record_sync(&state.pool, &token.id, result.created).await?;
    Ok(result)
}

/// v0.5.37 — drop the X connection. Removes the token row but
/// leaves any tweets that were already imported as memories
/// alone — the user kept those, the disconnect is about stopping
/// future syncs, not erasing past ones.
#[tauri::command]
pub async fn x_oauth_disconnect(state: State<'_, AppState>) -> AppResult<()> {
    x_oauth_repository::disconnect(&state.pool).await
}
