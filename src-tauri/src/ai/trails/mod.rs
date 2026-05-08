//! v0.5.58 — Memory Trails.
//!
//! A "trail" is the chain of saved memories on a topic over time.
//! It is *not* a richer Related Memories list — it's a different
//! shape with a different intent:
//!
//! | Surface           | Intent                                          | Order        |
//! |-------------------|-------------------------------------------------|--------------|
//! | Related Memories  | "What else looks like this one?"                | by score     |
//! | Memory Trail      | "What's the thread of my thinking on this?"     | by time      |
//!
//! The trail is built from a seed memory by combining five signals
//! into a pairwise link score, then chronologically ordering the
//! top candidates and trimming temporal outliers.
//!
//! ## Scoring
//!
//! `link_score(seed, other) ∈ [0, 1]` is a weighted blend:
//!
//! ```text
//!   0.40 · semantic_similarity   (max chunk-cosine, same as v0.5.46)
//!   0.20 · entity_overlap        (shared people/companies/topics, length-normalized)
//!   0.15 · topic_label_jaccard   (auto-tagger labels)
//!   0.15 · same_project          (1 if equal project_id, else 0)
//!   0.10 · temporal_proximity    (1 / (1 + days_apart / 30))
//! ```
//!
//! These weights are starting points. Expect tuning once the
//! feature is in real use.
//!
//! ## Coherence trimming
//!
//! After scoring + chronological ordering, we walk the chain and
//! keep only the connected segment containing the seed under a
//! 120-day gap rule. Two memories with `s ≥ 0.30` but separated
//! by half a year are usually two unrelated chapters that just
//! share a vocabulary; the trim keeps the "same conversation"
//! bias intact.
//!
//! ## What this module does NOT do
//!
//! * No clustering — that's the job of [`active_threads`] (v0.5.59).
//! * No persistence. Trails are computed on demand. The substrate
//!   (chunks, entities, topic_labels, project_id) is already
//!   persisted; rebuilding a trail is a fraction of a second on
//!   typical libraries.
//! * No LLM. Pure retrieval + structured signals.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::{
    ai::embeddings::similarity::{cosine, EmbeddingVector, SEMANTIC_FLOOR},
    db::repositories::SharedMemoryRepository,
    errors::app_error::AppResult,
    models::Memory,
};

/// One node on a trail. The frontend hydrates the rest of the
/// memory data from the in-memory store using `memory_id` — no
/// reason to re-serialize the full Memory shape on this hot path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailNode {
    pub memory_id: String,
    /// 0..1 — the link score between this node and the seed.
    /// The seed itself reports 1.0.
    pub link_score: f32,
    /// One-line "why this is on the trail" string the UI surfaces
    /// next to each node. Generated from the dominant signal in
    /// the score breakdown.
    pub rationale: String,
    /// Marks the seed memory the trail was built around. Renders
    /// with a filled marker in the UI.
    pub is_seed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailResult {
    pub seed_memory_id: String,
    /// Chronologically ordered (oldest first). Empty when the seed
    /// has no qualifying neighbors above the coherence floor.
    pub nodes: Vec<TrailNode>,
}

/// Maximum trail length. 7 keeps the UI compact and forces the
/// scoring to be selective. Beyond ~7 the chain stops feeling
/// like "your thinking on this topic" and starts feeling like
/// "every memory that mentions this word."
const MAX_TRAIL_LENGTH: usize = 7;

/// Floor on the link score for trail inclusion. Below this the
/// signal is mostly vocabulary overlap rather than topical
/// continuity.
const TRAIL_LINK_FLOOR: f32 = 0.30;

/// Maximum allowed gap between consecutive trail nodes. Trails
/// that span longer chronological gaps usually represent two
/// unrelated chapters that share vocabulary — better to surface
/// the recent segment than a disjoint chain.
const MAX_GAP_DAYS: i64 = 120;

/// Minimum trail size. Below 3 the surface looks weak — UI hides
/// the trail entirely rather than render a sparse "trail of 1."
const MIN_TRAIL_LENGTH: usize = 3;

