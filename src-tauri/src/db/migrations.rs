use sqlx::{Row, SqlitePool};

use crate::errors::app_error::AppResult;

pub const INITIAL_MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT,
  content TEXT NOT NULL,
  note TEXT,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  source_app TEXT,
  source_window TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS license_state (
  id TEXT PRIMARY KEY NOT NULL,
  license_key TEXT,
  is_activated INTEGER NOT NULL,
  activated_at TEXT,
  last_checked_at TEXT
);
"#;

async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> AppResult<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;

    Ok(rows.into_iter().any(|row| row.get::<String, _>("name") == column))
}

async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> AppResult<()> {
    if !has_column(pool, table, column).await? {
        let statement = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        sqlx::query(&statement).execute(pool).await?;
    }

    Ok(())
}

pub async fn run_migrations(pool: &SqlitePool) -> AppResult<()> {
    for statement in INITIAL_MIGRATION
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        sqlx::query(statement).execute(pool).await?;
    }

    ensure_column(pool, "memories", "source_type", "TEXT NOT NULL DEFAULT 'manual'").await?;
    ensure_column(pool, "memories", "url", "TEXT").await?;
    ensure_column(pool, "memories", "domain", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_domain", "TEXT").await?;
    ensure_column(pool, "memories", "canonical_url", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_title", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_description", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_image", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_site_name", "TEXT").await?;
    ensure_column(pool, "memories", "topic_labels", "TEXT").await?;
    ensure_column(pool, "memories", "bookmark_quality_score", "REAL").await?;
    ensure_column(pool, "memories", "is_duplicate_of", "TEXT").await?;
    ensure_column(pool, "memories", "bookmark_folder_path", "TEXT").await?;
    ensure_column(pool, "memories", "enrichment_status", "TEXT").await?;
    ensure_column(pool, "memories", "enriched_at", "TEXT").await?;
    ensure_column(pool, "memories", "last_enriched_at", "TEXT").await?;
    ensure_column(pool, "memories", "external_id", "TEXT").await?;
    ensure_column(pool, "memories", "folder_path", "TEXT").await?;

    sqlx::query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_external_source
        ON memories(source_app, external_id)
        WHERE external_id IS NOT NULL AND source_app IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}
