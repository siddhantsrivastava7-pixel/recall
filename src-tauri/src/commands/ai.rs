//! Tauri commands exposing the AI subsystem to the frontend.
//!
//! Phase 1 (v0.2.0) ships exactly five commands — anything more would
//! drift past the locked PRD scope:
//!
//!   * [`ai_status`] — read-only status for the AI Settings tab
//!   * [`ai_set_enabled`] — master toggle
//!   * [`ai_set_mode`] — currently a thin wrapper around the master toggle
//!     (kept in the surface area so Phase 2's Lite/Smart/Pro mode picker
//!     doesn't need a rename later)
//!   * [`ocr_run_for_memory`] — manual "OCR this one memory now"
//!   * [`ocr_rebuild_index`] — re-enqueue OCR for every eligible memory
//!
//! All commands are no-ops when AI is disabled — except the toggles
//! themselves and `ai_status`, which always reads.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{
    ai::embeddings::similarity::{
        aggregate_with_mmr, cosine, subtract_centroid, MatchStrength, RelatedMemoryHit,
        ScoredChunk, SEMANTIC_FLOOR,
    },
    ai::embeddings::EmbeddingVector,
    ai::hardware::HardwareInfo,
    ai::llm::{registry as llm_registry, LlmGenerationRequest},
    ai::scheduler::SchedulerStatus,
    db::repositories::EmbeddingCoverage,
    errors::app_error::{AppError, AppResult},
    state::app_state::AppState,
};

/// Aggregate snapshot the AI Settings tab renders. Cheap to recompute on
/// every tab open — the queue counts query is one indexed SQL aggregate.
/// Send-only (never deserialized from the frontend) — so we don't pull
/// `Deserialize` here and avoid touching the inner types' derives.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusPayload {
    /// Master enabled flag (mirrors the persisted setting + scheduler
    /// in-memory atomic; both are kept in lock-step).
    pub enabled: bool,
    /// Detected hardware tier + RAM/cores readout for the Settings tab.
    pub hardware: HardwareInfo,
    /// Stable engine label (e.g. `"apple-vision"`, `"windows-media-ocr"`,
    /// `"unsupported"`). Persisted on `memories.ocr_engine`.
    pub ocr_engine: &'static str,
    /// Whether a native OCR engine is available on this host. When
    /// `false`, the master toggle still works but no OCR jobs will run.
    pub ocr_available: bool,
    /// v0.3.0: embedding model identifier. `"unsupported"` when no
    /// adapter is configured.
    pub embedding_model: &'static str,
    /// v0.3.0: whether the embedding model file is present on disk
    /// and ready to embed without a network call.
    pub embedding_ready: bool,
    /// v0.3.0: aggregate embed-coverage counts for the Settings tab.
    pub embedding_coverage: EmbeddingCoverage,
    /// Live queue counts. `running` is informational; `queued` and
    /// `failed` (terminal failures, attempts maxed) drive the UI badges.
    pub queue: SchedulerStatus,
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> AppResult<AiStatusPayload> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    let queue = scheduler.status_snapshot().await?;
    let hardware = scheduler.hardware().clone();
    let ocr_engine = scheduler.ocr_engine_label();
    let embedding_model = scheduler.embedding_model_label();
    let embedding_ready = scheduler.embedding_is_ready().await;
    let mut embedding_coverage = state.memory_repository.embedding_coverage().await?;
    if embedding_model != "unsupported" {
        embedding_coverage.embedded_chunks_active_model = state
            .memory_repository
            .count_embedded_chunks_for_model(embedding_model)
            .await?;
    }

    Ok(AiStatusPayload {
        enabled: scheduler.is_enabled(),
        hardware,
        ocr_engine,
        ocr_available: ocr_engine != "unsupported",
        embedding_model,
        embedding_ready,
        embedding_coverage,
        queue,
    })
}

