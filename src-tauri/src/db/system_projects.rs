use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::app_error::AppResult;

pub const DEFAULT_INBOX_PROJECT_ID: &str = "system-inbox";
pub const DEFAULT_INBOX_PROJECT_NAME: &str = "Inbox";
pub const DEFAULT_INBOX_PROJECT_DESCRIPTION: &str =
    "Default local bucket for memories captured without a project.";

pub async fn ensure_default_inbox_project(pool: &SqlitePool) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO projects (id, name, description, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(DEFAULT_INBOX_PROJECT_ID)
    .bind(DEFAULT_INBOX_PROJECT_NAME)
    .bind(DEFAULT_INBOX_PROJECT_DESCRIPTION)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(())
}
