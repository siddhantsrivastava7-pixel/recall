//! Cosine similarity + MMR re-ranking for related-memory search.
//!
//! v0.3.0 uses brute-force cosine over all embedded chunks. Results
//! aggregate from chunk-level back to memory-level using
//! "mean of top-2 chunk similarities" so a candidate memory with
//! several medium-relevance chunks doesn't get out-scored by one with
//! a single great chunk *and* a single accidental match — and so a
//! memory with one terrific chunk doesn't lose to one with five
//! mediocre chunks. Mean-of-top-K (K=2) is the well-tested middle
//! ground.
//!
//! After per-memory aggregation we apply MMR (Maximal Marginal
//! Relevance) at λ=0.7 so the result list isn't dominated by one big
//! noisy memory or three near-duplicates.
//!
//!     mmr(c) = λ · relevance(c) − (1−λ) · max(sim(c, s) for s in selected)
//!
//! Each result carries the best-matching chunk's range so the UI can
//! render the matched snippet inline.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Number of new chunks since the last centroid compute that triggers
/// a refresh. The centroid is robust to small additions — keeping it
/// stable across short bursts of captures matters more than freshness.
const CENTROID_INVALIDATION_DELTA: u64 = 50;

/// Hard cosine threshold (post-centering) below which a candidate is
/// dropped entirely. With centered embeddings, ~0.15 is the empirical
/// boundary between "shares topic" and "shares English baseline" —
/// below it we'd be surfacing noise.
pub const SEMANTIC_FLOOR: f32 = 0.15;

/// MMR diversity weight. λ=1.0 = pure relevance (the original list).
/// λ=0.0 = pure diversity. 0.7 is "mostly relevance, some diversity"
/// — typical for related-document UIs.
const MMR_LAMBDA: f32 = 0.7;

/// How many of the candidate memory's top-scoring chunks we average
/// when computing per-memory relevance. Two strikes a balance
/// between robustness (vs noisy single-chunk matches) and not
/// punishing memories whose relevance is concentrated in one place.
const TOP_K_CHUNKS_PER_MEMORY: usize = 2;