#[tauri::command]
pub async fn ai_set_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<AiStatusPayload> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    // Persist the new flag on settings — single source of truth for
    // restart, with the in-memory atomic mirroring it for the worker
    // hot-path.
    let mut settings = state.settings_repository.get().await?;
    settings.ai_enabled = enabled;
    state.settings_repository.save(&settings).await?;

    scheduler.set_enabled(enabled);

    let queue = scheduler.status_snapshot().await?;
    let embedding_model = scheduler.embedding_model_label();
    let mut embedding_coverage = state.memory_repository.embedding_coverage().await?;
    if embedding_model != "unsupported" {
        embedding_coverage.embedded_chunks_active_model = state
            .memory_repository
            .count_embedded_chunks_for_model(embedding_model)
            .await?;
    }
    let embedding_ready = scheduler.embedding_is_ready().await;
    Ok(AiStatusPayload {
        enabled,
        hardware: scheduler.hardware().clone(),
        ocr_engine: scheduler.ocr_engine_label(),
        ocr_available: scheduler.ocr_engine_label() != "unsupported",
        embedding_model: scheduler.embedding_model_label(),
        embedding_ready,
        embedding_coverage,
        queue,
    })
}

/// AI mode is reserved for Phase 2's Lite/Smart/Pro picker. In Phase 1
/// we only ship `"off"` and `"on"` — anything else is rejected so we
/// don't accept values we have no intent to honor.
#[tauri::command]
pub async fn ai_set_mode(mode: String, state: State<'_, AppState>) -> AppResult<AiStatusPayload> {
    let normalized = mode.trim().to_ascii_lowercase();
    let enabled = match normalized.as_str() {
        "off" => false,
        "on" | "lite" | "smart" | "pro" => true,
        other => {
            return Err(AppError::Invalid(format!(
                "Unknown AI mode '{other}'. Allowed: off | on."
            )))
        }
    };
    ai_set_enabled(enabled, state).await
}

#[tauri::command]
pub async fn ocr_run_for_memory(
    memory_id: String,
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    if !scheduler.is_enabled() {
        return Err(AppError::Invalid(
            "Enable AI first to run OCR on individual memories.".into(),
        ));
    }
    scheduler.enqueue_ocr_for_memory(&memory_id).await
}

#[tauri::command]
pub async fn ocr_rebuild_index(state: State<'_, AppState>) -> AppResult<u64> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    if !scheduler.is_enabled() {
        return Err(AppError::Invalid(
            "Enable AI first to run an OCR rebuild.".into(),
        ));
    }
    scheduler.rebuild_ocr_index().await
}

/// Diagnostic snapshot of `clipboard.read_image()`. Used by the AI
/// Settings "Test clipboard image" button to surface, in one click, why
/// a copied screenshot might not be turning into a memory. Returns a
/// structured result so the UI can render the same shape regardless of
/// which branch hit (success / no image / error).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImageDiagnostic {
    /// `true` when `read_image()` returned a usable image with
    /// non-zero dimensions and a buffer length matching width × height × 4.
    pub ok: bool,
    /// Human-readable summary: `"Got 1920×1080 image (8.3 MB)"` on
    /// success, or the failure reason on the negative path.
    pub message: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: Option<u64>,
}

/// Trigger embedding model download. Idempotent: returns immediately
/// when files are already on disk. Surfaced via the "Download embedding
/// model" button in Settings → AI. After the download completes we
/// also reset any failed embed jobs so they retry against the now-
/// available model — covers the case where embed jobs were enqueued
/// before the download finished.
#[tauri::command]
pub async fn ai_download_embedding_model(state: State<'_, AppState>) -> AppResult<bool> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;
    scheduler.prepare_embedding_model().await?;
    let _ = scheduler.reset_failed_embed_jobs().await;
    scheduler.wake_workers();
    Ok(true)
}

/// Backfill summary returned to the UI after `embed_all_memories`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedAllSummary {
    pub memories_scanned: u64,
    pub memories_chunked: u64,
    pub chunks_created: u64,
    pub chunks_enqueued: u64,
    pub failed_jobs_reset: u64,
}

