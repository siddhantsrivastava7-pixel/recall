//! Embedding subsystem (v0.3.0+).
//!
//! Three responsibilities:
//!
//! 1. **Chunking** — split a memory's content into bounded paragraph-aware
//!    chunks with character offsets and content hashes. See [`chunker`].
//! 2. **Embedding** — turn each chunk's text into a fixed-dimension f32
//!    vector via a tier-aware [`EmbeddingAdapter`]. v0.3.0 ships a
//!    `fastembed-rs` adapter that uses BGE small/base/large by tier.
//! 3. **Similarity** — cosine over `Vec<f32>`, MMR-aware aggregation for
//!    "related memories" search. See [`similarity`].
//!
//! Storage lives on the `memory_chunks` table (created in
//! `db/migrations.rs`). Vectors are encoded as little-endian f32 BLOBs
//! so the on-disk size is `4 × dim` bytes per chunk.

pub mod auto_tagger;
pub mod chunker;
pub mod fastembed_adapter;
pub mod similarity;

use async_trait::async_trait;

use crate::errors::app_error::AppResult;

/// Stable identifier for the embedding model that produced a vector.
/// Stored on `memory_chunks.embedding_model` so we can detect when a
/// model change invalidates an existing embedding without having to
/// re-chunk.
pub type EmbeddingModelId = &'static str;

/// One vector + the dimension it was produced at. f32 is enough — BGE
/// models emit normalized vectors and `f32` cosine is plenty precise
/// at our scale.
#[derive(Debug, Clone)]
pub struct EmbeddingVector {
    pub model: EmbeddingModelId,
    pub dim: u32,
    pub values: Vec<f32>,
}

impl EmbeddingVector {
    /// Encode as little-endian f32 bytes for SQLite BLOB storage.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.values.len() * 4);
        for value in &self.values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Decode an existing BLOB. Returns `None` if the buffer length
    /// isn't a clean multiple of 4 — that would indicate corruption,
    /// and the worker should re-embed rather than panic.
    pub fn from_bytes(model: EmbeddingModelId, bytes: &[u8]) -> Option<Self> {
        if bytes.len() % 4 != 0 {
            return None;
        }
        let dim = (bytes.len() / 4) as u32;
        let mut values = Vec::with_capacity(dim as usize);
        for chunk in bytes.chunks_exact(4) {
            let arr: [u8; 4] = chunk.try_into().ok()?;
            values.push(f32::from_le_bytes(arr));
        }
        Some(EmbeddingVector { model, dim, values })
    }
}

/// Engine abstraction so we can swap the underlying embedding library
/// (fastembed in v0.3.0; possibly mistralrs alongside the LLM later)
/// without touching the queue/worker/repository layers.
#[async_trait]
pub trait EmbeddingAdapter: Send + Sync {
    /// Stable identifier — `"bge-small-en-v1.5"`, `"bge-base-en-v1.5"`,
    /// `"bge-large-en-v1.5"`. Matches `EmbeddingModelId` strings stored
    /// on chunk rows.
    fn model_id(&self) -> EmbeddingModelId;

    /// Vector dimension. 384 for bge-small, 768 for bge-base, 1024 for
    /// bge-large.
    fn dim(&self) -> u32;

    /// True when the model file is present locally and the adapter is
    /// ready to embed. Adapters that download lazily can return `false`
    /// before the first download completes; the AI Settings UI surfaces
    /// this as "Download embedding model" rather than running implicit
    /// network calls.
    async fn is_ready(&self) -> bool;

    /// Trigger model download / setup if not already ready. Returns
    /// quickly when already-ready; otherwise blocks the caller until
    /// the model is on-disk + loaded.
    async fn prepare(&self) -> AppResult<()>;

    /// Embed a batch of texts. The adapter is free to chunk this
    /// internally if its underlying engine has a batch-size cap.
    /// Returns vectors in the same order as the input.
    async fn embed_batch(&self, texts: Vec<String>) -> AppResult<Vec<EmbeddingVector>>;
}
