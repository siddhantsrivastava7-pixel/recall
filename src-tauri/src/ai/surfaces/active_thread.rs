//! v0.5.59 — Active Thread surface.
//!
//! Where Memory Trails answer "show me the thread of my thinking
//! on this seed across time," Active Thread answers a different
//! question: "what topic am I actively working on right now?"
//!
//! The picker pulls the last 14 days of saved memories, finds
//! clusters bound by shared topic_labels / entities / project,
//! and surfaces the strongest cluster as a Home card. Selection
//! sits between Weekly Recap and Forgotten Gold:
//!
//!   1. Weekly Recap      — Monday-morning, look-back card.
//!   2. **Active Thread**  — "you're working on X this week."
//!   3. Forgotten Gold    — older memory worth revisiting.
//!
//! The product rule from v0.5.23 stays intact: at most one
//! surface visible at a time. If a strong active thread exists,
//! it beats Forgotten Gold for the day; if not, the slot falls
//! through to Forgotten Gold as before.
//!
//! ## Scoring
//!
//! For each connected component in the 14-day window's
//! shared-feature graph:
//!
//! ```text
//!   thread_score =
//!       0.45 · density            (count / window_days, capped at 1)
//!     + 0.30 · recency_decay      (exp(-days_since_latest / 5))
//!     + 0.25 · entity_repeat      (most-frequent shared feature
//!                                  occurrence rate within cluster)
//! ```
//!
//! Edges in the feature graph fire when two memories share at
//! least 2 features (topic_labels + entity signatures + project).
//! Pure semantic similarity is **not** used here — we measured
//! and feature overlap is a sharper signal for "active topic"
//! than cosine in the recent window. Cosine still drives Memory
//! Trails on detail view.
//!
//! ## What this module does NOT do
//!
//! * No persistence layer of its own — uses the existing
//!   `proactive_surfaces` table the engine already manages.
//! * No new clusters table. Clusters are recomputed every time
//!   the picker runs (cheap on a 14-day window — ≤120 memories,
//!   one entity query per memory, in-memory union-find).
//! * No LLM. Pure structured signals.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::AppResult,
    models::Memory,
};

/// One active-thread candidate the engine can pass into the
/// generic ProactiveSurface persistence path. The "memory" used
/// as the surface row's `memory_id` is the most-recent member of
/// the cluster — clicking the Home card opens that memory's
/// detail view, where the v0.5.58 Memory Trail then lays out
/// the chain visually.
#[derive(Debug, Clone)]
pub struct ActiveThreadCandidate {
    /// Most-recent memory in the cluster. The Home card uses
    /// this as the navigation target on click.
    pub representative_memory_id: String,
    /// Number of memories in the cluster.
    pub count: usize,
    /// Days between earliest and latest member.
    pub span_days: i64,
    /// Short label for the surface card subtitle, derived from
    /// the most-frequent shared feature (topic_label or entity).
    pub label: String,
    /// 0..1 — comparable across calls but only roughly comparable
    /// across kinds. Used to log the "why this card" rationale,
    /// not to rank against other surface kinds.
    pub score: f32,
}

const WINDOW_DAYS: i64 = 14;
/// Cap on memories examined per pick. Most users stay well under
/// 120 captures in a 14-day window; the cap makes the union-find
/// pass O(120²) ≈ 14k iterations even for power users.
const MAX_WINDOW_MEMORIES: usize = 120;
const MIN_NODES: usize = 3;
const MIN_SPAN_DAYS: i64 = 3;
/// Latest member of the cluster must be within this many days of
/// now, otherwise the thread isn't "active" — it's "recent."
const RECENT_DAYS: i64 = 3;
const MIN_SHARED_FEATURES: usize = 2;

/// One memory's feature set inside the 14-day window. Built once
/// per pick, used by every helper below.
struct Feat {
    topics: HashSet<String>,
    entities: HashSet<String>,
    project: Option<String>,
}

