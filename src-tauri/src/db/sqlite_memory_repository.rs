use async_trait::async_trait;
use chrono::Utc;
use sqlx::types::Json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    db::repositories::MemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{LinkEnrichmentStatus, LinkEnrichmentUpdate, Memory, MemoryInput, MemorySourceType},
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
  memories.last_opened_at,
  memories.open_count,
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
              last_opened_at,
              open_count,
              created_at,
              updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 0, 0, NULL, ?, ?, NULL, NULL, NULL, ?, ?, ?, ?, NULL, 0, ?, ?)
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
        let enrichment_input_changed =
            url_changed || input.content != existing.content || input.note != existing.note;
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
