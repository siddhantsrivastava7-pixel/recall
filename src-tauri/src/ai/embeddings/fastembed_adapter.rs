//! `fastembed-rs` embedding adapter.
//!
//! v0.3.0 ships a single model — BGE-small-en-v1.5 (384-dim, ~30MB
//! download) — for all hardware tiers. Tier-aware model selection
//! (BGE-base for B, BGE-large for C) is deferred to v0.3.1 because
//! switching models invalidates every existing embedding and we want
//! to bake "Related memories" on a single model first.
//!
//! Model file lives in `<app_data>/models/embeddings/`. fastembed
//! downloads it from Hugging Face on first `prepare()` — that's the
//! one-time, opt-in cloud call mentioned in the no-cloud-calls policy
//! header in `ai/mod.rs`. Once downloaded, every embed call is
//! offline and stays offline.
//!
//! Thread model: fastembed's `TextEmbedding::embed` is synchronous and
//! CPU-bound. We wrap calls in `spawn_blocking` so the worker pool
//! isn't held by the ONNX session during a 200ms inference.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;

use crate::ai::embeddings::{EmbeddingAdapter, EmbeddingModelId, EmbeddingVector};
use crate::errors::app_error::{AppError, AppResult};

/// Stable id stored on `memory_chunks.embedding_model`. When this
/// changes (e.g. v0.3.1 ships tier-aware models) the worker re-embeds
/// any chunk whose `embedding_model` doesn't match the current adapter.
pub const MODEL_ID: EmbeddingModelId = "bge-small-en-v1.5";

/// Vector dimension for BGE-small. Stored on each chunk so a future
/// adapter that emits a different dim can detect the mismatch and
/// trigger re-embedding without trusting the stored value.
pub const MODEL_DIM: u32 = 384;

/// fastembed cache subdirectory under `app_data_dir()`. Files end up
/// at `<app_data>/models/embeddings/`.
const CACHE_SUBDIR: &str = "models/embeddings";

pub struct FastembedAdapter {
    app: AppHandle,
    cell: OnceCell<Arc<TextEmbedding>>,
}

impl FastembedAdapter {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            cell: OnceCell::new(),
        }
    }

    fn cache_dir(&self) -> AppResult<PathBuf> {
        let base = self
            .app
            .path()
            .app_data_dir()
            .map_err(|err| AppError::Invalid(format!("app_data_dir unavailable: {err}")))?;
        let dir = base.join(CACHE_SUBDIR);
        std::fs::create_dir_all(&dir).map_err(|err| {
            AppError::Invalid(format!(
                "Failed to create model cache {}: {err}",
                dir.display()
            ))
        })?;
        Ok(dir)
    }

    /// Lazily instantiate the underlying `TextEmbedding`. The first
    /// call triggers the download from Hugging Face if the files
    /// aren't on-disk yet (~30MB once); subsequent calls return the
    /// cached `Arc<TextEmbedding>`.
    async fn ensure_model(&self) -> AppResult<Arc<TextEmbedding>> {
        if let Some(model) = self.cell.get() {
            return Ok(model.clone());
        }
        let cache_dir = self.cache_dir()?;
        let model = tokio::task::spawn_blocking(move || {
            let opts = InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_show_download_progress(false)
                .with_cache_dir(cache_dir);
            TextEmbedding::try_new(opts)
                .map_err(|err| AppError::Invalid(format!("fastembed init failed: {err}")))
        })
        .await
        .map_err(|err| AppError::Invalid(format!("Embedding init task panicked: {err}")))??;

        let arc = Arc::new(model);
        let _ = self.cell.set(arc.clone());
        Ok(arc)
    }
}

#[async_trait]
impl EmbeddingAdapter for FastembedAdapter {
    fn model_id(&self) -> EmbeddingModelId {
        MODEL_ID
    }

    fn dim(&self) -> u32 {
        MODEL_DIM
    }

    async fn is_ready(&self) -> bool {
        if self.cell.get().is_some() {
            return true;
        }
        // Heuristic: if the cache dir exists *and* contains a
        // recursively-readable file, fastembed will load from disk
        // without a network call. Files end up nested under a Hugging
        // Face repo subdirectory (`models--Qdrant--bge-small-en-v1.5/`),
        // so a top-level `next_entry` finding *something* is sufficient.
        let Ok(dir) = self.cache_dir() else {
            return false;
        };
        match tokio::fs::read_dir(&dir).await {
            Ok(mut entries) => entries.next_entry().await.ok().flatten().is_some(),
            Err(_) => false,
        }
    }

    async fn prepare(&self) -> AppResult<()> {
        // Force a model construction. If files aren't local fastembed
        // will fetch them; once present this is a fast no-op.
        self.ensure_model().await.map(|_| ())
    }

    async fn embed_batch(&self, texts: Vec<String>) -> AppResult<Vec<EmbeddingVector>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.ensure_model().await?;
        let embeddings = tokio::task::spawn_blocking(move || {
            model
                .embed(texts, None)
                .map_err(|err| AppError::Invalid(format!("fastembed embed failed: {err}")))
        })
        .await
        .map_err(|err| AppError::Invalid(format!("Embedding task panicked: {err}")))??;

        let mut out = Vec::with_capacity(embeddings.len());
        for values in embeddings {
            let dim = values.len() as u32;
            if dim != MODEL_DIM {
                return Err(AppError::Invalid(format!(
                    "Unexpected embedding dim {dim} (expected {MODEL_DIM})"
                )));
            }
            out.push(EmbeddingVector {
                model: MODEL_ID,
                dim,
                values,
            });
        }
        Ok(out)
    }
}
