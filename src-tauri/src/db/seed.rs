use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{db::system_projects::ensure_default_inbox_project, errors::app_error::AppResult};

pub async fn ensure_seed_data(pool: &SqlitePool) -> AppResult<()> {
    ensure_default_inbox_project(pool).await?;

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) as count FROM memories")
        .fetch_one(pool)
        .await?;

    if count.0 > 0 {
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();
    let project_one = Uuid::new_v4().to_string();
    let project_two = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO projects (id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&project_one)
        .bind("Recall Product")
        .bind("Positioning, UX, and architecture notes for the product itself.")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

    sqlx::query("INSERT INTO projects (id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&project_two)
        .bind("Client Launch")
        .bind("Useful fragments gathered while preparing a launch plan.")
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

    for (title, content, note, project_id, source_app, source_window) in [
        (
            "Tauri multi-window note",
            "Use separate windows for widget, quick save, and command overlay. Keep shared domain logic platform-agnostic and window orchestration native-side.",
            "This is the core V1 product architecture principle.",
            Some(project_one.as_str()),
            Some("Code"),
            Some("recall/src-tauri/src/lib.rs"),
        ),
        (
            "Launch positioning line",
            "Recall is not a notes app. It is a private local-first memory layer for people working across browsers, documents, AI chats, and desktop tools.",
            "This line keeps messaging focused and differentiated.",
            Some(project_two.as_str()),
            Some("Chrome"),
            Some("Product positioning doc"),
        ),
        (
            "Search scoring idea",
            "Phrase matches and title matches should outrank loose content matches, with notes weighted slightly above raw content because they contain user intent.",
            "Useful for V1 search ranking and semantic-search migration later.",
            Some(project_one.as_str()),
            Some("Figma"),
            Some("Recall information architecture"),
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO memories (
              id, title, content, note, project_id, source_app, source_window, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(title)
        .bind(content)
        .bind(note)
        .bind(project_id)
        .bind(source_app)
        .bind(source_window)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    }

    sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES ('floating_widget_enabled', 'true')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES ('launch_on_startup_enabled', 'false')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES ('bookmark_auto_sync_enabled', 'true')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES ('bookmark_sync_interval_minutes', '15')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR REPLACE INTO app_settings (key, value) VALUES ('bookmark_sync_browsers', '[\"chrome\",\"edge\",\"brave\"]')")
        .execute(pool)
        .await?;

    Ok(())
}