/// Cosine similarity for two equal-length vectors. BGE outputs are
/// already L2-normalized, so this is effectively dot product, but we
/// don't *assume* that — a future adapter that doesn't normalize
/// would silently rank wrong otherwise.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "cosine on different-dim vectors");
    if a.is_empty() {
        return 0.0;
    }
    let mut dot: f32 = 0.0;
    let mut na: f32 = 0.0;
    let mut nb: f32 = 0.0;
    for i in 0..a.len() {
        let x = a[i];
        let y = b[i];
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub chunk_id: String,
    pub memory_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub text: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedMemoryHit {
    pub memory_id: String,
    /// Aggregated relevance score across the candidate's chunks.
    pub score: f32,
    /// The single chunk that best matched the source — used by the UI
    /// to render an inline excerpt with offsets to highlight.
    pub best_chunk: ScoredChunk,
}

/// Aggregate a flat list of chunk-level matches into per-memory hits,
/// then re-rank with MMR. `source_memory_id` is the memory the user
/// is viewing — its own chunks are excluded from results.
///
/// `chunk_vectors` maps chunk_id → vector and is used by the MMR
/// step to compute diversity (max similarity to already-selected).
pub fn aggregate_with_mmr(
    scored_chunks: Vec<ScoredChunk>,
    chunk_vectors: &HashMap<String, Vec<f32>>,
    source_memory_id: &str,
    top_n: usize,
) -> Vec<RelatedMemoryHit> {
    if top_n == 0 || scored_chunks.is_empty() {
        return Vec::new();
    }

    // Group by memory_id; drop the source memory's own chunks.
    let mut by_memory: HashMap<String, Vec<ScoredChunk>> = HashMap::new();
    for chunk in scored_chunks {
        if chunk.memory_id == source_memory_id {
            continue;
        }
        by_memory.entry(chunk.memory_id.clone()).or_default().push(chunk);
    }

    // Per-memory aggregation: sort each memory's chunks by score
    // descending, take the mean of top K, remember the single best
    // chunk for the UI.
    let mut hits: Vec<RelatedMemoryHit> = by_memory
        .into_iter()
        .filter_map(|(memory_id, mut chunks)| {
            if chunks.is_empty() {
                return None;
            }
            chunks.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
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

    if hits.is_empty() {
        return hits;
    }

    // MMR selection: greedy pick from `hits` ordered by an MMR score
    // that mixes relevance and diversity vs already-picked items.
    let mut selected: Vec<RelatedMemoryHit> = Vec::with_capacity(top_n);
    while selected.len() < top_n && !hits.is_empty() {
        let mut best_idx = 0usize;
        let mut best_mmr = f32::NEG_INFINITY;
        for (idx, candidate) in hits.iter().enumerate() {
            let max_sim_to_selected = selected
                .iter()
                .filter_map(|s| {
                    let cand_vec = chunk_vectors.get(&candidate.best_chunk.chunk_id)?;
                    let sel_vec = chunk_vectors.get(&s.best_chunk.chunk_id)?;
                    Some(cosine(cand_vec, sel_vec))
                })
                .fold(0.0f32, f32::max);

            let mmr = MMR_LAMBDA * candidate.score - (1.0 - MMR_LAMBDA) * max_sim_to_selected;
            if mmr > best_mmr {
                best_mmr = mmr;
                best_idx = idx;
            }
        }
        selected.push(hits.remove(best_idx));
    }

    selected
}

/// Compute the centroid (per-dimension arithmetic mean) of a slice of
/// vectors. All vectors must share dimension. Returns `None` for an
/// empty input. Used as the "English baseline" subtraction term that
/// dramatically improves cosine signal-to-noise on BGE outputs.
pub fn centroid(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vectors.first()?;
    let dim = first.len();
    if dim == 0 {
        return None;
    }
    let mut sum = vec![0.0_f32; dim];
    let mut n: u64 = 0;
    for v in vectors {
        if v.len() != dim {
            // Mixing vector dims would silently corrupt the centroid;
            // bail rather than pretend.
            return None;
        }
        for (i, value) in v.iter().enumerate() {
            sum[i] += *value;
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    for slot in sum.iter_mut() {
        *slot /= n as f32;
    }
    Some(sum)
}

/// Subtract `centroid` from `vector` in place. Returns the result so
/// the caller can chain. Centroid must match dim; mismatch is ignored
/// (returns vector unchanged) so callers don't have to special-case
/// the not-yet-built case.
pub fn subtract_centroid(mut vector: Vec<f32>, centroid: &[f32]) -> Vec<f32> {
    if vector.len() != centroid.len() {
        return vector;
    }
    for i in 0..vector.len() {
        vector[i] -= centroid[i];
    }
    vector
}

/// Coarse human-readable label for a *centered* cosine score. Skips
/// the misleading "% match" framing — centered cosine isn't a
/// probability, and uncentered cosine is buried in baseline noise.
/// Buckets are calibrated against BGE-small + BGE-base after
/// centering on a real user library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrength {
    Strong,
    Related,
    Loose,
}

impl MatchStrength {
    pub fn from_centered_cosine(score: f32) -> Self {
        // v0.3.7: Related threshold tightened from 0.30 to 0.38
        // based on user feedback that the 0.30–0.45 band caught too
        // many topical-neighborhood matches that aren't actually
        // related (same vocabulary, different subject). 0.38+ keeps
        // genuinely related matches while dropping the most common
        // false positives.
        if score >= 0.45 {
            MatchStrength::Strong
        } else if score >= 0.38 {
            MatchStrength::Related
        } else {
            MatchStrength::Loose
        }
    }
}

/// Snapshot of corpus centroid state. The scheduler caches this and
/// invalidates on either model-id change or chunk-count delta crossing
/// `CENTROID_INVALIDATION_DELTA`.
#[derive(Debug, Clone)]
pub struct CentroidCache {
    pub model_id: String,
    pub chunk_count_at_compute: u64,
    pub centroid: Vec<f32>,
}

impl CentroidCache {
    /// Should we recompute? `true` when the active model changed
    /// (different embedding space — old centroid is meaningless), or
    /// when the corpus has grown by more than the invalidation delta
    /// (new captures haven't shifted the mean meaningfully yet, but
    /// they will if we let it drift).
    pub fn is_stale(&self, active_model_id: &str, current_chunk_count: u64) -> bool {
        if self.model_id != active_model_id {
            return true;
        }
        current_chunk_count.saturating_sub(self.chunk_count_at_compute)
            >= CENTROID_INVALIDATION_DELTA
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(chunk_id: &str, memory_id: &str, score: f32) -> ScoredChunk {
        ScoredChunk {
            chunk_id: chunk_id.into(),
            memory_id: memory_id.into(),
            start_offset: 0,
            end_offset: 100,
            text: format!("text for {chunk_id}"),
            score,
        }
    }

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![0.5, 0.5, 0.5, 0.5];
        let s = cosine(&v, &v);
        assert!((s - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine(&a, &b)).abs() < 1e-5);
    }

    #[test]
    fn aggregate_excludes_source_memory() {
        let chunks = vec![
            make_chunk("c1", "src", 0.99),
            make_chunk("c2", "other", 0.5),
        ];
        let vectors = HashMap::from([
            ("c1".to_string(), vec![1.0, 0.0]),
            ("c2".to_string(), vec![0.0, 1.0]),
        ]);
        let hits = aggregate_with_mmr(chunks, &vectors, "src", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory_id, "other");
    }

    #[test]
    fn aggregate_uses_mean_of_top_two_chunks() {
        let chunks = vec![
            make_chunk("a1", "memA", 0.9),
            make_chunk("a2", "memA", 0.8),
            make_chunk("a3", "memA", 0.1),
            make_chunk("b1", "memB", 0.85),
        ];
        let vectors = HashMap::from([
            ("a1".to_string(), vec![1.0, 0.0]),
            ("a2".to_string(), vec![1.0, 0.0]),
            ("a3".to_string(), vec![1.0, 0.0]),
            ("b1".to_string(), vec![0.0, 1.0]),
        ]);
        let hits = aggregate_with_mmr(chunks, &vectors, "src", 5);
        assert_eq!(hits.len(), 2);
        // memA: mean(0.9, 0.8) = 0.85; memB: 0.85 (only one chunk).
        // Tied — order depends on MMR; both should appear.
        let scores: Vec<f32> = hits.iter().map(|h| h.score).collect();
        assert!(scores.contains(&0.85));
    }

    #[test]
    fn mmr_diversity_prefers_distinct_results() {
        // Three candidate memories. memA and memB are near-duplicates
        // (same vector); memC is unrelated. With MMR, after picking
        // the top-scoring one, the second pick should be memC (the
        // diverse one) rather than the duplicate.
        let chunks = vec![
            make_chunk("a1", "memA", 0.9),
            make_chunk("b1", "memB", 0.89),
            make_chunk("c1", "memC", 0.5),
        ];
        let vectors = HashMap::from([
            ("a1".to_string(), vec![1.0, 0.0, 0.0]),
            ("b1".to_string(), vec![0.99, 0.01, 0.0]), // near-duplicate of a1
            ("c1".to_string(), vec![0.0, 0.0, 1.0]),  // orthogonal
        ]);
        let hits = aggregate_with_mmr(chunks, &vectors, "src", 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].memory_id, "memA");
        assert_eq!(hits[1].memory_id, "memC", "MMR should prefer diverse memC over near-duplicate memB");
    }

    #[test]
    fn top_n_zero_yields_empty() {
        let chunks = vec![make_chunk("c1", "memA", 0.5)];
        let vectors = HashMap::new();
        let hits = aggregate_with_mmr(chunks, &vectors, "src", 0);
        assert!(hits.is_empty());
    }

    #[test]
    fn centroid_averages_per_dimension() {
        let vectors = vec![
            vec![1.0, 2.0, 3.0],
            vec![3.0, 2.0, 1.0],
            vec![2.0, 2.0, 2.0],
        ];
        let c = centroid(&vectors).expect("centroid");
        assert_eq!(c, vec![2.0, 2.0, 2.0]);
    }

    #[test]
    fn centroid_empty_returns_none() {
        assert!(centroid(&[]).is_none());
    }

    #[test]
    fn centroid_mismatched_dim_returns_none() {
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0, 5.0]];
        assert!(centroid(&vectors).is_none());
    }

    #[test]
    fn subtract_centroid_shifts_each_dim() {
        let v = vec![5.0, 5.0, 5.0];
        let c = vec![2.0, 3.0, 4.0];
        let result = subtract_centroid(v, &c);
        assert_eq!(result, vec![3.0, 2.0, 1.0]);
    }

    #[test]
    fn subtract_centroid_dim_mismatch_no_op() {
        let v = vec![5.0, 5.0];
        let c = vec![1.0, 2.0, 3.0];
        let result = subtract_centroid(v.clone(), &c);
        assert_eq!(result, v); // unchanged
    }

    #[test]
    fn match_strength_buckets() {
        assert_eq!(MatchStrength::from_centered_cosine(0.50), MatchStrength::Strong);
        assert_eq!(MatchStrength::from_centered_cosine(0.45), MatchStrength::Strong);
        // v0.3.7: 0.40 is now Related (was Related), 0.30 is now
        // Loose (was Related — moved into Loose because the 0.30–0.38
        // band was too noisy in practice).
        assert_eq!(MatchStrength::from_centered_cosine(0.40), MatchStrength::Related);
        assert_eq!(MatchStrength::from_centered_cosine(0.38), MatchStrength::Related);
        assert_eq!(MatchStrength::from_centered_cosine(0.30), MatchStrength::Loose);
        assert_eq!(MatchStrength::from_centered_cosine(0.20), MatchStrength::Loose);
        assert_eq!(MatchStrength::from_centered_cosine(-0.05), MatchStrength::Loose);
    }

    #[test]
    fn centroid_cache_stale_on_model_change() {
        let cache = CentroidCache {
            model_id: "bge-small-en-v1.5".into(),
            chunk_count_at_compute: 100,
            centroid: vec![0.0; 384],
        };
        assert!(cache.is_stale("bge-base-en-v1.5", 100));
    }

    #[test]
    fn centroid_cache_stale_on_chunk_growth() {
        let cache = CentroidCache {
            model_id: "bge-small-en-v1.5".into(),
            chunk_count_at_compute: 100,
            centroid: vec![0.0; 384],
        };
        assert!(!cache.is_stale("bge-small-en-v1.5", 100));
        assert!(!cache.is_stale("bge-small-en-v1.5", 149));
        assert!(cache.is_stale("bge-small-en-v1.5", 150));
        assert!(cache.is_stale("bge-small-en-v1.5", 200));
    }
}