/// Iterate every memory, run the chunker, hash-aware-replace into
/// `memory_chunks`, and enqueue embed jobs for any chunks without
/// vectors yet. Also resets failed `embed_chunk` queue rows so they
/// retry. Idempotent — running twice is safe; nothing changes for
/// memories whose chunks haven't drifted.
///
/// This is what makes the v0.3.0 release useful for users who already
/// had memories before upgrading: their pre-existing rows never went
/// through `capture_service.create()` so they have zero chunks. Until
/// we backfill, "Related memories" has nothing to relate against.
#[tauri::command]
pub async fn embed_all_memories(state: State<'_, AppState>) -> AppResult<EmbedAllSummary> {
    use crate::ai::embeddings::chunker;
    use crate::db::repositories::ChunkUpsert;

    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;
    if !scheduler.is_enabled() {
        return Err(AppError::Invalid(
            "Enable AI first to run an embedding rebuild.".into(),
        ));
    }
    if scheduler.embedding_model_label() == "unsupported" {
        return Err(AppError::Invalid(
            "No embedding adapter is configured on this host.".into(),
        ));
    }

    // Reset any stuck failed/running jobs first so they don't block
    // the dedupe_key for genuinely-new enqueues.
    let failed_jobs_reset = scheduler.reset_failed_embed_jobs().await?;

    let memories = state.memory_service.list().await?;
    let mut summary = EmbedAllSummary {
        memories_scanned: memories.len() as u64,
        memories_chunked: 0,
        chunks_created: 0,
        chunks_enqueued: 0,
        failed_jobs_reset,
    };

    for memory in memories {
        // Skip placeholder bodies — screenshots whose OCR hasn't
        // landed yet shouldn't get embedded against the placeholder
        // text. The OCR worker re-fires the chunk-and-embed flow
        // after promote_ocr_to_content runs.
        let content = memory.content.trim();
        if content.is_empty() {
            continue;
        }
        if memory.source_app.as_deref() == Some("screenshot")
            && content.starts_with("Screenshot from clipboard")
            && content.contains("OCR will fill in")
        {
            continue;
        }

        // v0.3.7: backfill auto-tags for existing memories. Detected
        // tags get merged into topic_labels (preserves any tags from
        // prior link-enrichment / classifier passes). The embedding
        // worker reads tags + title at embed time to build the
        // enriched text, so this is the only place tag detection
        // needs to happen for existing memories.
        let detected_tags = crate::ai::embeddings::auto_tagger::detect_tags(&memory.content);
        let tags = state
            .memory_repository
            .merge_topic_labels(&memory.id, &detected_tags)
            .await
            .unwrap_or_default();

        let mut chunks = chunker::chunk_text(&memory.content);
        if chunks.is_empty() {
            continue;
        }

        // Match the capture-hook hash semantics: each chunk's
        // content_hash reflects the *enriched* embedding text
        // (title + tags + chunk text), not the raw chunk text.
        for chunk in &mut chunks {
            let enriched = crate::ai::embeddings::auto_tagger::enriched_embedding_text(
                memory.title.as_deref(),
                &tags,
                &chunk.text,
            );
            chunk.content_hash = chunker::fnv1a_64_hex(&enriched);
        }

        let upserts: Vec<ChunkUpsert<'_>> = chunks
            .iter()
            .enumerate()
            .map(|(idx, c)| ChunkUpsert {
                chunk_index: idx,
                text: &c.text,
                start_offset: c.start_offset,
                end_offset: c.end_offset,
                byte_size: c.byte_size(),
                token_estimate: c.token_estimate(),
                content_hash: &c.content_hash,
            })
            .collect();

        let needs_embedding = state
            .memory_repository
            .replace_chunks_hash_aware(
                &memory.id,
                &upserts,
                Some(scheduler.embedding_model_label()),
            )
            .await?;

        summary.memories_chunked += 1;
        summary.chunks_created += chunks.len() as u64;
        for chunk_id in needs_embedding {
            if scheduler.enqueue_embed_chunk(&chunk_id, &memory.id).await? {
                summary.chunks_enqueued += 1;
            }
        }
    }

    Ok(summary)
}

