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
  is_trial INTEGER NOT NULL DEFAULT 0,
  activated_at TEXT,
  expires_at TEXT,
  last_checked_at TEXT
);
"#;

async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> AppResult<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .any(|row| row.get::<String, _>("name") == column))
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

    ensure_column(
        pool,
        "memories",
        "source_type",
        "TEXT NOT NULL DEFAULT 'manual'",
    )
    .await?;
    ensure_column(pool, "memories", "url", "TEXT").await?;
    ensure_column(pool, "memories", "domain", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_domain", "TEXT").await?;
    ensure_column(pool, "memories", "canonical_url", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_title", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_description", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_image", "TEXT").await?;
    ensure_column(pool, "memories", "resolved_site_name", "TEXT").await?;
    ensure_column(pool, "memories", "preview_text", "TEXT").await?;
    ensure_column(pool, "memories", "summary_text", "TEXT").await?;
    ensure_column(pool, "memories", "extracted_text", "TEXT").await?;
    ensure_column(pool, "memories", "memory_type", "TEXT").await?;
    ensure_column(pool, "memories", "topic_labels", "TEXT").await?;
    ensure_column(pool, "memories", "primary_topic", "TEXT").await?;
    ensure_column(pool, "memories", "quality_score", "REAL").await?;
    ensure_column(pool, "memories", "bookmark_quality_score", "REAL").await?;
    ensure_column(pool, "memories", "is_duplicate_of", "TEXT").await?;
    ensure_column(pool, "memories", "bookmark_folder_path", "TEXT").await?;
    ensure_column(pool, "memories", "enrichment_status", "TEXT").await?;
    ensure_column(pool, "memories", "enrichment_error", "TEXT").await?;
    ensure_column(pool, "memories", "enriched_at", "TEXT").await?;
    ensure_column(pool, "memories", "last_enriched_at", "TEXT").await?;
    ensure_column(pool, "memories", "external_id", "TEXT").await?;
    ensure_column(pool, "memories", "folder_path", "TEXT").await?;
    ensure_column(pool, "memories", "resurface_at", "TEXT").await?;
    ensure_column(pool, "memories", "resurface_dismissed_at", "TEXT").await?;
    ensure_column(pool, "memories", "last_opened_at", "TEXT").await?;
    ensure_column(
        pool,
        "memories",
        "open_count",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;

    sqlx::query(
        r#"
        UPDATE memories
        SET summary_text = CASE
          WHEN trim(COALESCE(content, '')) LIKE 'http://%'
            OR trim(COALESCE(content, '')) LIKE 'https://%'
          THEN CASE
            WHEN title IS NOT NULL
              AND trim(title) != ''
              AND trim(title) NOT LIKE 'http://%'
              AND trim(title) NOT LIKE 'https://%'
              AND instr(trim(title), '.') = 0
            THEN trim(title)
            WHEN note IS NOT NULL AND trim(note) != ''
            THEN substr(replace(replace(trim(note), char(13), ' '), char(10), ' '), 1, 220)
            WHEN COALESCE(resolved_domain, domain) IS NOT NULL
            THEN 'Saved link from ' || COALESCE(resolved_domain, domain) || '. Open the source to view the saved page.'
            ELSE substr(replace(replace(trim(content), char(13), ' '), char(10), ' '), 1, 220)
          END
          ELSE substr(replace(replace(trim(content), char(13), ' '), char(10), ' '), 1, 220)
        END
        WHERE summary_text IS NULL OR trim(summary_text) = ''
        "#,
    )
    .execute(pool)
    .await?;
    ensure_column(
        pool,
        "license_state",
        "is_trial",
        "INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    ensure_column(pool, "license_state", "expires_at", "TEXT").await?;

    sqlx::query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_external_source
        ON memories(source_app, external_id)
        WHERE external_id IS NOT NULL AND source_app IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    // ─── v0.2.0: AI subsystem foundation (additive, rollback-safe) ───────
    //
    // OCR fields on memories. Allowed `ocr_status` values: NULL | 'pending'
    // | 'running' | 'done' | 'failed'. `ocr_engine` records which native
    // adapter produced the text (so we can re-OCR with a different engine
    // later without losing the prior result on conflict).
    ensure_column(pool, "memories", "ocr_text", "TEXT").await?;
    ensure_column(pool, "memories", "ocr_status", "TEXT").await?;
    ensure_column(pool, "memories", "ocr_processed_at", "TEXT").await?;
    ensure_column(pool, "memories", "ocr_engine", "TEXT").await?;
    ensure_column(pool, "memories", "ocr_error", "TEXT").await?;

    // Empty model_assets table created in Phase 1 only so Phase 2 ships a
    // column add (cheap) rather than a CREATE TABLE against a populated DB.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS model_assets (
          id            TEXT PRIMARY KEY NOT NULL,
          kind          TEXT NOT NULL,
          version       TEXT NOT NULL,
          sha256        TEXT NOT NULL,
          byte_size     INTEGER,
          status        TEXT NOT NULL DEFAULT 'absent',
          downloaded_at TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Persisted AI work queue. `dedupe_key UNIQUE` is the linchpin: a crash
    // mid-OCR followed by re-enqueue does not double-process. `started_at`
    // / `finished_at` give us latency telemetry without a side log;
    // `last_error` survives across retry attempts for diagnosis.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ai_work_queue (
          id            TEXT PRIMARY KEY NOT NULL,
          kind          TEXT NOT NULL,
          payload       TEXT NOT NULL,
          dedupe_key    TEXT UNIQUE NOT NULL,
          status        TEXT NOT NULL,
          attempts      INTEGER NOT NULL DEFAULT 0,
          last_error    TEXT,
          scheduled_for TEXT,
          started_at    TEXT,
          finished_at   TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_ai_work_queue_status_scheduled
        ON ai_work_queue(status, scheduled_for)
        WHERE status IN ('queued', 'failed')
        "#,
    )
    .execute(pool)
    .await?;

    // ─── v0.3.0: Memory chunks for embeddings + RAG ─────────────────────
    //
    // Each memory is split into one or more chunks at capture time
    // (paragraph-aware char-budget chunker — see ai/embeddings/chunker.rs).
    // Each chunk carries its text, character offsets within the parent
    // memory, a content hash for deterministic re-embed invalidation, and
    // its embedding vector as a BLOB (little-endian f32).
    //
    // Why chunks instead of one vector per memory:
    //   1. RAG retrieval in Phase 3 returns top-K *chunks* so the LLM
    //      sees just the relevant slices of long memories rather than
    //      the whole 50KB article.
    //   2. Re-embedding on edit becomes incremental — chunks whose text
    //      didn't actually change keep their existing embeddings via
    //      content_hash matching.
    //   3. Citations resolve to character ranges (start_offset..end_offset)
    //      so the UI can highlight exactly the cited passage.
    //
    // ON DELETE CASCADE: deleting a memory drops every chunk that
    // referenced it — the worker never tries to embed an orphan, and the
    // store stays consistent without an explicit cleanup job.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_chunks (
          id                       TEXT PRIMARY KEY NOT NULL,
          memory_id                TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
          chunk_index              INTEGER NOT NULL,
          text                     TEXT NOT NULL,
          start_offset             INTEGER NOT NULL,
          end_offset               INTEGER NOT NULL,
          byte_size                INTEGER NOT NULL,
          token_estimate           INTEGER,
          content_hash             TEXT NOT NULL,
          embedding_model          TEXT,
          embedding_dim            INTEGER,
          embedding_vector         BLOB,
          embedding_generated_at   TEXT,
          created_at               TEXT NOT NULL,
          UNIQUE(memory_id, chunk_index)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_chunks_memory
        ON memory_chunks(memory_id)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_chunks_unembedded
        ON memory_chunks(embedding_generated_at)
        WHERE embedding_generated_at IS NULL
        "#,
    )
    .execute(pool)
    .await?;

    // Optional: parent-memory column tracking the embedding state without
    // a row-by-chunk join. `embedding_generated_at` on the parent is the
    // most recent successful embed time across its chunks; useful for the
    // AI Settings progress readout.
    ensure_column(pool, "memories", "embedding_model_version", "TEXT").await?;
    ensure_column(pool, "memories", "embedding_generated_at", "TEXT").await?;

    Ok(())
}