/// Compute the strongest active-thread candidate in the last 14
/// days, or `None` when no cluster qualifies. Self-captures
/// (Recall UI screenshots) and ask-recall outputs are excluded
/// from the candidate pool — they would inflate edges with
/// vocabulary overlap that doesn't reflect real saved context.
pub async fn pick_active_thread(
    memory_repo: &SharedMemoryRepository,
) -> AppResult<Option<ActiveThreadCandidate>> {
    let now = Utc::now();
    let window_start_iso = (now - Duration::days(WINDOW_DAYS)).to_rfc3339();

    let mut all = memory_repo.list().await?;
    all.retain(|m| {
        m.created_at.as_str() >= window_start_iso.as_str()
            && !is_self_capture(m)
            && m.source_app.as_deref() != Some("ask-recall")
    });
    if all.len() < MIN_NODES {
        return Ok(None);
    }
    // Newest first, cap to bound the pairwise scan.
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    all.truncate(MAX_WINDOW_MEMORIES);

    // Build feature sets per memory. One entity query per memory;
    // the topic-label set is read directly from the in-memory
    // Memory shape.
    let mut feats: Vec<Feat> = Vec::with_capacity(all.len());
    for memory in &all {
        let topics = topic_label_set(memory);
        let entities = memory_repo
            .list_entities_for_memory(&memory.id)
            .await
            .ok()
            .map(|rows| {
                rows.into_iter()
                    .map(|r| format!("{}:{}", r.entity_type, r.entity_value.to_lowercase()))
                    .collect::<HashSet<String>>()
            })
            .unwrap_or_default();
        feats.push(Feat {
            topics,
            entities,
            project: memory.project_id.clone(),
        });
    }

    // O(N²) edge build. N ≤ MAX_WINDOW_MEMORIES so this is bounded.
    let mut adj: HashMap<usize, HashSet<usize>> = HashMap::new();
    for i in 0..feats.len() {
        for j in (i + 1)..feats.len() {
            if shared_feature_count(&feats[i], &feats[j]) >= MIN_SHARED_FEATURES {
                adj.entry(i).or_default().insert(j);
                adj.entry(j).or_default().insert(i);
            }
        }
    }

    // Connected components via DFS.
    let mut visited = vec![false; feats.len()];
    let mut components: Vec<Vec<usize>> = Vec::new();
    for start in 0..feats.len() {
        if visited[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(idx) = stack.pop() {
            if visited[idx] {
                continue;
            }
            visited[idx] = true;
            component.push(idx);
            if let Some(neighbors) = adj.get(&idx) {
                for &n in neighbors {
                    if !visited[n] {
                        stack.push(n);
                    }
                }
            }
        }
        components.push(component);
    }

    // Filter + score. Track the strongest qualifying cluster.
    let mut best: Option<ActiveThreadCandidate> = None;
    for component in components {
        if component.len() < MIN_NODES {
            continue;
        }

        let mut earliest: Option<DateTime<Utc>> = None;
        let mut latest: Option<DateTime<Utc>> = None;
        for &idx in &component {
            let Some(dt) = parse_iso(&all[idx].created_at) else {
                continue;
            };
            earliest = Some(earliest.map(|e| e.min(dt)).unwrap_or(dt));
            latest = Some(latest.map(|l| l.max(dt)).unwrap_or(dt));
        }
        let (Some(earliest_dt), Some(latest_dt)) = (earliest, latest) else {
            continue;
        };
        let span_days = (latest_dt - earliest_dt).num_days();
        if span_days < MIN_SPAN_DAYS {
            continue;
        }
        // The latest member must be near "now," otherwise this is
        // a finished topic the user has moved on from.
        let days_since_latest = (now - latest_dt).num_days();
        if days_since_latest > RECENT_DAYS {
            continue;
        }

        let density = (component.len() as f32 / WINDOW_DAYS as f32).min(1.0);
        let recency_decay = (-(days_since_latest as f32) / 5.0).exp();
        let (entity_repeat, label) = dominant_feature(&component, &feats);
        let score = 0.45 * density + 0.30 * recency_decay + 0.25 * entity_repeat;

        let representative_memory_id = component
            .iter()
            .copied()
            .max_by_key(|&idx| all[idx].created_at.clone())
            .map(|idx| all[idx].id.clone())
            .unwrap_or_default();

        let candidate = ActiveThreadCandidate {
            representative_memory_id,
            count: component.len(),
            span_days,
            label,
            score,
        };

        if best
            .as_ref()
            .map(|b| b.score < candidate.score)
            .unwrap_or(true)
        {
            best = Some(candidate);
        }
    }

    Ok(best)
}

/// Count shared features across topic_labels, entities, and
/// project_id. Project counts as 1 if both have the same value.
fn shared_feature_count(a: &Feat, b: &Feat) -> usize {
    let mut count = a.topics.intersection(&b.topics).count();
    count += a.entities.intersection(&b.entities).count();
    if let (Some(pa), Some(pb)) = (a.project.as_deref(), b.project.as_deref()) {
        if pa == pb {
            count += 1;
        }
    }
    count
}

/// Pick the most-frequent shared feature across the cluster — its
/// rate (occurrences / cluster size) is the entity-repeat signal,
/// and the feature itself becomes the user-facing label. Topics
/// preferred over entities for the label (more legible), entities
/// preferred over project (more specific).
fn dominant_feature(component: &[usize], feats: &[Feat]) -> (f32, String) {
    let mut topic_counts: HashMap<String, usize> = HashMap::new();
    let mut entity_counts: HashMap<String, usize> = HashMap::new();
    let mut project_counts: HashMap<String, usize> = HashMap::new();
    for &idx in component {
        for t in &feats[idx].topics {
            *topic_counts.entry(t.clone()).or_insert(0) += 1;
        }
        for e in &feats[idx].entities {
            *entity_counts.entry(e.clone()).or_insert(0) += 1;
        }
        if let Some(p) = feats[idx].project.as_deref() {
            *project_counts.entry(p.to_string()).or_insert(0) += 1;
        }
    }

    // Pick the highest-occurrence shared feature, keeping the
    // category that yielded it for label formatting.
    let top_topic = topic_counts.iter().max_by_key(|&(_, &c)| c);
    let top_entity = entity_counts.iter().max_by_key(|&(_, &c)| c);
    let top_project = project_counts.iter().max_by_key(|&(_, &c)| c);

    // Order of preference: topic > entity > project. Within each,
    // pick only when count > 1 (a singleton isn't a "shared"
    // feature for cluster characterization).
    let cluster_size = component.len() as f32;
    if let Some((label, &count)) = top_topic {
        if count > 1 {
            let rate = (count as f32 / cluster_size).min(1.0);
            return (rate, format!("shared topic: {label}"));
        }
    }
    if let Some((entity, &count)) = top_entity {
        if count > 1 {
            // Entity signature is "type:value"; strip the type for
            // the user-facing label.
            let pretty = entity.split_once(':').map(|(_, v)| v).unwrap_or(entity);
            let rate = (count as f32 / cluster_size).min(1.0);
            return (rate, format!("shared entity: {pretty}"));
        }
    }
    if let Some((project, &count)) = top_project {
        if count > 1 {
            let rate = (count as f32 / cluster_size).min(1.0);
            return (rate, format!("project: {project}"));
        }
    }
    // Fall through: the cluster is tied together by feature
    // overlap that's spread thinly. Generic label, low signal.
    (0.0, "Active thread".to_string())
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

fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn is_self_capture(memory: &Memory) -> bool {
    memory
        .ocr_engine
        .as_deref()
        .map(|e| e.contains("self-capture"))
        .unwrap_or(false)
}