/// One memory in a related-memory result list. The chunk fields point
/// at the best-matching slice within the parent memory so the UI can
/// render an excerpt with offsets to highlight. v0.3.3 replaces the
/// raw "% match" with a coarse strength bucket — raw cosine is too
/// misleading to display as a probability.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedMemoryView {
    pub memory_id: String,
    /// Centered cosine score; useful for the UI to sort or threshold
    /// further but **not** for human display. Use `strength` for
    /// labels.
    pub score: f32,
    pub strength: MatchStrength,
    pub chunk_text: String,
    pub chunk_start: i64,
    pub chunk_end: i64,
}

/// Return up to `limit` related memories for the given source memory,
/// ranked by mean-of-top-2 chunk similarity with MMR diversity.
///
/// v0.3.3 changes:
///   * Filters retrieval to chunks embedded under the *active* model
///     (`embedding_model = ?adapter`) so a mid-upgrade DB with mixed
///     small/base namespaces never produces dim-mismatched cosine.
///   * Subtracts the corpus centroid from both source and candidate
///     vectors before cosine. Drops the BGE "English baseline" floor
///     of ~0.65–0.85 that was making everything look highly related.
///   * Drops candidates below `SEMANTIC_FLOOR` (centered cosine 0.15)
///     so the panel surfaces "no related memories" instead of a list
///     of weak hits.
#[tauri::command]
pub async fn find_related(
    memory_id: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<Vec<RelatedMemoryView>> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    let model_label = scheduler.embedding_model_label();
    if model_label == "unsupported" {
        return Ok(Vec::new());
    }

    let top_n = limit.unwrap_or(5).max(1) as usize;

    let centroid = scheduler
        .corpus_centroid(&state.memory_repository)
        .await?;

    // Source memory's chunks for the active model only — chunks
    // still embedded under the old model are stale and would mix
    // dimensions if we used them.
    let source_chunks = state
        .memory_repository
        .list_chunks_for_memory(&memory_id)
        .await?;
    let source_vectors: Vec<Vec<f32>> = source_chunks
        .iter()
        .filter_map(|chunk| {
            if chunk.embedding_model.as_deref() != Some(model_label) {
                return None;
            }
            let bytes = chunk.embedding_vector.as_ref()?;
            let v = EmbeddingVector::from_bytes(model_label, bytes)?.values;
            Some(maybe_center(v, centroid.as_deref()))
        })
        .collect();
    if source_vectors.is_empty() {
        return Ok(Vec::new());
    }

    // Active-model chunks across the whole corpus. Excludes
    // dim-mismatched rows by construction.
    let all_chunks = state
        .memory_repository
        .list_embedded_chunks_for_model(model_label)
        .await?;

    let mut scored: Vec<ScoredChunk> = Vec::with_capacity(all_chunks.len());
    let mut chunk_vectors: HashMap<String, Vec<f32>> = HashMap::new();

    for chunk in &all_chunks {
        if chunk.memory_id == memory_id {
            continue;
        }
        let Some(bytes) = &chunk.embedding_vector else {
            continue;
        };
        let Some(vec) = EmbeddingVector::from_bytes(model_label, bytes) else {
            continue;
        };
        let centered = maybe_center(vec.values, centroid.as_deref());

        let max_sim = source_vectors
            .iter()
            .map(|src| cosine(src, &centered))
            .fold(f32::NEG_INFINITY, f32::max);

        if max_sim < SEMANTIC_FLOOR {
            continue;
        }

        chunk_vectors.insert(chunk.id.clone(), centered);
        scored.push(ScoredChunk {
            chunk_id: chunk.id.clone(),
            memory_id: chunk.memory_id.clone(),
            start_offset: chunk.start_offset,
            end_offset: chunk.end_offset,
            text: chunk.text.clone(),
            score: max_sim,
        });
    }

    let hits: Vec<RelatedMemoryHit> =
        aggregate_with_mmr(scored, &chunk_vectors, &memory_id, top_n);

    Ok(hits
        .into_iter()
        .filter(|h| h.score >= SEMANTIC_FLOOR)
        .map(|h| RelatedMemoryView {
            memory_id: h.memory_id,
            score: h.score,
            strength: MatchStrength::from_centered_cosine(h.score),
            chunk_text: h.best_chunk.text,
            chunk_start: h.best_chunk.start_offset,
            chunk_end: h.best_chunk.end_offset,
        })
        .collect())
}

