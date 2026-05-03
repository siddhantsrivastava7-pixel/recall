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
use tauri::{AppHandle, Emitter, State};

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

        // v0.5.6: also re-extract structured entities. Backfill is
        // exactly the right moment to refresh entity rows alongside
        // the embedding refresh — both use the same content as
        // their input. We pass an empty projects slice for v0.5.6;
        // v0.5.7 will plumb through projects so project-name
        // detection works in the backfill path too.
        let _ = crate::ai::entities::extract_and_persist(
            &state.memory_repository,
            &memory.id,
            &memory.content,
            &[],
        )
        .await;

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
    // Public surface — preserves v0.4.4 behavior with MMR diversity
    // enabled (good for general search where the user doesn't want
    // 5 near-duplicate results).
    semantic_search_internal(&query, limit, true, &state).await
}

/// Internal helper that powers both the public `semantic_search`
/// Tauri command and Ask Recall's retrieval path.
///
/// `mmr_enabled = true` (general search): MMR re-ranking penalizes
/// candidates too similar to already-picked items. Avoids surfacing
/// 5 copies of the same article when the user asks for "tauri builds".
///
/// `mmr_enabled = false` (Ask Recall): pure-relevance ranking.
/// Diversity is the LLM's job, not the retriever's; for an
/// enumeration question like "what license keys did I save", we
/// want all matching memories even though they're near-identical.
/// (v0.5.4: with MMR on, the 2nd and 3rd license-key memories were
/// dropped below the top-K cutoff because their embeddings were
/// near-clones of the 1st — wrong behavior for Ask Recall.)
pub(crate) async fn semantic_search_internal(
    query: &str,
    limit: Option<u32>,
    mmr_enabled: bool,
    state: &State<'_, AppState>,
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

    // 2. Cosine across active-model chunks. Self-capture screenshots
    //    (v0.5.6) are excluded — these are screenshots of Recall's
    //    own UI that the user took, OCR'd into chunks, and would
    //    otherwise pollute retrieval by surfacing previous answers
    //    as "matches" for similar future questions.
    let all_chunks = state
        .memory_repository
        .list_embedded_chunks_for_model(model_label)
        .await?;
    let mut self_capture_memory_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for memory in state.memory_repository.list().await? {
        if memory
            .ocr_engine
            .as_deref()
            .map(|e| e.contains("self-capture"))
            .unwrap_or(false)
        {
            self_capture_memory_ids.insert(memory.id);
        }
    }
    let mut scored: Vec<ScoredChunk> = Vec::with_capacity(all_chunks.len());
    let mut chunk_vectors: HashMap<String, Vec<f32>> = HashMap::new();
    for chunk in &all_chunks {
        if self_capture_memory_ids.contains(&chunk.memory_id) {
            continue;
        }
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

    // 3. Aggregate chunks → memories. With MMR for general search,
    // pure relevance ranking for Ask Recall.
    let semantic_hits = if mmr_enabled {
        aggregate_with_mmr(scored, &chunk_vectors, "::query::", top_n * 3)
    } else {
        aggregate_by_relevance(scored, top_n * 3)
    };

    // 4. Compute a keyword score per matched memory. We score a memory
    // by counting query tokens that appear in its title/content/note/
    // source_app/topic_labels, then normalizing to [0, 1].
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

/// Pure-relevance memory aggregation: group chunks by memory, take
/// each memory's mean of top-K chunks as its score, sort, truncate.
/// No diversity penalty — the caller (Ask Recall) wants enumeration
/// of ALL relevant memories, even when they're near-duplicates.
fn aggregate_by_relevance(
    scored_chunks: Vec<ScoredChunk>,
    top_n: usize,
) -> Vec<RelatedMemoryHit> {
    if top_n == 0 || scored_chunks.is_empty() {
        return Vec::new();
    }
    const TOP_K_CHUNKS_PER_MEMORY: usize = 2;
    let mut by_memory: HashMap<String, Vec<ScoredChunk>> = HashMap::new();
    for chunk in scored_chunks {
        by_memory.entry(chunk.memory_id.clone()).or_default().push(chunk);
    }
    let mut hits: Vec<RelatedMemoryHit> = by_memory
        .into_iter()
        .filter_map(|(memory_id, mut chunks)| {
            if chunks.is_empty() {
                return None;
            }
            chunks.sort_by(|a, b| {
                b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
            });
            let take = chunks.len().min(TOP_K_CHUNKS_PER_MEMORY);
            let mean = chunks[..take].iter().map(|c| c.score).sum::<f32>() / take as f32;
            let best_chunk = chunks.into_iter().next().unwrap();
            Some(RelatedMemoryHit {
                memory_id,
                score: mean,
                best_chunk,
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(top_n);
    hits
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
    // v0.5.3: include topic_labels (auto-tags from v0.3.7) in the
    // keyword scan. Memories with opaque content like license keys,
    // hashes, or URLs get tagged by the auto-tagger but their actual
    // text doesn't contain the human phrase ("license key"). Without
    // this, a query for "license key" scores 0 against a memory
    // tagged `license-key` whose body is `RECALL-7K2P-XYZW-...`.
    // Matched as title-strength because tags are explicit signals,
    // not incidental substrings — they were applied because the
    // memory IS one of these things.
    let topic_labels: Vec<String> = memory
        .topic_labels
        .as_ref()
        .map(|json| {
            json.0
                .iter()
                .map(|s| s.to_lowercase().replace('-', " "))
                .collect()
        })
        .unwrap_or_default();

    let mut hits = 0_f32;
    for token in query_tokens {
        let in_title = title.contains(token);
        let in_topic = topic_labels.iter().any(|t| t.contains(token));
        let in_others =
            content.contains(token) || note.contains(token) || summary.contains(token)
                || source_app.contains(token);
        if in_title || in_topic {
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
        pre_formatted: false,
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

// ─── v0.5.8: manual scrub for diagnostics + recovery ─────────────

/// Result of `ai_force_scrub`. v0.5.9 expands this to include
/// per-tag before/after counts so the UI can show definitive
/// "is the scrub doing anything" diagnostics — v0.5.8's report
/// said 1262/1262 successful but 12 license-key tags persisted
/// in the DB, which the bare counter couldn't surface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrubResult {
    pub memories_scanned: u32,
    pub tag_rows_updated: u32,
    pub self_captures_marked: u32,
    pub entities_extracted: u32,
    pub errors: u32,
    pub elapsed_ms: u64,
    /// v0.5.9: bulk-SQL purge counter — number of memory rows
    /// whose `topic_labels` array was actually rewritten by the
    /// brute-force `purge_managed_topic_labels` pass. Distinct
    /// from `tag_rows_updated` (which counts per-memory replace
    /// calls regardless of whether they wrote).
    pub bulk_purge_rows_affected: u64,
    /// Per-managed-tag count BEFORE the scrub — tags currently
    /// applied across the library. Tag string → count.
    pub before_counts: HashMap<String, u32>,
    /// Per-managed-tag count AFTER the scrub.
    pub after_counts: HashMap<String, u32>,
}

/// Force-run the v0.5.7 backfill regardless of the persisted flag.
/// Useful when:
///   * v0.5.7's auto-backfill silently failed (silent eprintln on
///     Windows GUI builds means errors disappear into the void)
///   * The user upgraded across multiple versions and the
///     incremental flags don't capture every needed pass
///   * Diagnostic — confirms the scrub logic actually does what
///     we claim it does
///
/// Side effect: also resets the `ai_v0_5_7_backfill_done` flag if
/// it was unset, so subsequent launches don't re-run automatically.
#[tauri::command]
pub async fn ai_force_scrub(state: State<'_, AppState>) -> AppResult<ScrubResult> {
    use crate::ai::embeddings::auto_tagger;
    use crate::ai::entities;
    use crate::ai::scheduler::worker;

    let started_at = std::time::Instant::now();

    // ─── 0. BEFORE audit ─────────────────────────────────────────
    // Count how many memories carry each managed tag right now.
    // Exposed in the result so the user can see definitively
    // whether the scrub removed entries from the DB.
    let mut before_counts: HashMap<String, u32> = HashMap::new();
    for tag in auto_tagger::MANAGED_TAGS {
        let n = state
            .memory_repository
            .count_memories_by_topic_label(tag)
            .await
            .unwrap_or(0);
        before_counts.insert((*tag).to_string(), n as u32);
    }

    // ─── 1. Bulk SQL purge ───────────────────────────────────────
    // Single UPDATE that strips every managed tag from every
    // memory's `topic_labels` array. Bypasses the per-memory
    // replace_auto_tagger_tags path entirely so any subtle
    // serde/sqlx issue with that path can't prevent the scrub.
    // Idempotent — running twice produces the same end state.
    let bulk_purge_rows_affected = state
        .memory_repository
        .purge_managed_topic_labels(auto_tagger::MANAGED_TAGS)
        .await
        .unwrap_or_else(|err| {
            eprintln!("[recall][force-scrub] bulk purge failed: {err}");
            0
        });

    // After bulk purge, every memory's topic_labels has zero
    // managed-tag values. The per-memory loop below re-adds the
    // freshly-detected ones so URL/email/etc. tags are properly
    // applied for content that legitimately has them.

    let memories = state.memory_repository.list().await?;
    let total = memories.len() as u32;

    let mut tag_rows_updated: u32 = 0;
    let mut self_captures_marked: u32 = 0;
    let mut entities_extracted: u32 = 0;
    let mut errors: u32 = 0;

    for memory in &memories {
        // v0.5.10: detect self-capture FIRST so we can suppress
        // auto-tagging on those memories. Otherwise the auto-
        // tagger sees OCR'd Ask Recall UI text containing
        // license-key-shape strings ("RC-TRIAL-..." in the
        // answer panel) and re-tags the screenshot — defeating
        // the self-capture filter we already have at retrieval.
        let already_flagged = memory
            .ocr_engine
            .as_deref()
            .map(|e| e.contains("self-capture"))
            .unwrap_or(false);
        let detected_self_capture_now = !already_flagged
            && memory
                .ocr_text
                .as_deref()
                .map(worker::is_recall_self_capture_text)
                .unwrap_or(false);
        let is_self_capture = already_flagged || detected_self_capture_now;

        // Auto-tagger detection: empty for self-captures so they
        // don't carry license-key tags from OCR'd UI content.
        let detected_tags: Vec<&'static str> = if is_self_capture {
            Vec::new()
        } else {
            auto_tagger::detect_tags(&memory.content)
        };

        match state
            .memory_repository
            .replace_auto_tagger_tags(&memory.id, auto_tagger::MANAGED_TAGS, &detected_tags)
            .await
        {
            Ok(next_tags) => {
                let _ = next_tags;
                tag_rows_updated += 1;
            }
            Err(err) => {
                eprintln!(
                    "[recall][force-scrub] replace_auto_tagger_tags failed for {}: {err}",
                    memory.id
                );
                errors += 1;
            }
        }

        // 2. Self-capture re-flag for newly-detected self-captures.
        if let Some(ocr_text) = memory.ocr_text.as_deref() {
            if detected_self_capture_now {
                let new_engine = format!(
                    "{}+self-capture",
                    memory.ocr_engine.as_deref().unwrap_or("unknown")
                );
                match state
                    .memory_repository
                    .set_ocr_status(
                        &memory.id,
                        memory.ocr_status.as_deref().unwrap_or("done"),
                        Some(ocr_text),
                        Some(&new_engine),
                        memory.ocr_processed_at.as_deref(),
                    )
                    .await
                {
                    Ok(_) => self_captures_marked += 1,
                    Err(err) => {
                        eprintln!(
                            "[recall][force-scrub] self-capture flag failed for {}: {err}",
                            memory.id
                        );
                        errors += 1;
                    }
                }
            }
        }

        // 3. Re-extract entities. Idempotent.
        match entities::extract_and_persist(
            &state.memory_repository,
            &memory.id,
            &memory.content,
            &[],
        )
        .await
        {
            Ok(count) if count > 0 => entities_extracted += 1,
            Ok(_) => {}
            Err(_) => errors += 1,
        }
    }

    // Mark the v0.5.7 flag so the boot-time auto-backfill stops
    // re-running on subsequent launches. (If it was already true,
    // this is a no-op.)
    let mut current = state.settings_service.get().await.unwrap_or_default();
    current.ai_v0_5_7_backfill_done = Some(true);
    current.ai_v0_5_6_backfill_done = Some(true);
    let _ = state.settings_service.save(&current).await;

    // ─── AFTER audit ─────────────────────────────────────────────
    let mut after_counts: HashMap<String, u32> = HashMap::new();
    for tag in auto_tagger::MANAGED_TAGS {
        let n = state
            .memory_repository
            .count_memories_by_topic_label(tag)
            .await
            .unwrap_or(0);
        after_counts.insert((*tag).to_string(), n as u32);
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    eprintln!(
        "[recall][force-scrub] complete: scanned={total} tag_rows={tag_rows_updated} bulk_purge={bulk_purge_rows_affected} self_captures={self_captures_marked} entities={entities_extracted} errors={errors} elapsed_ms={elapsed_ms}"
    );
    eprintln!("[recall][force-scrub] before: {before_counts:?}");
    eprintln!("[recall][force-scrub] after:  {after_counts:?}");

    Ok(ScrubResult {
        memories_scanned: total,
        tag_rows_updated,
        self_captures_marked,
        entities_extracted,
        errors,
        elapsed_ms,
        bulk_purge_rows_affected,
        before_counts,
        after_counts,
    })
}

// ─── v0.5.6: structured entities ──────────────────────────────────

/// List structured entities (people, companies, products,
/// projects, time ranges) extracted from a single memory.
/// Memory-detail UI uses this to render entity chips below the
/// content.
#[tauri::command]
pub async fn list_entities_for_memory(
    memory_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::MemoryEntityRow>> {
    state
        .memory_repository
        .list_entities_for_memory(&memory_id)
        .await
}

/// List every memory whose entity rows include the given
/// (entity_type, entity_value) pair. Used by entity-pivot
/// retrieval surfaces — "show all memories about Anthropic"
/// resolves to `list_memories_by_entity("company", "Anthropic")`.
#[tauri::command]
pub async fn list_memories_by_entity(
    entity_type: String,
    entity_value: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::Memory>> {
    state
        .memory_repository
        .list_memories_by_entity(&entity_type, &entity_value)
        .await
}

// ─── v0.4.3: Ask Recall RAG pipeline ────────────────────────────────

/// One memory cited in an Ask Recall answer. Resolves
/// `[memory:<uuid>]` markers in the LLM output back to the underlying
/// memory + the chunk that fed the retrieval (so the UI can highlight
/// the cited passage on click).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallCitation {
    pub memory_id: String,
    pub title: Option<String>,
    pub chunk_text: String,
    pub chunk_start: i64,
    pub chunk_end: i64,
}

/// Final response from `ask_recall`. The `text` carries the full
/// generated answer including in-line `[memory:<uuid>]` markers for
/// the UI to render as clickable chips.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallResponse {
    pub question: String,
    pub text: String,
    pub citations: Vec<AskRecallCitation>,
    /// v0.5.5: every memory we fed the LLM as context, surfaced
    /// to the UI regardless of whether the model emitted a citation
    /// marker for it. For tag-pivot enumeration queries ("what
    /// license keys did I save"), the LLM may hedge and only cite
    /// one — but the user wants to see all retrieved candidates,
    /// not just the LLM's pick. The frontend renders these as
    /// source cards in addition to (or in place of) the citation
    /// chips.
    pub retrieved_sources: Vec<AskRecallCitation>,
    pub tokens_generated: u32,
    pub latency_ms: u64,
    /// Number of chunks the LLM was given context for. 0 means
    /// retrieval found nothing above the Strong-tier threshold —
    /// the model is instructed to say so explicitly when this is
    /// the case.
    pub context_chunks: u32,
    /// v0.5.5: the auto-tag class this query pivoted on, if any
    /// (e.g. "license-key", "url"). `None` for general queries.
    /// Used by the UI to label the sources panel ("3 license keys
    /// found") and by analytics to track tag-intent hit rate.
    pub tag_intent: Option<String>,
}

/// One source the LLM was given context for. Used internally to build
/// the citation map after generation completes — temporal queries
/// cite full memories (no chunk offsets), semantic queries cite the
/// specific chunk we ranked highest.
struct ContextSource {
    memory_id: String,
    title: Option<String>,
    /// v0.5.3: auto-tags from v0.3.7's topic detector. Surfaced to
    /// the LLM in the prompt header so opaque content (license
    /// keys, hashes, URLs) carries its semantic frame. Without
    /// this, a chunk like `RECALL-7K2P-XYZW-...` looks like noise
    /// to the model even when it's the right answer to the user's
    /// question.
    topic_labels: Vec<String>,
    excerpt: String,
    /// Offsets of the excerpt within the parent memory body. For
    /// semantic hits these are the chunk's real offsets; for temporal
    /// hits they're 0..len(excerpt) since we feed the truncated body
    /// directly.
    excerpt_start: i64,
    excerpt_end: i64,
}

/// Truncate body text for a temporal context block. Keeps the lead
/// (most title-like prose lives at the top) and adds an ellipsis when
/// we cut. Conservative cap so 30 memories fit in the ~12K-char budget.
fn truncate_for_context(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.trim().to_string()
    } else {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{}…", cut.trim_end())
    }
}

/// v0.5.12: response shape extended with session_id when the call
/// came in with one. The frontend uses this to track the current
/// conversation; the backend uses it to look up history on the
/// next turn. Single-shot calls (no session_id passed in) get
/// `session_id = None` back, exactly the v0.5.11 shape.
#[tauri::command]
pub async fn ask_recall(
    question: String,
    // v0.5.12: when present, treat this as a continuation of an
    // existing conversation. The backend looks up the session in
    // AppState, injects past turns into the prompt as multi-turn
    // chat-template messages, and appends the user/assistant pair
    // to the session after generation completes. When None, runs
    // single-shot with the v0.5.11 prompt format — preserves the
    // existing behavior for callers that don't yet manage sessions.
    session_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AskRecallResponse> {
    use crate::ai::ask::{session as ask_session, tag_intent, temporal};

    let trimmed = question.trim();
    if trimmed.is_empty() {
        return Err(AppError::Invalid("Question is required.".into()));
    }

    // v0.5.15: load session history from SQLite (replaces v0.5.12's
    // in-memory HashMap). The persisted shape is the
    // `AskRecallMessageRow` JSON-blob storage; we decode each row
    // back into the in-memory `Message` enum the prompt builder
    // expects. Validate the session exists when one was requested
    // — better to fail fast than silently fall back to single-shot.
    let history: Vec<ask_session::Message> = match &session_id {
        Some(sid) => {
            let session = state
                .ask_recall_session_repository
                .get_session(sid)
                .await?;
            match session {
                Some(s) => s
                    .messages
                    .iter()
                    .filter_map(message_row_to_session_message)
                    .collect(),
                None => {
                    return Err(AppError::Invalid(format!(
                        "Ask Recall session {sid} not found. Start a new chat to begin."
                    )));
                }
            }
        }
        None => Vec::new(),
    };

    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;
    if !scheduler.is_enabled() {
        return Err(AppError::Invalid("Enable AI first to ask Recall.".into()));
    }
    let llm = state
        .llm_adapter()
        .ok_or_else(|| AppError::Invalid("LLM adapter not configured on this host.".into()))?
        .clone();
    if !llm.is_ready().await {
        return Err(AppError::Invalid(
            "Download the Ask Recall model in Settings → AI before asking.".into(),
        ));
    }

    // v0.5.11: register a cancel handle for this in-flight ask. The
    // ask_recall_cancel command flips the flag and the LLM's
    // generation loop polls it every token. We use the literal key
    // "current" because v0.5.11 only supports one in-flight ask
    // at a time (LLM mutex serializes generation anyway). v0.5.12+
    // multi-turn uses session_id-keyed handles. Cleanup happens at
    // the natural completion points below — leaving a stale handle
    // briefly on early-error paths is harmless because cancel is a
    // pure flag flip and the next ask_recall replaces the entry.
    let cancel_handle = crate::ai::ask::session::CancelHandle::new();
    {
        let mut handles = state.ask_recall_cancel_handles.lock().await;
        handles.insert("current".to_string(), cancel_handle.clone());
    }

    // v0.5.11: phase events let the UI show "Searching memories…" /
    // "Reading 12 memories…" instead of an opaque spinner. Frontend
    // listens on `recall://ask-recall-stage` and swaps copy.
    let emit_stage = |stage: &'static str, detail: serde_json::Value| {
        let _ = app.emit(
            "recall://ask-recall-stage",
            serde_json::json!({ "stage": stage, "detail": detail }),
        );
    };
    emit_stage("retrieving", serde_json::json!({}));

    // ─── 1. Route by intent ──────────────────────────────────────────
    //
    // Three signals, in order:
    //   * tag intent (v0.5.5): query references a known auto-tag class
    //     ("license keys", "URLs", "phone numbers", etc.). When matched,
    //     we pull every memory with that tag directly via SQL — no
    //     cosine threshold, no MMR — so enumeration questions surface
    //     ALL members of the class. Cosine-only retrieval can't reliably
    //     rank 3 near-identical opaque alphanumeric strings against each
    //     other, which is why this path exists.
    //   * temporal intent: "summarize my week" → date-range pull
    //   * semantic ranking: everything else, the unified pipeline
    //
    // Tag intent and temporal are independent — a query like "what URLs
    // did I save last week" gets tag-pivoted first, then narrowed by
    // the temporal window. Pure prose queries fall through to semantic.
    let tag = tag_intent::detect(trimmed);
    // v0.5.14: inherit tag intent from the prior assistant turn
    // when the current question has no detected intent of its own.
    // For follow-ups like "which one is the latest?" after a turn
    // about license keys, the new question has no tag phrase but
    // the conversation context absolutely is about that tag class.
    // Without inheritance, fresh semantic_search pulls unrelated
    // memories ("ninth attempt of building STT software" matches
    // "which one is the latest" semantically) and the LLM has no
    // way to reject them. Inheriting the tag forces continued
    // tag-pivot retrieval so the same memory class stays in scope.
    let inherited_tag: Option<String> = if tag.is_none() && !history.is_empty() {
        history.iter().rev().find_map(|m| match m {
            ask_session::Message::Assistant {
                tag_intent: Some(t),
                ..
            } => Some(t.clone()),
            _ => None,
        })
    } else {
        None
    };
    let temporal = temporal::detect(trimmed);
    let semantic_query: String = match &temporal {
        Some((_, residual)) if !residual.is_empty() => residual.clone(),
        Some(_) => String::new(), // pure temporal — no semantic ranking
        None => trimmed.to_string(),
    };
    let route = match (&tag, &inherited_tag, &temporal, semantic_query.is_empty()) {
        (Some(_), _, Some(_), _) => "tag_plus_temporal",
        (Some(_), _, None, _) => "tag_pivot",
        (None, Some(_), Some(_), _) => "inherited_tag_plus_temporal",
        (None, Some(_), None, _) => "inherited_tag_pivot",
        (None, None, Some(_), true) => "temporal_only",
        (None, None, Some(_), false) => "temporal_plus_semantic",
        (None, None, None, _) => "semantic_only",
    };
    eprintln!(
        "[recall][ask-recall] route={} tag={:?} inherited={:?} temporal={:?} residual='{}'",
        route,
        tag.as_ref().map(|t| t.tag),
        inherited_tag.as_deref(),
        temporal.as_ref().map(|(w, _)| w.label),
        semantic_query
    );

    // ─── 2. Build candidate sources ──────────────────────────────────
    //
    // Budget: ~12K chars (~3K tokens). Reserve ~600 chars for the
    // system prompt + question; per-source overhead (header + blank
    // line) is ~80 chars; per-source body cap is 280 chars for
    // temporal queries, full chunk text for semantic.
    const BUDGET_CHARS: usize = 12_000;
    const PROMPT_OVERHEAD: usize = 600;
    const PER_SOURCE_OVERHEAD: usize = 80;
    const TEMPORAL_BODY_CAP: usize = 280;
    // v0.5.4: bumped 8 → 12. With MMR disabled (see below) and the
    // 8K-token context window from v0.5.1, we have prompt budget for
    // more sources. 12 covers enumeration questions ("what license
    // keys did I save") without overflowing the budget.
    const SEMANTIC_TOP_K: u32 = 12;
    const TEMPORAL_TOP_N: usize = 30;

    let mut sources: Vec<ContextSource> = Vec::new();
    let mut budget_remaining = BUDGET_CHARS.saturating_sub(PROMPT_OVERHEAD);

    // ─── 2a. Tag-pivot pre-pass ─────────────────────────────────────
    //
    // When tag intent is detected, we pull every memory carrying that
    // auto-tag and pack it into the context FIRST. This guarantees
    // enumeration queries surface every member of the class, not
    // just the one or two that happen to embed near the query phrase.
    //
    // The semantic path still runs after this (filling remaining
    // top-K slots with non-tagged candidates) so a query like "what
    // license keys did I save and where" still gets surrounding prose
    // context alongside the literal keys. For pure enumeration
    // questions, the tagged set fully populates the context.
    //
    // Memory IDs already added by tag-pivot are tracked in
    // `tag_pivoted_ids` so the temporal/semantic paths can skip them
    // (avoid duplicate context blocks).
    let mut tag_pivoted_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // v0.5.14: tag-pivot fires for either an explicit tag (current
    // turn detected one) OR an inherited tag (prior assistant
    // turn's intent carried forward). The downstream code only
    // needs the tag string + label; we synthesize a minimal
    // descriptor for the inherited case.
    let pivot_tag_str: Option<&str> = tag
        .as_ref()
        .map(|t| t.tag)
        .or(inherited_tag.as_deref());
    let pivot_label: Option<String> = tag
        .as_ref()
        .map(|t| t.label.to_string())
        .or_else(|| inherited_tag.as_ref().map(|s| s.replace('-', " ")));
    if let Some(pivot_tag_value) = pivot_tag_str {
        let tagged = state
            .memory_repository
            .list_memories_by_topic_label(pivot_tag_value)
            .await?;
        // v0.5.6: drop self-captures (Recall UI screenshots) up
        // front so they never enter the candidate set, even if
        // the auto-tagger tagged them before the v0.5.6 scrub
        // backfill ran.
        let tagged: Vec<crate::models::Memory> = tagged
            .into_iter()
            .filter(|m| {
                !m.ocr_engine
                    .as_deref()
                    .map(|e| e.contains("self-capture"))
                    .unwrap_or(false)
            })
            .collect();
        eprintln!(
            "[recall][ask-recall] tag-pivot: tag={} (inherited={}) hits={}",
            pivot_tag_value,
            tag.is_none(),
            tagged.len()
        );
        // Apply temporal window filter when both intents fired.
        let filtered: Vec<&crate::models::Memory> = match &temporal {
            Some((window, _)) => tagged
                .iter()
                .filter(|m| {
                    m.created_at.as_str() >= window.start_iso.as_str()
                        && m.created_at.as_str() <= window.end_iso.as_str()
                })
                .collect(),
            None => tagged.iter().collect(),
        };
        for memory in filtered.iter().take(SEMANTIC_TOP_K as usize) {
            // For tag-pivot, the entire content is the answer (it's
            // typed-content like a license key) — feed it whole, not
            // chunked. Truncate to TEMPORAL_BODY_CAP to keep budget
            // bounded.
            let raw_body = memory
                .extracted_text
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&memory.content);
            let excerpt = truncate_for_context(raw_body, TEMPORAL_BODY_CAP);
            let need = excerpt.len() + PER_SOURCE_OVERHEAD;
            if need > budget_remaining {
                break;
            }
            budget_remaining -= need;
            let topic_labels = memory
                .topic_labels
                .as_ref()
                .map(|json| json.0.clone())
                .unwrap_or_default();
            tag_pivoted_ids.insert(memory.id.clone());
            sources.push(ContextSource {
                memory_id: memory.id.clone(),
                title: memory.title.clone(),
                topic_labels,
                excerpt_start: 0,
                excerpt_end: excerpt.chars().count() as i64,
                excerpt,
            });
        }
    }

    if let Some((window, _)) = &temporal {
        // Pull every memory in the window from the repo. For mixed
        // queries we still pull by date range first — semantic
        // ranking happens within the window below.
        let all = state.memory_repository.list().await?;
        let mut in_window: Vec<&crate::models::Memory> = all
            .iter()
            .filter(|m| {
                m.created_at.as_str() >= window.start_iso.as_str()
                    && m.created_at.as_str() <= window.end_iso.as_str()
            })
            .collect();
        // Newest first — for "summarize my week" recency matters more
        // than insertion order.
        in_window.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Mixed: re-rank within the window using semantic_search,
        // then pick the top-K by score. For temporal-only, just take
        // the most recent up to TEMPORAL_TOP_N.
        let ordered_ids: Vec<String> = if semantic_query.is_empty() {
            in_window
                .iter()
                .take(TEMPORAL_TOP_N)
                .map(|m| m.id.clone())
                .collect()
        } else {
            // Reuse semantic_search to score everything by relevance,
            // then intersect with the window. We pass a generous limit
            // because the intersection step throws away the bulk.
            // v0.5.4: MMR off so enumeration questions ("what license
            // keys did I save last week") don't drop near-duplicate
            // matches.
            let hits =
                semantic_search_internal(&semantic_query, Some(64), false, &state).await?;
            let in_window_ids: std::collections::HashSet<&str> =
                in_window.iter().map(|m| m.id.as_str()).collect();
            let mut ranked: Vec<String> = hits
                .into_iter()
                .filter(|h| in_window_ids.contains(h.memory_id.as_str()))
                .map(|h| h.memory_id)
                .collect();
            // If semantic returned nothing in-window (e.g. the
            // residual is too generic), fall back to recency so the
            // user still gets *something* date-bound.
            if ranked.is_empty() {
                ranked = in_window
                    .iter()
                    .take(TEMPORAL_TOP_N)
                    .map(|m| m.id.clone())
                    .collect();
            } else {
                ranked.truncate(TEMPORAL_TOP_N);
            }
            ranked
        };

        let by_id: HashMap<&str, &crate::models::Memory> =
            in_window.iter().map(|m| (m.id.as_str(), *m)).collect();
        for id in &ordered_ids {
            // Skip memories already added by the tag-pivot pre-pass —
            // duplicating a chunk in the prompt wastes budget without
            // adding signal for the LLM.
            if tag_pivoted_ids.contains(id) {
                continue;
            }
            let Some(memory) = by_id.get(id.as_str()) else {
                continue;
            };
            // Body source preference: extracted_text > content > note.
            // Title is shown in the header so we don't repeat it.
            let raw_body = memory
                .extracted_text
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&memory.content);
            let excerpt = truncate_for_context(raw_body, TEMPORAL_BODY_CAP);
            let need = excerpt.len() + PER_SOURCE_OVERHEAD;
            if need > budget_remaining {
                break;
            }
            budget_remaining -= need;
            let topic_labels = memory
                .topic_labels
                .as_ref()
                .map(|json| json.0.clone())
                .unwrap_or_default();
            sources.push(ContextSource {
                memory_id: memory.id.clone(),
                title: memory.title.clone(),
                topic_labels,
                excerpt_start: 0,
                excerpt_end: excerpt.chars().count() as i64,
                excerpt,
            });
        }
        eprintln!(
            "[recall][ask-recall] temporal: window=[{}..{}] candidates={} packed={}",
            window.start_iso,
            window.end_iso,
            in_window.len(),
            sources.len()
        );
    } else if (tag.is_some() || inherited_tag.is_some()) && tag_pivoted_ids.len() >= 2 {
        // v0.5.10: tag intent fired AND tag-pivot returned ≥2
        // memories. Skip semantic padding — for an enumeration
        // question like "what license keys did I save?", random
        // semantic matches (URLs, command-line outputs whose
        // embeddings happen to score high) just clutter the
        // sources panel. Tag-pivot already covered the answer.
        // v0.5.14: same gate applies for inherited tag intent
        // (follow-up turns in a tag-pivot conversation). Without
        // this, "which one is the latest?" pulled STT-iteration
        // memories alongside the license keys and the LLM picked
        // the iteration count as its answer.
        // We only fall through to semantic when tag-pivot is
        // empty/sparse, which suggests the user's question
        // wasn't really tag enumeration despite the phrasing.
        eprintln!(
            "[recall][ask-recall] semantic: skipped (tag intent + {} tag-pivot hits)",
            tag_pivoted_ids.len()
        );
    } else {
        // Pure topical (or sparse tag-pivot): defer to the unified
        // retrieval pipeline. MMR disabled — Ask Recall needs
        // enumeration of relevant memories, not diversity.
        let hits =
            semantic_search_internal(&semantic_query, Some(SEMANTIC_TOP_K), false, &state).await?;
        eprintln!(
            "[recall][ask-recall] semantic: hits={} top_score={:.3}",
            hits.len(),
            hits.first().map(|h| h.score).unwrap_or(0.0)
        );
        for hit in hits {
            // Tag-pivot pre-pass already added the strongest tagged
            // matches; skip duplicates so the semantic path fills
            // remaining budget with fresh context.
            if tag_pivoted_ids.contains(&hit.memory_id) {
                continue;
            }
            let need = hit.chunk_text.len() + PER_SOURCE_OVERHEAD;
            if need > budget_remaining {
                break;
            }
            budget_remaining -= need;
            // Need the title + topic_labels for the citation chip
            // and prompt context. One extra lookup per top-K hit
            // is fine — bounded at SEMANTIC_TOP_K = 12.
            let memory = state.memory_repository.find(&hit.memory_id).await?;
            let title = memory.as_ref().and_then(|m| m.title.clone());
            let topic_labels = memory
                .as_ref()
                .and_then(|m| m.topic_labels.as_ref())
                .map(|json| json.0.clone())
                .unwrap_or_default();
            sources.push(ContextSource {
                memory_id: hit.memory_id.clone(),
                title,
                topic_labels,
                excerpt: hit.chunk_text,
                excerpt_start: hit.chunk_start,
                excerpt_end: hit.chunk_end,
            });
        }
    }

    // ─── 2c. Multi-turn context carry-forward (v0.5.13) ──────────────
    //
    // Each turn's retrieval sees only the current question's text.
    // For follow-ups like "which one is the latest?" after a turn
    // about license keys, that question by itself doesn't pull
    // license-key memories — semantic_search instead returns
    // memories about iterations/versions/attempts, and the LLM
    // (which has the chat history in its prompt) tries to ground
    // its answer in whatever it was given. Wrong context, wrong
    // answer.
    //
    // Fix: prepend the prior assistant turn's `retrieved_sources`
    // to the current turn's context. The LLM then sees:
    //   * memories the previous turn was grounded in (the license
    //     keys), so "which one" has a concrete antecedent
    //   * any new memories the fresh retrieval pulled
    // Cap at SEMANTIC_TOP_K so the prompt stays bounded.
    if !history.is_empty() {
        // Find the most recent assistant message and pull its
        // retrieved_sources. Walk from the end so we always pick
        // the latest, even if history is malformed.
        let prev_sources_opt = history.iter().rev().find_map(|m| match m {
            ask_session::Message::Assistant {
                retrieved_sources, ..
            } => Some(retrieved_sources.clone()),
            _ => None,
        });
        if let Some(prev_sources) = prev_sources_opt {
            let already: std::collections::HashSet<String> = sources
                .iter()
                .map(|s| s.memory_id.clone())
                .collect();
            let mut carried: Vec<ContextSource> = Vec::new();
            for prev in &prev_sources {
                if already.contains(&prev.memory_id) {
                    continue;
                }
                // topic_labels were not preserved on the citation
                // shape (frontend doesn't need them); the LLM
                // already has them in the prior chat-template
                // turn, so re-deriving is unnecessary.
                carried.push(ContextSource {
                    memory_id: prev.memory_id.clone(),
                    title: prev.title.clone(),
                    topic_labels: Vec::new(),
                    excerpt: prev.chunk_text.clone(),
                    excerpt_start: prev.chunk_start,
                    excerpt_end: prev.chunk_end,
                });
            }
            // Carried first (prior context wins for follow-ups),
            // then current. Truncate to SEMANTIC_TOP_K so the
            // prompt budget stays bounded.
            let carried_count = carried.len();
            carried.extend(sources.drain(..));
            sources = carried;
            sources.truncate(SEMANTIC_TOP_K as usize);
            eprintln!(
                "[recall][ask-recall] carried {} prior sources; total context now {}",
                carried_count,
                sources.len()
            );
        }
    }

    // ─── 3. Build the prompt ────────────────────────────────────────
    let context_chunks_count = sources.len() as u32;
    let prompt = if sources.is_empty() {
        // No-context branch: explicitly forbid citations so the LLM
        // doesn't hallucinate `[memory:0]` markers (v0.4.3 bug).
        format!(
            "There are no saved memories that match this question. Answer with one short sentence stating that you have no saved memories about this topic. Do not guess. Do not write any [memory:...] markers — there is nothing to cite.\n\nQuestion: {}",
            trimmed
        )
    } else {
        let mut block = String::with_capacity(BUDGET_CHARS);
        for src in &sources {
            // v0.5.3: surface auto-tags ("license-key", "url",
            // "code-snippet", etc.) in the per-memory header so the
            // LLM has the same semantic frame the ranker had. Without
            // this, opaque content like `RECALL-7K2P-XYZW-...` is
            // unrecognizable as a license key to the model.
            let title_part = src
                .title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Untitled");
            let tags_part = if src.topic_labels.is_empty() {
                String::new()
            } else {
                format!(" · tags: {}", src.topic_labels.join(", "))
            };
            block.push_str(&format!(
                "[memory:{}] {}{}\n{}\n\n",
                src.memory_id, title_part, tags_part, src.excerpt
            ));
        }
        // v0.5.5: separate intros per route. The tag-pivot intro is
        // imperative — when the user asks for an enumerable class
        // ("license keys", "URLs"), they want the actual values
        // listed, not hedge-language. Qwen 7B at Q4_K_M reads
        // opaque alphanumerics as low-confidence by default and
        // wraps them in qualifiers ("might be a license key") —
        // a directive prompt overrides that conservative bias.
        // v0.5.14: prompt intro chooses among (explicit tag) /
        // (inherited tag) / (temporal-only) / (general). The
        // explicit-tag intro is imperative ("list every one") for
        // enumeration questions like "what license keys did I
        // save". The inherited-tag intro is calmer ("the user is
        // continuing the conversation about <class>") because the
        // follow-up question may not be enumeration — they might
        // ask "which one is the latest?" and want a specific
        // answer, not a re-list.
        let intro = match (&tag, &inherited_tag, &temporal) {
            (Some(intent), _, Some(w)) => format!(
                "The user is asking for ALL {} from their saved memories from {}. Each memory below tagged with `{}` IS a {} — list every one with its exact body content, one per line. Cite each with [memory:<id>] using the exact id shown. Do not hedge. Do not say 'might be' or 'not explicitly stated'. The tag is authoritative.",
                intent.label,
                w.0.label,
                intent.tag,
                intent.label.trim_end_matches('s')
            ),
            (Some(intent), _, None) => format!(
                "The user is asking for ALL {} from their saved memories. Each memory below tagged with `{}` IS a {} — list every one with its exact body content, one per line. Cite each with [memory:<id>] using the exact id shown. Do not hedge. Do not say 'might be' or 'not explicitly stated'. The tag is authoritative.",
                intent.label,
                intent.tag,
                intent.label.trim_end_matches('s')
            ),
            (None, Some(inherited), _) => {
                let label = pivot_label.clone().unwrap_or_else(|| inherited.replace('-', " "));
                format!(
                    "The user is continuing a conversation about their saved {}. Use only the memories below; every memory tagged `{}` IS a {}. Cite each fact with [memory:<id>] using the exact id shown. Trust the tag when the body looks opaque. If the memories don't answer the follow-up, say so plainly.",
                    label,
                    inherited,
                    label.trim_end_matches('s')
                )
            }
            (None, None, Some((w, _))) => format!(
                "The user is asking about their saved memories from {}. Use only the memories below; cite each fact with [memory:<id>] using the exact id shown. Tags after each memory header (e.g. 'tags: license-key') describe what the memory is — trust them when the body looks opaque. If the memories don't answer the question, say so.",
                w.label
            ),
            (None, None, None) => "Use only the memories below to answer. Cite each fact with [memory:<id>] using the exact id shown. Tags after each memory header (e.g. 'tags: license-key') describe what the memory is — trust them when the body looks opaque. If the memories don't contain the answer, say so explicitly.".to_string(),
        };
        format!("{}\n\nMemories:\n{}\nQuestion: {}", intro, block, trimmed)
    };

    // ─── 4. Stream + parse ──────────────────────────────────────────
    let app_clone = app.clone();
    let started_at = std::time::Instant::now();
    let on_token: Box<dyn Fn(String) + Send + Sync> = Box::new(move |delta: String| {
        let _ = app_clone.emit(
            "recall://ask-recall-token",
            serde_json::json!({ "delta": delta }),
        );
    });
    // v0.5.12: when this turn is part of a multi-turn conversation,
    // wrap the user-content prompt in the full Qwen2.5 chat template
    // including all prior turns. The adapter sees this verbatim
    // (pre_formatted=true) so the model gets proper turn boundaries
    // instead of a flat string of pasted-together text.
    //
    // History budgeting: keep the most recent 3 user-assistant
    // pairs. Older turns drop entirely — we don't summarize or
    // partially-include because Qwen2.5 handles abrupt history
    // truncation more cleanly than truncated turn-bodies. If the
    // user wants Recall to remember turn 1 specifically by turn 6,
    // they re-paste the relevant context — same as any other LLM
    // chat surface. v0.5.13+ can add smarter summarization.
    let (final_prompt, pre_formatted) = if history.is_empty() {
        (prompt, false)
    } else {
        const MAX_HISTORY_PAIRS: usize = 3;
        let mut wrapped = String::with_capacity(prompt.len() + 8_192);
        wrapped.push_str("<|im_start|>system\n");
        wrapped.push_str("You are Recall, a helpful AI assistant grounded in the user's saved memories. Cite each factual claim with [memory:<id>]. If the provided context doesn't contain the answer, say so explicitly — do not guess.");
        wrapped.push_str("<|im_end|>\n");
        // Walk history pair-by-pair, keep last MAX_HISTORY_PAIRS
        // complete (User, Assistant) pairs.
        let pairs: Vec<(&ask_session::Message, &ask_session::Message)> = history
            .chunks_exact(2)
            .filter_map(|pair| match (&pair[0], &pair[1]) {
                (
                    u @ ask_session::Message::User { .. },
                    a @ ask_session::Message::Assistant { .. },
                ) => Some((u, a)),
                _ => None,
            })
            .collect();
        let drop_n = pairs.len().saturating_sub(MAX_HISTORY_PAIRS);
        for (user_msg, assistant_msg) in pairs.into_iter().skip(drop_n) {
            if let ask_session::Message::User { content, .. } = user_msg {
                wrapped.push_str("<|im_start|>user\n");
                wrapped.push_str(content);
                wrapped.push_str("<|im_end|>\n");
            }
            if let ask_session::Message::Assistant { content, .. } = assistant_msg {
                wrapped.push_str("<|im_start|>assistant\n");
                wrapped.push_str(content);
                wrapped.push_str("<|im_end|>\n");
            }
        }
        // Current turn — `prompt` here is the intro+memories+question
        // text from the existing build above.
        wrapped.push_str("<|im_start|>user\n");
        wrapped.push_str(&prompt);
        wrapped.push_str("<|im_end|>\n");
        wrapped.push_str("<|im_start|>assistant\n");
        eprintln!(
            "[recall][ask-recall] multi-turn: history_pairs={} kept={} prompt_chars={}",
            history.len() / 2,
            (history.len() / 2).min(MAX_HISTORY_PAIRS),
            wrapped.len()
        );
        (wrapped, true)
    };

    let request = LlmGenerationRequest {
        prompt: final_prompt,
        pre_formatted,
        max_tokens: 512,
        temperature: 0.0,
    };
    // v0.5.11: emit prefill stage so the UI can show
    // "Reading N memories…" before the model warms up the prompt.
    // First-token arrival is the implicit signal for "generating"
    // (frontend swaps on first delta), so we don't emit a separate
    // generating event here.
    emit_stage(
        "prefill",
        serde_json::json!({ "memories": context_chunks_count }),
    );
    // v0.5.11: cancellable streaming. The cancel handle was
    // registered above; the LLM loop polls it every token and
    // returns a partial response if cancelled.
    let response = llm
        .generate_streaming_cancellable(request, cancel_handle.clone(), on_token)
        .await?;
    let latency_ms = started_at.elapsed().as_millis() as u64;

    // Citations: dedupe by memory_id, resolve back to the source we
    // fed (so the chip's preview is the exact excerpt the LLM saw).
    // We restrict to memory_ids that actually appeared in `sources` —
    // anything else is a hallucinated marker and gets dropped.
    let citation_re = regex::Regex::new(r"\[memory:([0-9a-fA-F\-]+)\]").unwrap();
    let by_id: HashMap<&str, &ContextSource> =
        sources.iter().map(|s| (s.memory_id.as_str(), s)).collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut citations: Vec<AskRecallCitation> = Vec::new();
    for cap in citation_re.captures_iter(&response.text) {
        let memory_id = cap[1].to_string();
        if !seen.insert(memory_id.clone()) {
            continue;
        }
        let Some(src) = by_id.get(memory_id.as_str()) else {
            // Hallucinated id — don't surface a chip the user can't
            // click through to. Renderer falls through to plain text.
            continue;
        };
        citations.push(AskRecallCitation {
            memory_id: memory_id.clone(),
            title: src.title.clone(),
            chunk_text: src.excerpt.clone(),
            chunk_start: src.excerpt_start,
            chunk_end: src.excerpt_end,
        });
    }

    // v0.5.5: build `retrieved_sources` from every memory we fed
    // the LLM, regardless of whether the model emitted a citation
    // marker for it. For tag-pivot enumeration the LLM may hedge
    // and only cite one — but the user wants to see all retrieved
    // candidates, so the UI renders these as source cards.
    let retrieved_sources: Vec<AskRecallCitation> = sources
        .iter()
        .map(|src| AskRecallCitation {
            memory_id: src.memory_id.clone(),
            title: src.title.clone(),
            chunk_text: src.excerpt.clone(),
            chunk_start: src.excerpt_start,
            chunk_end: src.excerpt_end,
        })
        .collect();

    let cancelled = cancel_handle.is_cancelled();
    let _ = app.emit(
        "recall://ask-recall-complete",
        serde_json::json!({
            "tokens": response.tokens_generated,
            "latencyMs": latency_ms,
            "cancelled": cancelled,
        }),
    );

    // v0.5.11: drop the cancel handle from the registry now that
    // generation is done. Subsequent cancel calls before the next
    // ask_recall starts will no-op silently.
    {
        let mut handles = state.ask_recall_cancel_handles.lock().await;
        handles.remove("current");
    }

    eprintln!(
        "[recall][ask-recall] done: tokens={} latency_ms={} citations={} retrieved={} context_chunks={} cancelled={}",
        response.tokens_generated,
        latency_ms,
        citations.len(),
        retrieved_sources.len(),
        context_chunks_count,
        cancelled
    );

    // v0.5.12: append User + Assistant messages to the session if
    // this turn was part of a conversation. We only append AFTER
    // generation completes successfully so a cancelled or errored
    // turn doesn't leave a half-state in the history. (Cancelled
    // turns DO append — the partial answer is still useful context
    // for the next turn, and the user explicitly chose to keep
    // what they got.)
    let now = chrono::Utc::now().to_rfc3339();

    // v0.5.15: persist user + assistant rows to the DB. The history
    // count we just snapshotted (`history.len()`) gives us the
    // sequence index for the user row; assistant follows at + 1.
    // After this call, get_session() returns the full thread for
    // re-rendering. If no session_id was passed, we're on the
    // legacy single-shot path — skip persistence.
    if let Some(sid) = &session_id {
        let user_seq = history.len() as i64;
        let assistant_seq = user_seq + 1;
        let user_row = crate::models::AskRecallMessageRow {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: sid.clone(),
            sequence: user_seq,
            role: "user".to_string(),
            content: trimmed.to_string(),
            retrieved_sources: None,
            citations: None,
            tokens_generated: None,
            latency_ms: None,
            tag_intent: None,
            timestamp: now.clone(),
        };
        let assistant_row = crate::models::AskRecallMessageRow {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: sid.clone(),
            sequence: assistant_seq,
            role: "assistant".to_string(),
            content: response.text.clone(),
            retrieved_sources: serde_json::to_string(&retrieved_sources).ok(),
            citations: serde_json::to_string(&citations).ok(),
            tokens_generated: Some(response.tokens_generated as i64),
            latency_ms: Some(latency_ms as i64),
            tag_intent: tag.as_ref().map(|t| t.tag.to_string()),
            timestamp: now.clone(),
        };
        if let Err(err) = state
            .ask_recall_session_repository
            .append_message(&user_row)
            .await
        {
            eprintln!("[recall][ask-recall] failed to persist user msg: {err}");
        }
        if let Err(err) = state
            .ask_recall_session_repository
            .append_message(&assistant_row)
            .await
        {
            eprintln!("[recall][ask-recall] failed to persist assistant msg: {err}");
        }

        // First-turn title generation: if this was the user's
        // first message in the session, also rename the session
        // to a trimmed version of the question (the placeholder
        // "New chat" gets replaced immediately) and kick off an
        // async LLM call to produce a 4-6 word summary title.
        if user_seq == 0 {
            let placeholder = trim_to_title(trimmed, 50);
            if let Err(err) = state
                .ask_recall_session_repository
                .rename_session(sid, &placeholder)
                .await
            {
                eprintln!("[recall][ask-recall] placeholder rename failed: {err}");
            }
            spawn_llm_title_generation(
                app.clone(),
                state.ask_recall_session_repository.clone(),
                state.llm_adapter().cloned(),
                sid.clone(),
                trimmed.to_string(),
                response.text.clone(),
            );
        }
    }

    Ok(AskRecallResponse {
        question: trimmed.to_string(),
        text: response.text,
        citations,
        retrieved_sources,
        tokens_generated: response.tokens_generated,
        latency_ms,
        context_chunks: context_chunks_count,
        tag_intent: tag.as_ref().map(|t| t.tag.to_string()),
    })
}

/// v0.5.15: trim a string to a clean title at most `max_chars`
/// long. Cuts at the last whitespace before the limit so we
/// don't end mid-word, falls back to hard cut if no whitespace
/// is available within budget.
fn trim_to_title(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim().trim_end_matches('?').trim_end_matches('.');
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max_chars).collect();
    if let Some(idx) = cut.rfind(char::is_whitespace) {
        let mut out = cut[..idx].trim().to_string();
        out.push('…');
        return out;
    }
    let mut out = cut;
    out.push('…');
    out
}

/// v0.5.15: convert a persisted message row back into the
/// in-memory shape the prompt builder expects. Returns None for
/// rows whose JSON blobs fail to decode (defensive — shouldn't
/// happen in practice but we'd rather skip than crash).
fn message_row_to_session_message(
    row: &crate::models::AskRecallMessageRow,
) -> Option<crate::ai::ask::session::Message> {
    use crate::ai::ask::session::Message;
    match row.role.as_str() {
        "user" => Some(Message::User {
            content: row.content.clone(),
            timestamp: row.timestamp.clone(),
        }),
        "assistant" => {
            let retrieved_sources: Vec<AskRecallCitation> = row
                .retrieved_sources
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let citations: Vec<AskRecallCitation> = row
                .citations
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            Some(Message::Assistant {
                content: row.content.clone(),
                retrieved_sources,
                citations,
                tokens_generated: row.tokens_generated.unwrap_or(0) as u32,
                latency_ms: row.latency_ms.unwrap_or(0) as u64,
                tag_intent: row.tag_intent.clone(),
                timestamp: row.timestamp.clone(),
            })
        }
        _ => None,
    }
}

/// v0.5.15: fire-and-forget LLM call to generate a 4-6 word
/// summary title for a freshly-started conversation. Runs ~1s
/// after the first turn completes; on success, updates the
/// session row's `llm_title` field and emits an event the
/// sidebar listens for. Failures are silent — the placeholder
/// title (the user's first message, trimmed) stays in place.
fn spawn_llm_title_generation(
    app: AppHandle,
    repo: crate::db::repositories::SharedAskRecallSessionRepository,
    llm: Option<std::sync::Arc<dyn crate::ai::llm::AskRecallAdapter>>,
    session_id: String,
    user_question: String,
    assistant_answer: String,
) {
    let Some(llm) = llm else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if !llm.is_ready().await {
            return;
        }
        let prompt = format!(
            "<|im_start|>system\nYou produce 4-6 word chat titles. Output ONLY the title with no quotes, no period, no prefix. Be specific to the topic.<|im_end|>\n<|im_start|>user\nUser: {}\n\nAssistant: {}\n\nTitle:<|im_end|>\n<|im_start|>assistant\n",
            user_question.chars().take(200).collect::<String>(),
            assistant_answer.chars().take(400).collect::<String>(),
        );
        let request = LlmGenerationRequest {
            prompt,
            pre_formatted: true,
            max_tokens: 24,
            temperature: 0.0,
        };
        let Ok(response) = llm.generate(request).await else {
            return;
        };
        let title = response
            .text
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('.')
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() || title.chars().count() > 80 {
            return;
        }
        if let Err(err) = repo.set_llm_title(&session_id, &title).await {
            eprintln!("[recall][ask-recall] set_llm_title failed: {err}");
            return;
        }
        let _ = app.emit(
            "recall://ask-recall-session-renamed",
            serde_json::json!({ "sessionId": session_id, "title": title }),
        );
    });
}

/// v0.5.11: flip the cancel flag for the in-flight ask_recall (if
/// any). The LLM generation loop polls the flag every token; on
/// cancel the loop returns a partial response and the streaming
/// completes early. Idempotent — no-ops if no ask is currently in
/// flight (the registry entry was already removed by completion).
#[tauri::command]
pub async fn ask_recall_cancel(state: State<'_, AppState>) -> AppResult<bool> {
    let handles = state.ask_recall_cancel_handles.lock().await;
    if let Some(handle) = handles.get("current") {
        handle.cancel();
        eprintln!("[recall][ask-recall] cancel requested");
        Ok(true)
    } else {
        Ok(false)
    }
}

// ─── v0.5.15: persistent Ask Recall sessions ──────────────────────

/// Create a fresh session row with a placeholder "Untitled" title.
/// The frontend immediately switches to the new session as the
/// active conversation. Real titles arrive in two waves: when the
/// first user message lands we set `title` to its trimmed text,
/// then ~1s after the first turn completes we run a small LLM
/// call and fill in `llm_title` with a 4–6 word summary.
#[tauri::command]
pub async fn ask_recall_new_session(state: State<'_, AppState>) -> AppResult<String> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    state
        .ask_recall_session_repository
        .create_session(&session_id, "New chat", &now)
        .await?;
    Ok(session_id)
}

/// List every session newest-first. Drives the RECENT CHATS
/// sidebar surface. Light shape — message bodies stay in the
/// `ask_recall_messages` table until the user opens the chat.
#[tauri::command]
pub async fn ask_recall_list_sessions(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::AskRecallSessionSummary>> {
    state.ask_recall_session_repository.list_sessions().await
}

/// Read one session's full message history. Returns None when
/// the id doesn't exist (session was deleted from another
/// surface). The frontend rehydrates AskView's thread state from
/// the returned `messages` array.
///
/// v0.5.15: decodes the per-row `retrieved_sources` / `citations`
/// JSON blobs into structured arrays before serializing to the
/// frontend. Saves the frontend from having to JSON.parse
/// everything at render time.
#[tauri::command]
pub async fn ask_recall_get_session(
    session_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<DecodedAskRecallSession>> {
    let session = state
        .ask_recall_session_repository
        .get_session(&session_id)
        .await?;
    Ok(session.map(|s| DecodedAskRecallSession {
        session_id: s.session_id,
        title: s.title,
        llm_title: s.llm_title,
        created_at: s.created_at,
        last_used_at: s.last_used_at,
        messages: s
            .messages
            .into_iter()
            .map(decode_message_row)
            .collect(),
    }))
}

/// v0.5.15: decoded session payload — same fields as
/// `AskRecallSessionFull` but messages have their JSON blobs
/// already deserialized into structured arrays.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedAskRecallSession {
    pub session_id: String,
    pub title: String,
    pub llm_title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub messages: Vec<DecodedAskRecallMessage>,
}

/// v0.5.15: discriminated by `role`. Mirrors the frontend's
/// `AskRecallMessage` type so JSON deserialization on the TS
/// side is direct.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "role")]
pub enum DecodedAskRecallMessage {
    User {
        content: String,
        timestamp: String,
    },
    Assistant {
        content: String,
        retrieved_sources: Vec<AskRecallCitation>,
        citations: Vec<AskRecallCitation>,
        tokens_generated: u32,
        latency_ms: u64,
        tag_intent: Option<String>,
        timestamp: String,
    },
}

fn decode_message_row(row: crate::models::AskRecallMessageRow) -> DecodedAskRecallMessage {
    if row.role == "assistant" {
        let retrieved_sources: Vec<AskRecallCitation> = row
            .retrieved_sources
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let citations: Vec<AskRecallCitation> = row
            .citations
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        DecodedAskRecallMessage::Assistant {
            content: row.content,
            retrieved_sources,
            citations,
            tokens_generated: row.tokens_generated.unwrap_or(0) as u32,
            latency_ms: row.latency_ms.unwrap_or(0) as u64,
            tag_intent: row.tag_intent,
            timestamp: row.timestamp,
        }
    } else {
        DecodedAskRecallMessage::User {
            content: row.content,
            timestamp: row.timestamp,
        }
    }
}

/// Drop a session and (via ON DELETE CASCADE) all its messages.
/// Idempotent.
#[tauri::command]
pub async fn ask_recall_delete_session(
    session_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state
        .ask_recall_session_repository
        .delete_session(&session_id)
        .await
}

/// Manually rename a session. Updates the `title` field. The
/// LLM-generated `llm_title` is left untouched — the sidebar
/// surfaces `llm_title` when present, falling back to `title`.
#[tauri::command]
pub async fn ask_recall_rename_session(
    session_id: String,
    title: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::Invalid("Title cannot be empty.".into()));
    }
    state
        .ask_recall_session_repository
        .rename_session(&session_id, trimmed)
        .await
}
