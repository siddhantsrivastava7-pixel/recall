use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    errors::app_error::AppResult,
    models::{
        AppSettings, LicenseState, LinkEnrichmentUpdate, Memory, MemoryChunkRow, MemoryInput,
        Project,
    },
};

#[async_trait]
pub trait MemoryRepository: Send + Sync {
    async fn list(&self) -> AppResult<Vec<Memory>>;
    async fn find(&self, id: &str) -> AppResult<Option<Memory>>;
    async fn find_by_external_source(
        &self,
        source_app: &str,
        external_id: &str,
    ) -> AppResult<Option<Memory>>;
    async fn create(&self, input: MemoryInput) -> AppResult<Memory>;
    async fn update(&self, id: &str, input: MemoryInput) -> AppResult<Memory>;
    async fn update_link_enrichment(
        &self,
        id: &str,
        enrichment: LinkEnrichmentUpdate,
    ) -> AppResult<Option<Memory>>;
    async fn set_resurface(
        &self,
        id: &str,
        resurface_at: Option<String>,
        updated_at: &str,
    ) -> AppResult<Option<Memory>>;
    async fn dismiss_resurface(
        &self,
        id: &str,
        dismissed_at: &str,
        updated_at: &str,
    ) -> AppResult<Option<Memory>>;
    async fn mark_opened(&self, id: &str, opened_at: &str) -> AppResult<Option<Memory>>;
    /// Update OCR status fields on a memory row. Used by the AI scheduler:
    /// `'running'` while a worker is processing it, `'done'` with the
    /// recognized text on success, `'failed'` with the error on failure.
    /// Passing `None` for any field leaves the existing value untouched
    /// when the status is `'running'` (we don't want a transient state to
    /// blow away last-good text); on `'done' | 'failed'` the fields are
    /// written verbatim so callers control exactly what's persisted.
    async fn set_ocr_status(
        &self,
        id: &str,
        status: &str,
        text: Option<&str>,
        engine: Option<&str>,
        processed_at: Option<&str>,
    ) -> AppResult<()>;
    /// After OCR succeeds on a screenshot memory, replace the placeholder
    /// `"Screenshot from clipboard (...). OCR will fill in the text once
    /// it runs."` body with the recognized text, and the placeholder
    /// `"Screenshot · <date> · <time>"` title with the first line of
    /// that text. Returns `Ok(true)` when the row was updated, `Ok(false)`
    /// when the row's current content/title doesn't look like a
    /// placeholder (i.e. the user edited it manually — we never clobber
    /// human edits). The match is intentionally narrow on purpose.
    async fn promote_ocr_to_content(
        &self,
        id: &str,
        ocr_text: &str,
        derived_title: &str,
    ) -> AppResult<bool>;
    /// Clear `memory.url` for screenshot memories whose backing file has
    /// been purged by the retention GC. Returns the number of rows
    /// updated. The OCR text + everything else stays — only the dangling
    /// `file://` URL goes.
    async fn clear_url_for_purged_screenshots(
        &self,
        purged_paths: &[String],
    ) -> AppResult<u64>;
    async fn delete(&self, id: &str) -> AppResult<()>;
    async fn clear(&self) -> AppResult<()>;

    // ─── v0.3.0: chunk + embedding operations ─────────────────────────

    /// Read all chunk rows for a memory, ordered by `chunk_index`.
    /// Used by re-chunk flows to compare incoming chunks against
    /// existing rows by `content_hash`, and by `find_related` to load
    /// the source memory's vectors.
    async fn list_chunks_for_memory(&self, memory_id: &str) -> AppResult<Vec<MemoryChunkRow>>;

    /// Replace a memory's chunks. Hash-aware: an incoming chunk whose
    /// `content_hash` matches an existing chunk keeps its existing
    /// `embedding_*` columns (avoiding a re-embed). Chunks that go
    /// missing are deleted; novel chunks are inserted with
    /// `embedding_generated_at = NULL` so the worker picks them up.
    /// Returns the IDs of chunks that need fresh embeddings.
    async fn replace_chunks_hash_aware(
        &self,
        memory_id: &str,
        chunks: &[ChunkUpsert<'_>],
    ) -> AppResult<Vec<String>>;

    /// Persist an embedding on a chunk row. Called by the worker after
    /// successful inference.
    async fn set_chunk_embedding(
        &self,
        chunk_id: &str,
        model: &str,
        dim: u32,
        vector_bytes: &[u8],
        generated_at: &str,
    ) -> AppResult<()>;

    /// Read every chunk that has an embedding. Used by the brute-force
    /// `find_related` cosine search. At sub-50k vectors this is fast
    /// enough; we'll swap to `sqlite-vec` when a real user crosses the
    /// threshold.
    async fn list_embedded_chunks(&self) -> AppResult<Vec<MemoryChunkRow>>;

    /// Aggregate embedding-coverage counts for the AI Settings tab:
    /// how many memories have at least one chunk, how many of those
    /// have all chunks embedded, etc.
    async fn embedding_coverage(&self) -> AppResult<EmbeddingCoverage>;
}

/// Input shape for `replace_chunks_hash_aware`. Borrows the chunk
/// fields rather than owning them so the chunker can hand its
/// `Vec<Chunk>` directly without cloning text.
#[derive(Debug, Clone)]
pub struct ChunkUpsert<'a> {
    pub chunk_index: usize,
    pub text: &'a str,
    pub start_offset: usize,
    pub end_offset: usize,
    pub byte_size: usize,
    pub token_estimate: usize,
    pub content_hash: &'a str,
}

/// Summary of embedding-coverage state — surfaced in AI Settings.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCoverage {
    pub total_memories: u64,
    pub memories_with_chunks: u64,
    pub total_chunks: u64,
    pub embedded_chunks: u64,
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn list(&self) -> AppResult<Vec<Project>>;
    async fn create(&self, name: &str, description: Option<String>) -> AppResult<Project>;
    async fn update(&self, id: &str, name: &str, description: Option<String>)
        -> AppResult<Project>;
    async fn delete(&self, id: &str) -> AppResult<()>;
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self) -> AppResult<AppSettings>;
    async fn save(&self, settings: &AppSettings) -> AppResult<AppSettings>;
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
pub trait LicenseRepository: Send + Sync {
    async fn get(&self) -> AppResult<LicenseState>;
    async fn save(&self, license_state: &LicenseState) -> AppResult<LicenseState>;
    async fn clear(&self) -> AppResult<()>;
}

pub type SharedMemoryRepository = Arc<dyn MemoryRepository>;
pub type SharedProjectRepository = Arc<dyn ProjectRepository>;
pub type SharedSettingsRepository = Arc<dyn SettingsRepository>;
pub type SharedLicenseRepository = Arc<dyn LicenseRepository>;