fn maybe_center(values: Vec<f32>, centroid: Option<&[f32]>) -> Vec<f32> {
    match centroid {
        Some(c) => subtract_centroid(values, c),
        None => values,
    }
}

/// One row in the blended search result list. `keyword_score` and
/// `semantic_score` are returned alongside the blended `score` for
/// transparency / debugging — frontend ranks on `score`, displays the
/// `strength` label from the cosine side only.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchHit {
    pub memory_id: String,
    /// Blended ranking score in [0, 1]. Used for ordering only.
    pub score: f32,
    /// Centered cosine, in approximately [-0.2, 1.0]. Source of
    /// `strength`; useful for tuning thresholds in the future.
    pub semantic_score: f32,
    /// Keyword path's contribution before blending, in [0, 1].
    pub keyword_score: f32,
    pub strength: MatchStrength,
    pub chunk_text: String,
    pub chunk_start: i64,
    pub chunk_end: i64,
}

/// Hybrid keyword + semantic search. v0.3.3.
///
///   * Embed the query string with the active adapter.
///   * Cosine across all active-model chunks (mean-centered).
///   * MMR-aggregate to memory level (mean-of-top-2 + λ=0.7 diversity).
///   * Compute a per-memory keyword score (substring + word overlap
///     with title/content/note/source-app fields, normalized).
///   * Blend at 0.5 / 0.5 (keyword / semantic); both sides clamped to
///     [0, 1] so the blend is well-defined regardless of underlying
///     score ranges.
///   * Filter out blended results below SEMANTIC_FLOOR on the
///     semantic side — keyword-only matches still rank, but a row
///     with zero topical signal and a thin keyword hit shouldn't
///     promote past a strongly-related-but-keyword-poor memory.
///
/// Returns up to `limit` results (default 12) ordered by blended
/// score descending.
#[tauri::command]
pub async fn semantic_search(
    query: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SemanticSearchHit>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;
    let model_label = scheduler.embedding_model_label();
    if model_label == "unsupported" {
        return Ok(Vec::new());
    }

    let top_n = limit.unwrap_or(12).max(1) as usize;

    // 1. Embed the query.
    let Some(query_embedding) = scheduler.embed_query(trimmed).await? else {
        // Adapter not ready yet — return empty so the caller falls
        // back to keyword-only search.
        return Ok(Vec::new());
    };
    let centroid = scheduler
        .corpus_centroid(&state.memory_repository)
        .await?;
    let query_vec = maybe_center(query_embedding.values, centroid.as_deref());

    // 2. Cosine across active-model chunks.
    let all_chunks = state
        .memory_repository
        .list_embedded_chunks_for_model(model_label)
        .await?;
    let mut scored: Vec<ScoredChunk> = Vec::with_capacity(all_chunks.len());
    let mut chunk_vectors: HashMap<String, Vec<f32>> = HashMap::new();
    for chunk in &all_chunks {
        let Some(bytes) = &chunk.embedding_vector else {
            continue;
        };
        let Some(v) = EmbeddingVector::from_bytes(model_label, bytes) else {
            continue;
        };
        let centered = maybe_center(v.values, centroid.as_deref());
        let sim = cosine(&query_vec, &centered);
        if sim < SEMANTIC_FLOOR {
            continue;
        }
        chunk_vectors.insert(chunk.id.clone(), centered);
        scored.push(ScoredChunk {
            chunk_id: chunk.id.clone(),
            memory_id: chunk.memory_id.clone(),
            start_offset: chunk.start_offset,
            end_offset: chunk.end_offset,
            text: chunk.text.clone(),
            score: sim,
        });
    }

    // 3. MMR-aggregate to memory level. Use a sentinel "src" id since
    // there's no source memory to exclude — the query is the source.
    let semantic_hits = aggregate_with_mmr(scored, &chunk_vectors, "::query::", top_n * 3);

    // 4. Compute a keyword score per matched memory. We score a memory
    // by counting query tokens that appear in its title/content/note/
    // source_app, then normalizing to [0, 1].
    let query_tokens = tokenize_query(trimmed);
    let mut blended: Vec<SemanticSearchHit> = Vec::with_capacity(semantic_hits.len());
    for hit in semantic_hits {
        let memory = match state.memory_repository.find(&hit.memory_id).await? {
            Some(m) => m,
            None => continue,
        };
        let kw = keyword_score(&memory, &query_tokens);
        let semantic = hit.score.clamp(0.0, 1.0);
        let blended_score = 0.5 * kw + 0.5 * semantic;
        blended.push(SemanticSearchHit {
            memory_id: hit.memory_id,
            score: blended_score,
            semantic_score: hit.score,
            keyword_score: kw,
            strength: MatchStrength::from_centered_cosine(hit.score),
            chunk_text: hit.best_chunk.text,
            chunk_start: hit.best_chunk.start_offset,
            chunk_end: hit.best_chunk.end_offset,
        });
    }

    blended.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    blended.truncate(top_n);
    Ok(blended)
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_string())
        .collect()
}

