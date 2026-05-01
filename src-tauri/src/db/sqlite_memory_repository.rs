use async_trait::async_trait;
use chrono::Utc;
use sqlx::types::Json;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{
    db::repositories::{ChunkUpsert, EmbeddingCoverage, MemoryRepository},
    errors::app_error::{AppError, AppResult},
    models::{
        LinkEnrichmentStatus, LinkEnrichmentUpdate, Memory, MemoryChunkRow, MemoryInput,
        MemorySourceType,
    },
    services::link_utils::extract_domain,
};

const MEMORY_SELECT: &str = r#"
SELECT
  memories.id,
  memories.source_type,
  memories.title,
  memories.content,
  memories.note,
  memories.project_id,
  projects.name AS project_name,
  memories.url,
  memories.domain,
  memories.resolved_domain,
  memories.canonical_url,
  memories.resolved_title,
  memories.resolved_description,
  memories.resolved_image,
  memories.resolved_site_name,
  memories.preview_text,
  memories.summary_text,
  memories.extracted_text,
  memories.memory_type,
  memories.topic_labels,
  memories.primary_topic,
  memories.quality_score,
  memories.bookmark_quality_score,
  memories.is_duplicate_of,
  memories.bookmark_folder_path,
  memories.enrichment_status,
  memories.enrichment_error,
  memories.enriched_at,
  memories.last_enriched_at,
  memories.external_id,
  memories.folder_path,
  memories.source_app,
  memories.source_window,
  memories.resurface_at,
  memories.resurface_dismissed_at,
  memories.last_opened_at,
  memories.open_count,
  memories.ocr_text,
  memories.ocr_status,
  memories.ocr_processed_at,
  memories.ocr_engine,
  memories.ocr_error,
  memories.embedding_model_version,
  memories.embedding_generated_at,
  memories.created_at,
  memories.updated_at
FROM memories
LEFT JOIN projects ON projects.id = memories.project_id
"#;

pub struct SqliteMemoryRepository {
    pool: SqlitePool,
}

impl SqliteMemoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn source_type_label(source_type: MemorySourceType) -> &'static str {
    match source_type {
        MemorySourceType::Manual => "manual",
        MemorySourceType::Bookmark => "bookmark",
    }
}

fn pending_enrichment_status() -> Option<LinkEnrichmentStatus> {
    Some(LinkEnrichmentStatus::Pending)
}

fn clean_summary_text(value: &str) -> Option<String> {
    let collapsed = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn trim_summary(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    if let Some((index, _)) = truncated
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(character, '.' | '!' | '?'))
    {
        if index >= 48 {
            return truncated[..=index].trim().to_string();
        }
    }

    truncated
        .rsplit_once(' ')
        .map(|(head, _)| head)
        .unwrap_or(truncated.as_str())
        .trim()
        .to_string()
}

