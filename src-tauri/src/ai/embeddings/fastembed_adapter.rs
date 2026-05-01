//! `fastembed-rs` embedding adapter.
//!
//! v0.3.3 ships tier-aware embedding models:
//!
//! | Tier | Model              | Dim | Approx download | Approx RSS at use |
//! |------|--------------------|-----|-----------------|--------------------|
//! |  A   | bge-small-en-v1.5  | 384 |  ~30 MB         |  ~200 MB           |
//! |  B   | bge-base-en-v1.5   | 768 | ~110 MB         |  ~450 MB           |
//! |  C   | bge-base-en-v1.5   | 768 | ~110 MB         |  ~450 MB           |
//!
//! BGE-base is meaningfully better at near-misses on a 1k+ memory
//! library than BGE-small — our retrieval-quality work in v0.3.3
//! made the difference visible in Related and Ask Recall surfaces.
//! BGE-large (1024) was on the table but the marginal recall gain
//! doesn't justify a 1.3 GB download.
//!
//! Model files live in `<app_data>/models/embeddings/`. fastembed
//! downloads them from Hugging Face on first `prepare()` — that's the
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
use crate::ai::hardware::HardwareTier;
use crate::errors::app_error::{AppError, AppResult};

/// Stable id stored on `memory_chunks.embedding_model`. Used by the
/// worker to detect stale rows after a model upgrade and re-embed them.
///
/// v0.3.7 bumps the suffix to `+t` because the embedding *strategy*
/// changed (now enriches each chunk's text with the parent memory's
/// title and `topic_labels` before embedding). The model file is the
/// same — fastembed still pulls bge-base-en-v1.5 — but the vectors
/// produced live in a semantically different space than the
/// pre-v0.3.7 ones, so we mark them with a different id and let the
/// existing model-mismatch re-embed path catch the upgrade.
pub const MODEL_ID_SMALL: EmbeddingModelId = "bge-small-en-v1.5+t";
pub const MODEL_ID_BASE: EmbeddingModelId = "bge-base-en-v1.5+t";

pub const MODEL_DIM_SMALL: u32 = 384;
pub const MODEL_DIM_BASE: u32 = 768;

/// fastembed cache subdirectory under `app_data_dir()`. Files end up
/// at `<app_data>/models/embeddings/`.
const CACHE_SUBDIR: &str = "models/embeddings";

/// Pick the embedding model size to install based on the detected
/// hardware tier. Tier A (~8 GB RAM) gets the small model; B and C
/// get base. Override knobs land in a later release if real users
/// need them; the auto pick is the right default.
pub fn default_model_for_tier(tier: HardwareTier) -> (EmbeddingModelId, u32, EmbeddingModel) {
    match tier {
        HardwareTier::A => (MODEL_ID_SMALL, MODEL_DIM_SMALL, EmbeddingModel::BGESmallENV15),
        HardwareTier::B | HardwareTier::C => {
            (MODEL_ID_BASE, MODEL_DIM_BASE, EmbeddingModel::BGEBaseENV15)
        }
    }
}

pub struct FastembedAdapter {
    app: AppHandle,
    cell: OnceCell<Arc<TextEmbedding>>,
    model_id: EmbeddingModelId,
    model_dim: u32,
    fastembed_model: EmbeddingModel,
}

impl FastembedAdapter {
    /// Construct the adapter for a specific hardware tier. Pick is
    /// captured at scheduler init; switching tiers requires a restart
    /// (ok for v0.3.3 — the user-facing knob will land later).
    pub fn for_tier(app: AppHandle, tier: HardwareTier) -> Self {
        let (model_id, model_dim, fastembed_model) = default_model_for_tier(tier);
        Self {
            app,
            cell: OnceCell::new(),
            model_id,
            model_dim,
            fastembed_model,
        }
    }

    /// Backwards-compat constructor used by older callers; defaults
    /// to BGE-small. New code should use `for_tier` so the right
    /// model is picked for the host.
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            cell: OnceCell::new(),
            model_id: MODEL_ID_SMALL,
            model_dim: MODEL_DIM_SMALL,
            fastembed_model: EmbeddingModel::BGESmallENV15,
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
    /// aren't on-disk yet (~30 MB for small, ~110 MB for base);
    /// subsequent calls return the cached `Arc<TextEmbedding>`.
    async fn ensure_model(&self) -> AppResult<Arc<TextEmbedding>> {
        if let Some(model) = self.cell.get() {
            return Ok(model.clone());
        }
        let cache_dir = self.cache_dir()?;
        let fastembed_model = self.fastembed_model.clone();
        let model = tokio::task::spawn_blocking(move || {
            let opts = InitOptions::new(fastembed_model)
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
        self.model_id
    }

    fn dim(&self) -> u32 {
        self.model_dim
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
            if dim != self.model_dim {
                return Err(AppError::Invalid(format!(
                    "Unexpected embedding dim {dim} (expected {})",
                    self.model_dim
                )));
            }
            out.push(EmbeddingVector {
                model: self.model_id,
                dim,
                values,
            });
        }
        Ok(out)
    }
}