/// Keyword score in [0, 1]: fraction of query tokens that appear
/// somewhere in the memory's textual fields. Weighted slightly toward
/// title hits but kept simple — this score is one input to the blend,
/// not the whole story.
fn keyword_score(memory: &crate::models::Memory, query_tokens: &[String]) -> f32 {
    if query_tokens.is_empty() {
        return 0.0;
    }
    let title = memory.title.as_deref().unwrap_or("").to_lowercase();
    let content = memory.content.to_lowercase();
    let note = memory.note.as_deref().unwrap_or("").to_lowercase();
    let source_app = memory.source_app.as_deref().unwrap_or("").to_lowercase();
    let summary = memory.summary_text.as_deref().unwrap_or("").to_lowercase();

    let mut hits = 0_f32;
    for token in query_tokens {
        let in_title = title.contains(token);
        let in_others =
            content.contains(token) || note.contains(token) || summary.contains(token)
                || source_app.contains(token);
        if in_title {
            hits += 1.0;
        } else if in_others {
            hits += 0.7;
        }
    }
    let max_possible = query_tokens.len() as f32;
    (hits / max_possible).min(1.0)
}

#[tauri::command]
pub async fn ai_diagnose_clipboard_image(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ClipboardImageDiagnostic> {
    let result = state.platform.clipboard.read_image(&app).await;
    Ok(match result {
        Ok(Some(image)) => {
            let bytes = image.rgba.len() as u64;
            let mb = (bytes as f64) / (1024.0 * 1024.0);
            ClipboardImageDiagnostic {
                ok: true,
                message: format!(
                    "Got {}×{} image ({:.1} MB RGBA). Copy a screenshot, click again, and you should see a new memory appear.",
                    image.width, image.height, mb
                ),
                width: Some(image.width),
                height: Some(image.height),
                byte_size: Some(bytes),
            }
        }
        Ok(None) => ClipboardImageDiagnostic {
            ok: false,
            message: "No image on the clipboard, or the format isn't decodable. Copy an image (Win+Shift+S, Cmd+Shift+4, or right-click an image → Copy) and click again.".into(),
            width: None,
            height: None,
            byte_size: None,
        },
        Err(error) => ClipboardImageDiagnostic {
            ok: false,
            message: format!("Clipboard read failed: {error}"),
            width: None,
            height: None,
            byte_size: None,
        },
    })
}

// ─── v0.4.0: Ask Recall LLM commands ────────────────────────────────

/// Read-only readout of the LLM model the user would get for their
/// hardware tier. Used by AI Settings to render the Ask Recall
/// section even before any download happens.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmStatusPayload {
    pub model_id: String,
    pub hf_repo: String,
    pub approx_download_mb: u64,
    pub approx_inference_ram_mb: u64,
    pub context_window_tokens: u64,
    /// True when model + tokenizer files are on disk and the
    /// adapter can run inference without a network call.
    pub ready: bool,
}