fn is_url_only(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

fn looks_like_domain(value: &str) -> bool {
    let trimmed = value.trim().trim_start_matches("www.").to_ascii_lowercase();
    trimmed.contains('.') && !trimmed.contains(' ') && !trimmed.starts_with("http")
}

fn build_summary_text(
    title: Option<&str>,
    note: Option<&str>,
    content: &str,
    url: Option<&str>,
    domain: Option<&str>,
) -> Option<String> {
    let normalized_content = clean_summary_text(content)?;
    let content_is_url = is_url_only(&normalized_content);

    let candidates = [
        if !content_is_url {
            Some(normalized_content.as_str())
        } else {
            None
        },
        note,
        title.filter(|value| !is_url_only(value) && !looks_like_domain(value)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Some(cleaned) = clean_summary_text(candidate) {
            return Some(trim_summary(&cleaned, 220));
        }
    }

    if let Some(domain) = domain {
        return Some(format!(
            "Saved link from {domain}. Open the source to view the saved page."
        ));
    }

    url.and_then(extract_domain).map(|domain| {
        format!("Saved link from {domain}. Open the source to view the saved page.")
    })
}

#[async_trait]
impl MemoryRepository for SqliteMemoryRepository {
    async fn list(&self) -> AppResult<Vec<Memory>> {
        let records = sqlx::query_as::<_, Memory>(&format!(
            "{MEMORY_SELECT} ORDER BY datetime(memories.updated_at) DESC"
        ))
        .fetch_all(&self.pool)
        .await?;

        Ok(records)
    }

    async fn find(&self, id: &str) -> AppResult<Option<Memory>> {
        let record = sqlx::query_as::<_, Memory>(&format!("{MEMORY_SELECT} WHERE memories.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(record)
    }

    async fn find_by_external_source(
        &self,
        source_app: &str,
        external_id: &str,
    ) -> AppResult<Option<Memory>> {
        let record = sqlx::query_as::<_, Memory>(&format!(
            "{MEMORY_SELECT} WHERE memories.source_app = ? AND memories.external_id = ?"
        ))
        .bind(source_app)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(record)
    }

    async fn create(&self, input: MemoryInput) -> AppResult<Memory> {
        if input.content.trim().is_empty() {
            return Err(AppError::Invalid("Memory content is required.".into()));
        }

        let source_type = input.source_type.unwrap_or(MemorySourceType::Manual);
        let id = Uuid::new_v4().to_string();
        let created_at = input.created_at.unwrap_or_else(|| Utc::now().to_rfc3339());
        let updated_at = input.updated_at.unwrap_or_else(|| created_at.clone());
        let domain = input.url.as_deref().and_then(extract_domain);
        let resolved_domain = domain.clone();
        let canonical_url = input.url.clone();
        let summary_text = build_summary_text(
            input.title.as_deref(),
            input.note.as_deref(),
            &input.content,
            input.url.as_deref(),
            domain.as_deref(),
        );
        let enrichment_status = pending_enrichment_status();
        let bookmark_folder_path = if source_type == MemorySourceType::Bookmark {
            input.folder_path.clone()
        } else {
            None
        };

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO memories (
              id,
              source_type,
              title,
              content,
              note,
              project_id,
              url,
              domain,
              resolved_domain,
              canonical_url,
              resolved_title,
              resolved_description,
              resolved_image,
              resolved_site_name,
              preview_text,
              summary_text,
              extracted_text,
              memory_type,
              topic_labels,
              primary_topic,
              quality_score,
              bookmark_quality_score,
              is_duplicate_of,
              bookmark_folder_path,
              enrichment_status,
              enrichment_error,
              enriched_at,
              last_enriched_at,
              external_id,
              folder_path,
              source_app,
              source_window,
              resurface_at,
              resurface_dismissed_at,
              last_opened_at,
              open_count,
              created_at,
              updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, ?, NULL, NULL, NULL, NULL, 0, 0, NULL, ?, ?, NULL, NULL, NULL, ?, ?, ?, ?, NULL, NULL, NULL, 0, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(source_type_label(source_type))
        .bind(input.title)
        .bind(input.content)
        .bind(input.note)
        .bind(input.project_id)
        .bind(input.url)
        .bind(domain)
        .bind(resolved_domain)
        .bind(canonical_url)
        .bind(summary_text)
        .bind(bookmark_folder_path)
        .bind(enrichment_status)
        .bind(input.external_id)
        .bind(input.folder_path)
        .bind(input.source_app)
        .bind(input.source_window)
        .bind(&created_at)
        .bind(&updated_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        self.find(&id)
            .await?
            .ok_or_else(|| AppError::Invalid("Created memory could not be reloaded.".into()))
    }

    async fn update(&self, id: &str, input: MemoryInput) -> AppResult<Memory> {
        if input.content.trim().is_empty() {
            return Err(AppError::Invalid("Memory content is required.".into()));
        }

        let existing = self
            .find(id)
            .await?
            .ok_or_else(|| AppError::Invalid("Memory not found.".into()))?;
        let source_type = input.source_type.unwrap_or(MemorySourceType::Manual);
        let url_changed = input.url != existing.url;
        let enrichment_input_changed = url_changed || input.content != existing.content;
        let summary_input_changed =
            enrichment_input_changed || input.title != existing.title || input.note != existing.note;
        let domain = input.url.as_deref().and_then(extract_domain);
        let resolved_domain = if enrichment_input_changed {
            domain.clone()
        } else {
            existing.resolved_domain.clone().or(domain.clone())
        };
        let canonical_url = if enrichment_input_changed {
            input.url.clone()
        } else {
            existing.canonical_url.clone().or(input.url.clone())
        };

        let resolved_title = if enrichment_input_changed {
            None
        } else {
            existing.resolved_title.clone()
        };
        let resolved_description = if enrichment_input_changed {
            None
        } else {
            existing.resolved_description.clone()
        };
        let resolved_image = if enrichment_input_changed {
            None
        } else {
            existing.resolved_image.clone()
        };
        let resolved_site_name = if enrichment_input_changed {
            None
        } else {
            existing.resolved_site_name.clone()
        };
        let topic_labels = if enrichment_input_changed {
            None
        } else {
            existing.topic_labels.clone()
        };
        let bookmark_quality_score = if enrichment_input_changed {
            Some(0.0)
        } else {
            existing.bookmark_quality_score
        };
        let preview_text = if enrichment_input_changed {
            None
        } else {
            existing.preview_text.clone()
        };
        let extracted_text = if enrichment_input_changed {
            None
        } else {
            existing.extracted_text.clone()
        };
        let summary_text = if summary_input_changed {
            build_summary_text(
                input.title.as_deref(),
                input.note.as_deref(),
                &input.content,
                input.url.as_deref(),
                domain.as_deref(),
            )
        } else {
            existing.summary_text.clone()
        };
        let memory_type = if enrichment_input_changed {
            None
        } else {
            existing.memory_type
        };
        let quality_score = if enrichment_input_changed {
            Some(0.0)
        } else {
            existing.quality_score
        };
        let primary_topic = if enrichment_input_changed {
            None
        } else {
            existing.primary_topic.clone()
        };
        let is_duplicate_of = if enrichment_input_changed {
            None
        } else {
            existing.is_duplicate_of.clone()
        };
        let bookmark_folder_path = if source_type == MemorySourceType::Bookmark {
            input
                .folder_path
                .clone()
                .or(existing.bookmark_folder_path.clone())
        } else {
            None
        };
        let enrichment_status = if enrichment_input_changed {
            pending_enrichment_status()
        } else {
            existing.enrichment_status
        };
        let enrichment_error = if enrichment_input_changed {
            None
        } else {
            existing.enrichment_error.clone()
        };
        let enriched_at = if enrichment_input_changed {
            None
        } else {
            existing.enriched_at.clone()
        };
        let last_enriched_at = if enrichment_input_changed {
            None
        } else {
            existing.last_enriched_at.clone()
        };

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            UPDATE memories
            SET
              source_type = ?,
              title = ?,
              content = ?,
              note = ?,
              project_id = ?,
              url = ?,
              domain = ?,
              resolved_domain = ?,
              canonical_url = ?,
              resolved_title = ?,
              resolved_description = ?,
              resolved_image = ?,
              resolved_site_name = ?,
              preview_text = ?,
              summary_text = ?,
              extracted_text = ?,
              memory_type = ?,
              topic_labels = ?,
              primary_topic = ?,
              quality_score = ?,
              bookmark_quality_score = ?,
              is_duplicate_of = ?,
              bookmark_folder_path = ?,
              enrichment_status = ?,
              enrichment_error = ?,
              enriched_at = ?,
              last_enriched_at = ?,
              external_id = ?,
              folder_path = ?,
              source_app = ?,
              source_window = ?,
              updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(source_type_label(source_type))
        .bind(input.title)
        .bind(input.content)
        .bind(input.note)
        .bind(input.project_id)
        .bind(input.url)
        .bind(domain)
        .bind(resolved_domain)
        .bind(canonical_url)
        .bind(resolved_title)
        .bind(resolved_description)
        .bind(resolved_image)
        .bind(resolved_site_name)
        .bind(preview_text)
        .bind(summary_text)
        .bind(extracted_text)
        .bind(memory_type)
        .bind(topic_labels)
        .bind(primary_topic)
        .bind(quality_score)
        .bind(bookmark_quality_score)
        .bind(is_duplicate_of)
        .bind(bookmark_folder_path)
        .bind(enrichment_status)
        .bind(enrichment_error)
        .bind(enriched_at)
        .bind(last_enriched_at)
        .bind(input.external_id)
        .bind(input.folder_path)
        .bind(input.source_app)
        .bind(input.source_window)
        .bind(input.updated_at.unwrap_or_else(|| Utc::now().to_rfc3339()))
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        self.find(id)
            .await?
            .ok_or_else(|| AppError::Invalid("Memory not found.".into()))
    }

    async fn update_link_enrichment(
        &self,
        id: &str,
        enrichment: LinkEnrichmentUpdate,
    ) -> AppResult<Option<Memory>> {
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET
              url = ?,
              domain = ?,
              resolved_domain = ?,
              canonical_url = ?,
              resolved_title = ?,
              resolved_description = ?,
              resolved_image = ?,
              resolved_site_name = ?,
              preview_text = ?,
              summary_text = ?,
              extracted_text = ?,
              memory_type = ?,
              topic_labels = ?,
              primary_topic = ?,
              quality_score = ?,
              bookmark_quality_score = ?,
              is_duplicate_of = ?,
              bookmark_folder_path = ?,
              enrichment_status = ?,
              enrichment_error = ?,
              enriched_at = ?
              ,
              last_enriched_at = ?
            WHERE id = ?
            "#,
        )
        .bind(enrichment.url)
        .bind(enrichment.domain)
        .bind(enrichment.resolved_domain)
        .bind(enrichment.canonical_url)
        .bind(enrichment.resolved_title)
        .bind(enrichment.resolved_description)
        .bind(enrichment.resolved_image)
        .bind(enrichment.resolved_site_name)
        .bind(enrichment.preview_text)
        .bind(enrichment.summary_text)
        .bind(enrichment.extracted_text)
        .bind(enrichment.memory_type)
        .bind(enrichment.topic_labels.map(Json))
        .bind(enrichment.primary_topic)
        .bind(enrichment.quality_score)
        .bind(enrichment.bookmark_quality_score)
        .bind(enrichment.is_duplicate_of)
        .bind(enrichment.bookmark_folder_path)
        .bind(enrichment.enrichment_status)
        .bind(enrichment.enrichment_error)
        .bind(enrichment.enriched_at)
        .bind(enrichment.last_enriched_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.find(id).await
    }

    async fn set_resurface(
        &self,
        id: &str,
        resurface_at: Option<String>,
        updated_at: &str,
    ) -> AppResult<Option<Memory>> {
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET
              resurface_at = ?,
              resurface_dismissed_at = NULL,
              updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(resurface_at)
        .bind(updated_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.find(id).await
    }

    async fn dismiss_resurface(
        &self,
        id: &str,
        dismissed_at: &str,
        updated_at: &str,
    ) -> AppResult<Option<Memory>> {
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET
              resurface_dismissed_at = ?,
              updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(dismissed_at)
        .bind(updated_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.find(id).await
    }

    async fn mark_opened(&self, id: &str, opened_at: &str) -> AppResult<Option<Memory>> {
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET
              last_opened_at = ?,
              open_count = COALESCE(open_count, 0) + 1
            WHERE id = ?
            "#,
        )
        .bind(opened_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.find(id).await
    }

    async fn set_ocr_status(
        &self,
        id: &str,
        status: &str,
        text: Option<&str>,
        engine: Option<&str>,
        processed_at: Option<&str>,
    ) -> AppResult<()> {
        // For 'running' we only flip the status — leave any existing text
        // (e.g. from a prior successful OCR) and the last error untouched
        // until the run actually finishes. For 'done' / 'failed' / 'pending'
        // the caller writes the full triple and the row reflects that.
        if status == "running" {
            sqlx::query(
                r#"
                UPDATE memories
                SET ocr_status = ?
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else if status == "failed" {
            sqlx::query(
                r#"
                UPDATE memories
                SET ocr_status = ?,
                    ocr_error  = ?,
                    ocr_engine = COALESCE(?, ocr_engine),
                    ocr_processed_at = COALESCE(?, ocr_processed_at)
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(text) // we overload `text` as the error message for status='failed'
            .bind(engine)
            .bind(processed_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE memories
                SET ocr_status = ?,
                    ocr_text   = ?,
                    ocr_engine = ?,
                    ocr_processed_at = ?,
                    ocr_error  = NULL
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(text)
            .bind(engine)
            .bind(processed_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn promote_ocr_to_content(
        &self,
        id: &str,
        ocr_text: &str,
        derived_title: &str,
    ) -> AppResult<bool> {
        // Two guards keep this from clobbering user edits:
        //   1. source_app must be 'screenshot' or 'imported_image' — only
        //      Recall's own clipboard-image branch tags rows that way.
        //   2. content must still match the synthetic placeholder body
        //      that the watcher writes on first capture. Once the user
        //      edits the body even slightly, the LIKE match misses and
        //      the row is left alone.
        // We also rebuild summary_text so the timeline preview reflects
        // the new body.
        let result = sqlx::query(
            r#"
            UPDATE memories
            SET
              content = ?1,
              title = CASE
                WHEN title LIKE 'Screenshot · %' THEN ?2
                ELSE title
              END,
              summary_text = substr(?1, 1, 220),
              updated_at = ?3
            WHERE id = ?4
              AND source_app IN ('screenshot', 'imported_image')
              AND content LIKE 'Screenshot from clipboard%OCR will fill in%'
            "#,
        )
        .bind(ocr_text)
        .bind(derived_title)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn clear_url_for_purged_screenshots(
        &self,
        purged_paths: &[String],
    ) -> AppResult<u64> {
        if purged_paths.is_empty() {
            return Ok(0);
        }

        // Build the IN-clause dynamically. SQLite handles thousands of
        // bound parameters fine; in practice the daily GC batch will be
        // a few dozen at most.
        let mut clauses = Vec::with_capacity(purged_paths.len());
        for _ in purged_paths {
            clauses.push("url LIKE ?");
        }
        let where_clause = clauses.join(" OR ");
        let sql = format!(
            r#"
            UPDATE memories
            SET url = NULL
            WHERE source_app IN ('screenshot', 'imported_image')
              AND url IS NOT NULL
              AND ({where_clause})
            "#
        );

        let mut query = sqlx::query(&sql);
        for path in purged_paths {
            // file:// URLs are stored verbatim; match on the path
            // suffix (the filename portion) which is unique per file.
            // `%/<filename>` covers both Windows (with mangled drive
            // letter) and POSIX path layouts.
            let like_pattern = format!("%{path}");
            query = query.bind(like_pattern);
        }
        let result = query.execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn list_chunks_for_memory(&self, memory_id: &str) -> AppResult<Vec<MemoryChunkRow>> {
        let rows = sqlx::query_as::<_, MemoryChunkRow>(
            r#"
            SELECT id, memory_id, chunk_index, text, start_offset, end_offset,
                   byte_size, token_estimate, content_hash, embedding_model,
                   embedding_dim, embedding_vector, embedding_generated_at, created_at
            FROM memory_chunks
            WHERE memory_id = ?
            ORDER BY chunk_index ASC
            "#,
        )
        .bind(memory_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn replace_chunks_hash_aware(
        &self,
        memory_id: &str,
        chunks: &[ChunkUpsert<'_>],
    ) -> AppResult<Vec<String>> {
        // Read existing chunks once, key by content_hash so we can
        // copy embedding columns forward for any incoming chunk whose
        // text didn't actually change. The chunk_id changes (we're
        // re-inserting), but the embedding bytes don't — and any
        // ai_work_queue rows that referenced the old chunk_id will
        // become no-op successes when the worker can't find them
        // (process_embed_chunk treats missing chunk as Ok(())).
        //
        // We can't UPDATE existing rows' chunk_index in place — when
        // chunker output reshuffles (e.g., a daily transcript grew
        // and pushed boundaries), an UPDATE that moves a row to an
        // index still held by another (about-to-be-orphaned) row
        // hits `UNIQUE(memory_id, chunk_index)` before we ever get to
        // delete the orphan. So: DELETE every existing row for this
        // memory first, then INSERT the new chunks at their final
        // positions. Preserves hash-aware embedding reuse, drops the
        // UNIQUE-collision risk entirely.
        let existing = self.list_chunks_for_memory(memory_id).await?;
        let by_hash: std::collections::HashMap<String, MemoryChunkRow> = existing
            .into_iter()
            .map(|r| (r.content_hash.clone(), r))
            .collect();

        let now = Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await?;

        sqlx::query("DELETE FROM memory_chunks WHERE memory_id = ?")
            .bind(memory_id)
            .execute(&mut *transaction)
            .await?;

        let mut needs_embedding: Vec<String> = Vec::new();
        for chunk in chunks {
            let new_id = Uuid::new_v4().to_string();
            let preserved = by_hash.get(chunk.content_hash);
            sqlx::query(
                r#"
                INSERT INTO memory_chunks
                  (id, memory_id, chunk_index, text, start_offset, end_offset,
                   byte_size, token_estimate, content_hash,
                   embedding_model, embedding_dim, embedding_vector, embedding_generated_at,
                   created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&new_id)
            .bind(memory_id)
            .bind(chunk.chunk_index as i64)
            .bind(chunk.text)
            .bind(chunk.start_offset as i64)
            .bind(chunk.end_offset as i64)
            .bind(chunk.byte_size as i64)
            .bind(chunk.token_estimate as i64)
            .bind(chunk.content_hash)
            .bind(preserved.and_then(|r| r.embedding_model.as_deref()))
            .bind(preserved.and_then(|r| r.embedding_dim))
            .bind(preserved.and_then(|r| r.embedding_vector.as_deref()))
            .bind(preserved.and_then(|r| r.embedding_generated_at.as_deref()))
            .bind(&now)
            .execute(&mut *transaction)
            .await?;

            // Need a fresh embedding when either there's no preserved
            // row at all (novel hash) or the preserved row never had
            // a vector to begin with.
            let needs_fresh = match preserved {
                Some(p) => p.embedding_vector.is_none(),
                None => true,
            };
            if needs_fresh {
                needs_embedding.push(new_id);
            }
        }

        transaction.commit().await?;
        Ok(needs_embedding)
    }

    async fn set_chunk_embedding(
        &self,
        chunk_id: &str,
        model: &str,
        dim: u32,
        vector_bytes: &[u8],
        generated_at: &str,
    ) -> AppResult<()> {
        sqlx::query(
            r#"
            UPDATE memory_chunks
            SET embedding_model = ?,
                embedding_dim = ?,
                embedding_vector = ?,
                embedding_generated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(model)
        .bind(dim as i64)
        .bind(vector_bytes)
        .bind(generated_at)
        .bind(chunk_id)
        .execute(&self.pool)
        .await?;

        // Bubble the timestamp up to the parent memory so
        // `embedding_coverage` and the AI settings progress readout
        // can answer "when did this memory's embeddings last update?"
        // without scanning every chunk.
        sqlx::query(
            r#"
            UPDATE memories
            SET embedding_model_version = ?,
                embedding_generated_at = ?
            WHERE id = (
              SELECT memory_id FROM memory_chunks WHERE id = ?
            )
            "#,
        )
        .bind(model)
        .bind(generated_at)
        .bind(chunk_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_embedded_chunks(&self) -> AppResult<Vec<MemoryChunkRow>> {
        let rows = sqlx::query_as::<_, MemoryChunkRow>(
            r#"
            SELECT id, memory_id, chunk_index, text, start_offset, end_offset,
                   byte_size, token_estimate, content_hash, embedding_model,
                   embedding_dim, embedding_vector, embedding_generated_at, created_at
            FROM memory_chunks
            WHERE embedding_vector IS NOT NULL
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_embedded_chunks_for_model(
        &self,
        model_id: &str,
    ) -> AppResult<Vec<MemoryChunkRow>> {
        let rows = sqlx::query_as::<_, MemoryChunkRow>(
            r#"
            SELECT id, memory_id, chunk_index, text, start_offset, end_offset,
                   byte_size, token_estimate, content_hash, embedding_model,
                   embedding_dim, embedding_vector, embedding_generated_at, created_at
            FROM memory_chunks
            WHERE embedding_vector IS NOT NULL
              AND embedding_model = ?1
            "#,
        )
        .bind(model_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn count_embedded_chunks_for_model(&self, model_id: &str) -> AppResult<u64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM memory_chunks
            WHERE embedding_vector IS NOT NULL
              AND embedding_model = ?1
            "#,
        )
        .bind(model_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0.max(0) as u64)
    }

    async fn embedding_coverage(&self) -> AppResult<EmbeddingCoverage> {
        // `embedded_chunks_active_model` is left at 0 here; the
        // command layer fills it in by calling
        // `count_embedded_chunks_for_model` with the live adapter id —
        // the repository doesn't know which model is currently active.
        let row = sqlx::query(
            r#"
            SELECT
              (SELECT COUNT(*) FROM memories) AS total_memories,
              (SELECT COUNT(DISTINCT memory_id) FROM memory_chunks) AS memories_with_chunks,
              (SELECT COUNT(*) FROM memory_chunks) AS total_chunks,
              (SELECT COUNT(*) FROM memory_chunks WHERE embedding_vector IS NOT NULL) AS embedded_chunks
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(EmbeddingCoverage {
            total_memories: row.try_get::<i64, _>("total_memories")?.max(0) as u64,
            memories_with_chunks: row.try_get::<i64, _>("memories_with_chunks")?.max(0) as u64,
            total_chunks: row.try_get::<i64, _>("total_chunks")?.max(0) as u64,
            embedded_chunks: row.try_get::<i64, _>("embedded_chunks")?.max(0) as u64,
            embedded_chunks_active_model: 0,
        })
    }

    async fn delete(&self, id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear(&self) -> AppResult<()> {
        sqlx::query("DELETE FROM memories")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