/// Build a trail centered on the given seed memory id. Returns
/// `Ok(TrailResult)` with possibly an empty `nodes` vec when no
/// qualifying neighbors exist. Errors only on infrastructure
/// failures (missing memory, DB issues).
///
/// `model_label` is the active embedding model id; chunks
/// embedded under any other model are skipped (mixing dims would
/// make cosine meaningless).
///
/// `centroid` is the pre-computed corpus centroid used for
/// centered cosine. Caller passes it through so we don't compute
/// it twice when build_trail is called alongside other retrieval
/// paths.
pub async fn build_trail(
    memory_repo: &SharedMemoryRepository,
    seed_memory_id: &str,
    model_label: &str,
    centroid: Option<&[f32]>,
) -> AppResult<TrailResult> {
    let seed = match memory_repo.find(seed_memory_id).await? {
        Some(m) => m,
        None => {
            return Ok(TrailResult {
                seed_memory_id: seed_memory_id.to_string(),
                nodes: Vec::new(),
            })
        }
    };

    let all_memories = memory_repo.list().await?;
    if all_memories.len() < 2 {
        return Ok(TrailResult {
            seed_memory_id: seed_memory_id.to_string(),
            nodes: Vec::new(),
        });
    }

    // ---- 1. Pull seed signals up front ----
    let seed_chunks = memory_repo.list_chunks_for_memory(seed_memory_id).await?;
    let seed_vectors: Vec<Vec<f32>> = seed_chunks
        .iter()
        .filter_map(|chunk| {
            if chunk.embedding_model.as_deref() != Some(model_label) {
                return None;
            }
            let bytes = chunk.embedding_vector.as_ref()?;
            let v = EmbeddingVector::from_bytes(model_label, bytes)?.values;
            Some(maybe_center(v, centroid))
        })
        .collect();

    let seed_topics = topic_label_set(&seed);
    let seed_entities = memory_repo
        .list_entities_for_memory(seed_memory_id)
        .await
        .ok()
        .map(entity_signature_set)
        .unwrap_or_default();
    let seed_created_at = parse_iso(&seed.created_at);

    // ---- 2. Score every other eligible memory ----
    // Eligibility: drop self-captures (Recall UI screenshots, see
    // v0.5.6 for context). Drop ask-recall outputs (those are
    // generated answers, not user-saved memories).
    let candidates: Vec<&Memory> = all_memories
        .iter()
        .filter(|m| m.id != seed_memory_id)
        .filter(|m| !is_self_capture(m))
        .filter(|m| m.source_app.as_deref() != Some("ask-recall"))
        .collect();

    let mut scored: Vec<ScoredMemory> = Vec::with_capacity(candidates.len());

    for candidate in &candidates {
        let breakdown = score_pair(
            &seed,
            candidate,
            &seed_vectors,
            &seed_topics,
            &seed_entities,
            seed_created_at.as_ref(),
            memory_repo,
            model_label,
            centroid,
        )
        .await?;

        if breakdown.total < TRAIL_LINK_FLOOR {
            continue;
        }
        scored.push(ScoredMemory {
            memory_id: candidate.id.clone(),
            created_at: candidate.created_at.clone(),
            link_score: breakdown.total,
            rationale: rationale_for(&breakdown),
        });
    }

    // ---- 3. Top-K, then chronological order ----
    scored.sort_by(|a, b| {
        b.link_score
            .partial_cmp(&a.link_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(MAX_TRAIL_LENGTH.saturating_sub(1)); // -1 leaves room for the seed

    // Build the final ordered chain: seed + scored, sorted by created_at.
    let mut chain: Vec<TrailNode> = scored
        .into_iter()
        .map(|s| TrailNode {
            memory_id: s.memory_id,
            link_score: s.link_score,
            rationale: s.rationale,
            is_seed: false,
        })
        .collect();
    chain.push(TrailNode {
        memory_id: seed.id.clone(),
        link_score: 1.0,
        rationale: "this memory".to_string(),
        is_seed: true,
    });

    // Pull each node's created_at for sorting + gap trimming. We
    // re-fetch from the candidate set we already have rather than
    // round-tripping the DB again.
    let mut by_id: HashMap<&str, &Memory> = HashMap::new();
    for m in &all_memories {
        by_id.insert(m.id.as_str(), m);
    }
    chain.sort_by(|a, b| {
        let av = by_id.get(a.memory_id.as_str()).map(|m| m.created_at.as_str()).unwrap_or("");
        let bv = by_id.get(b.memory_id.as_str()).map(|m| m.created_at.as_str()).unwrap_or("");
        av.cmp(bv)
    });

    // ---- 4. Coherence trim: keep only the segment containing the seed ----
    let trimmed = trim_to_seed_segment(chain, &by_id);

    // ---- 5. Reject sparse trails ----
    let nodes = if trimmed.len() < MIN_TRAIL_LENGTH {
        Vec::new()
    } else {
        trimmed
    };

    Ok(TrailResult {
        seed_memory_id: seed.id,
        nodes,
    })
}

/// One scored candidate ahead of trail assembly. We keep this
/// separate from `TrailNode` because nodes need order info and
/// rationale; scoring just needs the score.
struct ScoredMemory {
    memory_id: String,
    created_at: String,
    link_score: f32,
    rationale: String,
}

/// Five-part decomposition of a single pairwise link score. Used
/// internally to pick the dominant signal for the user-facing
/// rationale string.
struct ScoreBreakdown {
    total: f32,
    semantic_weighted: f32,
    semantic_raw: f32,
    entity_weighted: f32,
    shared_entity: Option<String>,
    topic_weighted: f32,
    shared_topic: Option<String>,
    project_weighted: f32,
    temporal_weighted: f32,
    days_apart: i64,
}

#[allow(clippy::too_many_arguments)]
async fn score_pair(
    seed: &Memory,
    other: &Memory,
    seed_vectors: &[Vec<f32>],
    seed_topics: &HashSet<String>,
    seed_entities: &HashSet<String>,
    seed_created_at: Option<&chrono::DateTime<chrono::Utc>>,
    memory_repo: &SharedMemoryRepository,
    model_label: &str,
    centroid: Option<&[f32]>,
) -> AppResult<ScoreBreakdown> {
    // ----- semantic similarity (40%) -----
    let semantic_raw = if seed_vectors.is_empty() {
        0.0
    } else {
        let other_chunks = memory_repo.list_chunks_for_memory(&other.id).await?;
        let mut max_sim: f32 = 0.0;
        for chunk in &other_chunks {
            if chunk.embedding_model.as_deref() != Some(model_label) {
                continue;
            }
            let Some(bytes) = chunk.embedding_vector.as_ref() else {
                continue;
            };
            let Some(vec) = EmbeddingVector::from_bytes(model_label, bytes) else {
                continue;
            };
            let centered = maybe_center(vec.values, centroid);
            for src in seed_vectors {
                let sim = cosine(src, &centered);
                if sim > max_sim {
                    max_sim = sim;
                }
            }
        }
        // Clamp negative similarities (centered cosine can go below
        // zero) to 0 so the weighted component stays in [0, 0.4].
        max_sim.max(0.0).min(1.0)
    };
    let semantic_weighted = 0.40 * semantic_raw;

    // ----- topic_label jaccard (15%) -----
    let other_topics = topic_label_set(other);
    let (topic_jaccard, shared_topic) = jaccard_with_sample(seed_topics, &other_topics);
    let topic_weighted = 0.15 * topic_jaccard;

    // ----- entity overlap (20%) -----
    let other_entities = memory_repo
        .list_entities_for_memory(&other.id)
        .await
        .ok()
        .map(entity_signature_set)
        .unwrap_or_default();
    let (entity_overlap_normalized, shared_entity) =
        entity_overlap(seed_entities, &other_entities);
    let entity_weighted = 0.20 * entity_overlap_normalized;

    // ----- same project (15%) -----
    let project_weighted = match (&seed.project_id, &other.project_id) {
        (Some(a), Some(b)) if a == b => 0.15,
        _ => 0.0,
    };

    // ----- temporal proximity (10%) -----
    let other_created_at = parse_iso(&other.created_at);
    let days_apart = match (seed_created_at, other_created_at.as_ref()) {
        (Some(a), Some(b)) => (a.signed_duration_since(*b)).num_days().abs(),
        _ => 365, // unknown date → treat as far away
    };
    let temporal_proximity = 1.0 / (1.0 + days_apart as f32 / 30.0);
    let temporal_weighted = 0.10 * temporal_proximity;

    let total = semantic_weighted
        + entity_weighted
        + topic_weighted
        + project_weighted
        + temporal_weighted;

    Ok(ScoreBreakdown {
        total,
        semantic_weighted,
        semantic_raw,
        entity_weighted,
        shared_entity,
        topic_weighted,
        shared_topic,
        project_weighted,
        temporal_weighted,
        days_apart,
    })
}

/// Pick the strongest contribution and turn it into a one-line
/// rationale string for the UI. "Strongest" means largest
/// weighted value — already accounts for the per-signal weight,
/// so a 0.20 entity component beats a 0.18 topic component even
/// though the unweighted entity score might be lower.
fn rationale_for(b: &ScoreBreakdown) -> String {
    let entries: [(&str, f32); 5] = [
        ("similarity", b.semantic_weighted),
        ("entity", b.entity_weighted),
        ("topic", b.topic_weighted),
        ("project", b.project_weighted),
        ("temporal", b.temporal_weighted),
    ];
    let (kind, _) = entries
        .iter()
        .copied()
        .max_by(|a, c| a.1.partial_cmp(&c.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(("similarity", 0.0));

    match kind {
        "similarity" => format!("similar content · cosine {:.2}", b.semantic_raw),
        "entity" => match &b.shared_entity {
            Some(e) => format!("shared entity: {e}"),
            None => "shared entities".to_string(),
        },
        "topic" => match &b.shared_topic {
            Some(t) => format!("shared topic: {t}"),
            None => "shared topics".to_string(),
        },
        "project" => "same project".to_string(),
        "temporal" => format!("captured {} day(s) apart", b.days_apart),
        _ => "connected".to_string(),
    }
}

fn topic_label_set(memory: &Memory) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(tag) = memory.primary_topic.as_deref() {
        set.insert(tag.to_lowercase());
    }
    if let Some(json) = memory.topic_labels.as_ref() {
        for tag in &json.0 {
            set.insert(tag.to_lowercase());
        }
    }
    set
}

fn entity_signature_set(rows: Vec<crate::models::MemoryEntityRow>) -> HashSet<String> {
    rows.into_iter()
        .map(|row| format!("{}:{}", row.entity_type, row.entity_value.to_lowercase()))
        .collect()
}

/// Jaccard with one sample of an intersecting element returned
/// alongside, so the rationale can name a specific shared item.
fn jaccard_with_sample(
    a: &HashSet<String>,
    b: &HashSet<String>,
) -> (f32, Option<String>) {
    if a.is_empty() || b.is_empty() {
        return (0.0, None);
    }
    let intersection: Vec<&String> = a.intersection(b).collect();
    if intersection.is_empty() {
        return (0.0, None);
    }
    let union_size = a.union(b).count() as f32;
    let inter_size = intersection.len() as f32;
    let sample = intersection.first().map(|s| (*s).clone());
    (inter_size / union_size, sample)
}

/// Length-normalized entity overlap. Plain count would over-weight
/// memories with many entities; sqrt-normalize so a 2-of-3 match
/// scores higher than 2-of-30. Sample the strongest shared entity
/// (longest value, as a heuristic for specificity) for the rationale.
fn entity_overlap(
    a: &HashSet<String>,
    b: &HashSet<String>,
) -> (f32, Option<String>) {
    if a.is_empty() || b.is_empty() {
        return (0.0, None);
    }
    let shared: Vec<&String> = a.intersection(b).collect();
    if shared.is_empty() {
        return (0.0, None);
    }
    let denom = ((a.len() as f32) * (b.len() as f32)).sqrt().max(1.0);
    let raw = shared.len() as f32 / denom;
    let normalized = raw.min(1.0);
    // "shared:value" → strip the type prefix for the rationale
    let best = shared
        .into_iter()
        .max_by_key(|s| s.len())
        .map(|s| s.split_once(':').map(|(_, v)| v.to_string()).unwrap_or_else(|| s.clone()));
    (normalized, best)
}

fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn is_self_capture(memory: &Memory) -> bool {
    memory
        .ocr_engine
        .as_deref()
        .map(|e| e.contains("self-capture"))
        .unwrap_or(false)
}

fn maybe_center(mut v: Vec<f32>, centroid: Option<&[f32]>) -> Vec<f32> {
    if let Some(c) = centroid {
        if c.len() == v.len() {
            for (slot, ref_val) in v.iter_mut().zip(c.iter()) {
                *slot -= *ref_val;
            }
        }
    }
    v
}

/// Walk the chronologically-ordered chain and keep only the
/// connected segment containing the seed under the
/// `MAX_GAP_DAYS` rule. A "gap" is any consecutive pair whose
/// `created_at` differs by more than the threshold.
fn trim_to_seed_segment(
    chain: Vec<TrailNode>,
    by_id: &HashMap<&str, &Memory>,
) -> Vec<TrailNode> {
    if chain.is_empty() {
        return chain;
    }
    let dates: Vec<Option<chrono::DateTime<chrono::Utc>>> = chain
        .iter()
        .map(|n| {
            by_id
                .get(n.memory_id.as_str())
                .and_then(|m| parse_iso(&m.created_at))
        })
        .collect();

    let seed_idx = match chain.iter().position(|n| n.is_seed) {
        Some(i) => i,
        None => return chain,
    };

    // Expand left while the previous gap fits the threshold
    let mut start = seed_idx;
    while start > 0 {
        let (a, b) = (&dates[start - 1], &dates[start]);
        if let (Some(a), Some(b)) = (a, b) {
            if (b.signed_duration_since(*a)).num_days().abs() > MAX_GAP_DAYS {
                break;
            }
        }
        start -= 1;
    }

    // Expand right while the next gap fits the threshold
    let mut end = seed_idx;
    while end + 1 < chain.len() {
        let (a, b) = (&dates[end], &dates[end + 1]);
        if let (Some(a), Some(b)) = (a, b) {
            if (b.signed_duration_since(*a)).num_days().abs() > MAX_GAP_DAYS {
                break;
            }
        }
        end += 1;
    }

    chain.into_iter().skip(start).take(end - start + 1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_empty_returns_zero() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = ["x".into()].into_iter().collect();
        let (score, sample) = jaccard_with_sample(&a, &b);
        assert_eq!(score, 0.0);
        assert!(sample.is_none());
    }

    #[test]
    fn jaccard_full_overlap_returns_one() {
        let a: HashSet<String> = ["x".into(), "y".into()].into_iter().collect();
        let b = a.clone();
        let (score, _) = jaccard_with_sample(&a, &b);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn entity_overlap_normalizes_by_size() {
        // 2 shared out of (3, 3) ≈ 2/3, vs 2 shared out of (3, 30) ≈ 2/√90.
        // The second case must score lower.
        let small: HashSet<String> = ["e:a".into(), "e:b".into(), "e:c".into()]
            .into_iter()
            .collect();
        let small_match: HashSet<String> = ["e:a".into(), "e:b".into(), "e:d".into()]
            .into_iter()
            .collect();
        let huge: HashSet<String> = (0..30).map(|i| format!("e:other{i}")).collect();
        let mut huge_match = huge.clone();
        huge_match.insert("e:a".into());
        huge_match.insert("e:b".into());

        let (s_small, _) = entity_overlap(&small, &small_match);
        let (s_huge, _) = entity_overlap(&small, &huge_match);
        assert!(s_small > s_huge);
    }

    #[test]
    fn rationale_picks_dominant_signal() {
        let b = ScoreBreakdown {
            total: 0.55,
            semantic_weighted: 0.10,
            semantic_raw: 0.25,
            entity_weighted: 0.18,
            shared_entity: Some("Bharath".into()),
            topic_weighted: 0.12,
            shared_topic: Some("pricing".into()),
            project_weighted: 0.15,
            temporal_weighted: 0.00,
            days_apart: 4,
        };
        // Entity weighted (0.18) is highest → rationale names it.
        assert_eq!(rationale_for(&b), "shared entity: Bharath");
    }

    // Note: end-to-end behavior of `trim_to_seed_segment` is
    // covered by integration tests against a seeded SQLite DB,
    // not unit tests. Constructing a fully-shaped Memory in unit
    // code drifts every time the model gets a new column; the
    // trim logic itself is small enough that the integration
    // path is the right place.
}