#[tauri::command]
pub async fn ai_llm_status(state: State<'_, AppState>) -> AppResult<LlmStatusPayload> {
    let adapter = state
        .llm_adapter()
        .ok_or_else(|| AppError::Invalid("LLM adapter not configured on this host.".into()))?;

    let entry = llm_registry::entry_by_id(adapter.model_id())
        .ok_or_else(|| AppError::Invalid("LLM model id not found in registry.".into()))?;

    Ok(LlmStatusPayload {
        model_id: adapter.model_id().to_string(),
        hf_repo: adapter.hf_repo().to_string(),
        approx_download_mb: entry.approx_download_mb,
        approx_inference_ram_mb: entry.approx_inference_ram_mb,
        context_window_tokens: entry.context_window_tokens as u64,
        ready: adapter.is_ready().await,
    })
}

/// Trigger the model + tokenizer download. Idempotent; returns
/// quickly when files are already on disk.
#[tauri::command]
pub async fn ai_download_llm(state: State<'_, AppState>) -> AppResult<bool> {
    let adapter = state
        .llm_adapter()
        .ok_or_else(|| AppError::Invalid("LLM adapter not configured on this host.".into()))?;
    adapter.prepare().await?;
    Ok(true)
}

/// Drop loaded weights from RAM. Next generation will lazily reload.
#[tauri::command]
pub async fn ai_unload_llm(state: State<'_, AppState>) -> AppResult<bool> {
    let adapter = state
        .llm_adapter()
        .ok_or_else(|| AppError::Invalid("LLM adapter not configured on this host.".into()))?;
    adapter.unload().await?;
    Ok(true)
}

/// v0.4.0a smoke test. Runs a fixed prompt end-to-end through the
/// adapter to verify download + load + inference all work on this
/// host, before we build the full Ask Recall pipeline on top.
/// Returns the generated text and timing so AI Settings can show
/// "model returned 87 tokens in 4.2s — looking good."
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmDiagnosticPayload {
    pub ok: bool,
    pub model_id: String,
    pub prompt: String,
    pub response: String,
    pub tokens_generated: u32,
    pub latency_ms: u64,
    pub message: String,
}

#[tauri::command]
pub async fn ai_diagnose_llm(state: State<'_, AppState>) -> AppResult<LlmDiagnosticPayload> {
    let adapter = state
        .llm_adapter()
        .ok_or_else(|| AppError::Invalid("LLM adapter not configured on this host.".into()))?;
    let model_id = adapter.model_id().to_string();

    let prompt = "In one short sentence, what is Recall (the local-first memory app)?".to_string();
    let request = LlmGenerationRequest {
        prompt: prompt.clone(),
        max_tokens: 80,
        temperature: 0.0,
    };

    match adapter.generate(request).await {
        Ok(resp) => Ok(LlmDiagnosticPayload {
            ok: true,
            model_id,
            prompt,
            response: resp.text,
            tokens_generated: resp.tokens_generated,
            latency_ms: resp.latency_ms,
            message: format!(
                "Model returned {} tokens in {}ms.",
                resp.tokens_generated, resp.latency_ms
            ),
        }),
        Err(error) => Ok(LlmDiagnosticPayload {
            ok: false,
            model_id,
            prompt,
            response: String::new(),
            tokens_generated: 0,
            latency_ms: 0,
            message: error.to_string(),
        }),
    }
}
