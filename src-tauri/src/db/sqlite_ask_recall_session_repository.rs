//! SQLite implementation of `AskRecallSessionRepository`.
//!
//! v0.5.15: persistent Ask Recall conversations — sessions and
//! their messages survive app restart so the sidebar's RECENT
//! CHATS section actually has content. Storage shape mirrors the
//! in-memory v0.5.11 shape closely; the only twist is that
//! `retrieved_sources` and `citations` are stored as JSON BLOBs
//! since nothing else queries them by content (they're pure
//! presentation data the LLM emitted markers for).
//!
//! Two writes coordinate via a single transaction in
//! `append_message`: the message row INSERT and the parent
//! session's `message_count + 1` / `last_used_at = now` UPDATE.
//! Without the transaction a crash mid-append would leave the
//! count out of sync with the actual rows; the sidebar's
//! "12 messages" badge would lie. Cheap to do — both writes hit
//! the same page.

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::{
    db::repositories::AskRecallSessionRepository,
    errors::app_error::AppResult,
    models::{AskRecallMessageRow, AskRecallSessionFull, AskRecallSessionSummary},
};

pub struct SqliteAskRecallSessionRepository {
    pool: SqlitePool,
}

impl SqliteAskRecallSessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AskRecallSessionRepository for SqliteAskRecallSessionRepository {
    async fn create_session(
        &self,
        session_id: &str,
        title: &str,
        now_iso: &str,
    ) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO ask_recall_sessions
              (session_id, title, llm_title, created_at, last_used_at, message_count)
            VALUES (?1, ?2, NULL, ?3, ?3, 0)
            "#,
        )
        .bind(session_id)
        .bind(title)
        .bind(now_iso)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn rename_session(&self, session_id: &str, title: &str) -> AppResult<()> {
        sqlx::query(
            r#"
            UPDATE ask_recall_sessions
            SET title = ?1
            WHERE session_id = ?2
            "#,
        )
        .bind(title)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_llm_title(&self, session_id: &str, llm_title: &str) -> AppResult<()> {
        sqlx::query(
            r#"
            UPDATE ask_recall_sessions
            SET llm_title = ?1
            WHERE session_id = ?2
            "#,
        )
        .bind(llm_title)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> AppResult<()> {
        // ON DELETE CASCADE handles ask_recall_messages. Idempotent —
        // deleting a missing session is Ok(()) by SQLite semantics.
        sqlx::query("DELETE FROM ask_recall_sessions WHERE session_id = ?1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_sessions(&self) -> AppResult<Vec<AskRecallSessionSummary>> {
        let rows = sqlx::query_as::<_, AskRecallSessionSummary>(
            r#"
            SELECT session_id, title, llm_title, created_at, last_used_at, message_count
            FROM ask_recall_sessions
            ORDER BY datetime(last_used_at) DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> AppResult<Option<AskRecallSessionFull>> {
        let summary = sqlx::query_as::<_, AskRecallSessionSummary>(
            r#"
            SELECT session_id, title, llm_title, created_at, last_used_at, message_count
            FROM ask_recall_sessions
            WHERE session_id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(summary) = summary else {
            return Ok(None);
        };
        let messages = sqlx::query_as::<_, AskRecallMessageRow>(
            r#"
            SELECT id, session_id, sequence, role, content,
                   retrieved_sources, citations, tokens_generated,
                   latency_ms, tag_intent, timestamp
            FROM ask_recall_messages
            WHERE session_id = ?1
            ORDER BY sequence ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(AskRecallSessionFull {
            session_id: summary.session_id,
            title: summary.title,
            llm_title: summary.llm_title,
            created_at: summary.created_at,
            last_used_at: summary.last_used_at,
            messages,
        }))
    }

    async fn append_message(&self, message: &AskRecallMessageRow) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO ask_recall_messages
              (id, session_id, sequence, role, content,
               retrieved_sources, citations, tokens_generated,
               latency_ms, tag_intent, timestamp)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(message.sequence)
        .bind(&message.role)
        .bind(&message.content)
        .bind(&message.retrieved_sources)
        .bind(&message.citations)
        .bind(message.tokens_generated)
        .bind(message.latency_ms)
        .bind(&message.tag_intent)
        .bind(&message.timestamp)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE ask_recall_sessions
            SET message_count = message_count + 1,
                last_used_at = ?1
            WHERE session_id = ?2
            "#,
        )
        .bind(&message.timestamp)
        .bind(&message.session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
