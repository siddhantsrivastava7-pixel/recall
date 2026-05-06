//! v0.5.37 — X OAuth token persistence.
//!
//! Thin SQLite wrapper around the `x_oauth_tokens` table. The
//! repo is intentionally small (4 ops): the only consumers are
//! the OAuth completion flow, the bookmark sync scheduler, the
//! status read for the Settings UI, and the disconnect flow that
//! deletes the row on user request.
//!
//! There's at most one row in v0.5.37 (single-user), but the
//! schema's `x_user_id UNIQUE` constraint already supports
//! multiple in case a future "multi-account" feature lands.

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::errors::app_error::AppResult;
use crate::services::x_bookmark_sync::XOAuthRow;

/// Insert (or replace on x_user_id collision) the just-exchanged
/// token. Returns the row id so callers can hold a handle.
pub async fn upsert_token(pool: &SqlitePool, row: &XOAuthRow) -> AppResult<String> {
    sqlx::query(
        r#"
        INSERT INTO x_oauth_tokens
          (id, x_user_id, x_username, access_token, refresh_token,
           expires_at, scope, connected_at, last_synced_at, last_sync_count)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(x_user_id) DO UPDATE SET
          access_token   = excluded.access_token,
          refresh_token  = excluded.refresh_token,
          expires_at     = excluded.expires_at,
          scope          = excluded.scope,
          x_username     = excluded.x_username,
          connected_at   = excluded.connected_at
        "#,
    )
    .bind(&row.id)
    .bind(&row.x_user_id)
    .bind(&row.x_username)
    .bind(&row.access_token)
    .bind(&row.refresh_token)
    .bind(&row.expires_at)
    .bind(&row.scope)
    .bind(&row.connected_at)
    .bind(&row.last_synced_at)
    .bind(row.last_sync_count)
    .execute(pool)
    .await?;
    Ok(row.id.clone())
}

/// Read the most-recently-connected row (single-user world today,
/// but ordering newest-first means a future multi-account UI can
/// surface them in connect-time order).
pub async fn current(pool: &SqlitePool) -> AppResult<Option<XOAuthRow>> {
    let row = sqlx::query_as::<_, XOAuthRow>(
        r#"
        SELECT id, x_user_id, x_username, access_token, refresh_token,
               expires_at, scope, connected_at, last_synced_at, last_sync_count
        FROM x_oauth_tokens
        ORDER BY datetime(connected_at) DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Stamp a successful sync. `created` is the count of new
/// memories produced this run; `last_sync_count` accumulates
/// across runs so the UI can show "Synced 423 bookmarks total".
pub async fn record_sync(
    pool: &SqlitePool,
    token_id: &str,
    created_this_run: u32,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE x_oauth_tokens
        SET last_synced_at  = ?1,
            last_sync_count = last_sync_count + ?2
        WHERE id = ?3
        "#,
    )
    .bind(now)
    .bind(created_this_run as i64)
    .bind(token_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop the connected token (Settings → "Disconnect X"). The
/// memories created from past syncs stay — disconnecting is about
/// stopping future syncs and forgetting credentials, not
/// retroactively erasing the tweets the user already pulled.
pub async fn disconnect(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query("DELETE FROM x_oauth_tokens")
        .execute(pool)
        .await?;
    Ok(())
}

/// v0.5.37 — generate a fresh row id when callers haven't built
/// one already. Surfaced in `XOAuthRow::default_id` so the OAuth
/// flow doesn't need to import uuid directly.
#[allow(dead_code)]
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}
