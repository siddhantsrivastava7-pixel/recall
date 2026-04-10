use async_trait::async_trait;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    db::repositories::ProjectRepository,
    errors::app_error::{AppError, AppResult},
    models::Project,
};

pub struct SqliteProjectRepository {
    pool: SqlitePool,
}

impl SqliteProjectRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProjectRepository for SqliteProjectRepository {
    async fn list(&self) -> AppResult<Vec<Project>> {
        let projects = sqlx::query_as::<_, Project>(
            "SELECT id, name, description, created_at, updated_at FROM projects ORDER BY datetime(updated_at) DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(projects)
    }

    async fn create(&self, name: &str, description: Option<String>) -> AppResult<Project> {
        if name.trim().is_empty() {
            return Err(AppError::Invalid("Project name is required.".into()));
        }

        let id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO projects (id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name.trim())
        .bind(description)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&self.pool)
        .await?;

        let project = sqlx::query_as::<_, Project>(
            "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(project)
    }

    async fn update(
        &self,
        id: &str,
        name: &str,
        description: Option<String>,
    ) -> AppResult<Project> {
        if name.trim().is_empty() {
            return Err(AppError::Invalid("Project name is required.".into()));
        }

        sqlx::query("UPDATE projects SET name = ?, description = ?, updated_at = ? WHERE id = ?")
            .bind(name.trim())
            .bind(description)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;

        let project = sqlx::query_as::<_, Project>(
            "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        project.ok_or_else(|| AppError::Invalid("Project not found.".into()))
    }

    async fn delete(&self, id: &str) -> AppResult<()> {
        sqlx::query("UPDATE memories SET project_id = NULL WHERE project_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear(&self) -> AppResult<()> {
        sqlx::query("DELETE FROM projects")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
