//! SQLite implementation of `ProactiveSurfaceRepository`.
//!
//! Schema is created in `migrations.rs`:
//!
//! ```sql
//! CREATE TABLE proactive_surfaces (
//!   id           TEXT PRIMARY KEY,
//!   kind         TEXT NOT NULL,
//!   memory_id    TEXT NOT NULL,
//!   score        REAL NOT NULL DEFAULT 0,
//!   reason       TEXT,
//!   surfaced_at  TEXT NOT NULL,
//!   dismissed_at TEXT,
//!   expires_at   TEXT,
//!   FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
//! );
//! ```
//!
//! `ON DELETE CASCADE` ties the card's lifetime to its referenced
//! memory — if the user deletes the underlying memory we don't
//! want a stale "Forgotten Gold" card pointing into the void.

use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    db::repositories::ProactiveSurfaceRepository,
    errors::app_error::AppResult,
    models::ProactiveSurfaceRow,
};

pub struct SqliteProactiveSurfaceRepository {
    pool: SqlitePool,
}

impl SqliteProactiveSurfaceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProactiveSurfaceRepository for SqliteProactiveSurfaceRepository {
    async fn record_surface(
        &self,
        kind: &str,
        memory_id: &str,
        score: f64,
        reason: Option<&str>,
        surfaced_at: &str,
        expires_at: Option<&str>,
    ) -> AppResult<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO proactive_surfaces \
               (id, kind, memory_id, score, reason, surfaced_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(kind)
        .bind(memory_id)
        .bind(score)
        .bind(reason)
        .bind(surfaced_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn latest_active_for_kind(
        &self,
        kind: &str,
        now_iso: &str,
    ) -> AppResult<Option<ProactiveSurfaceRow>> {
        // Active = not dismissed AND (no expiry OR expiry > now).
        // Newest-first by surfaced_at so a fresh card displaces an
        // older one of the same kind even if both are technically
        // active.
        let row = sqlx::query_as::<_, ProactiveSurfaceRow>(
            "SELECT id, kind, memory_id, score, reason, surfaced_at, \
                    dismissed_at, expires_at \
             FROM proactive_surfaces \
             WHERE kind = ? \
               AND dismissed_at IS NULL \
               AND (expires_at IS NULL OR datetime(expires_at) > datetime(?)) \
             ORDER BY datetime(surfaced_at) DESC \
             LIMIT 1",
        )
        .bind(kind)
        .bind(now_iso)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn dismiss(&self, id: &str, dismissed_at: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE proactive_surfaces \
             SET dismissed_at = ? \
             WHERE id = ? AND dismissed_at IS NULL",
        )
        .bind(dismissed_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn has_recorded_since(
        &self,
        kind: &str,
        since_iso: &str,
    ) -> AppResult<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM proactive_surfaces \
             WHERE kind = ? AND datetime(surfaced_at) >= datetime(?)",
        )
        .bind(kind)
        .bind(since_iso)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }
}
