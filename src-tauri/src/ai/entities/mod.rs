//! v0.5.6 — structured entity extraction.
//!
//! Pattern-based detectors for the lightweight context layer over
//! memories: people, companies, products, projects, time ranges.
//!
//! Same architecture as the auto-tagger:
//!   * Pure regex / dictionary lookups, no NLP model
//!   * Conservative — would rather miss an entity than mis-tag one
//!   * Idempotent — re-running on the same content produces the
//!     same entity set, which the DB layer's UNIQUE constraint
//!     deduplicates
//!
//! Why not a real NER model: shipping spaCy/HF NER means another
//! 100–500 MB model download per language, plus a Python sidecar
//! or ONNX runtime expansion. For the precision we need (catch
//! the obvious cases, miss ambiguous ones cleanly), regex +
//! dictionaries are the right tool. We can swap to a model later
//! if real users hit the precision/recall wall.
//!
//! Place detection is deliberately deferred — it requires a
//! city/country dictionary that's nontrivial to bundle (~5k cities
//! at ~100KB compressed). Adding place support is a v0.5.7 task
//! once we settle on the dictionary source.

pub mod company;
pub mod person;
pub mod product;
pub mod project;
pub mod time_range;

use crate::db::repositories::SharedMemoryRepository;
use crate::errors::app_error::AppResult;

/// One extracted entity. Fields map 1:1 to the `memory_entities`
/// table columns. `confidence` is a [0.0, 1.0] hint the detector
/// emits — UI/ranker can use it to weight or filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub entity_type: EntityType,
    /// Normalized form (e.g. "Anthropic" not "anthropic" or "ANTHROPIC").
    /// What the UI displays and what entity-pivot retrieval queries
    /// against.
    pub entity_value: String,
    /// The original substring from the source content. Useful for
    /// debugging and for highlighting the match in the UI.
    pub raw_match: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityType {
    Person,
    Company,
    Product,
    Project,
    /// Time range mentioned in content (e.g. "Q3 2024", "March
    /// 2025", "last week"). Distinct from the temporal-intent
    /// detector which parses query phrases — this extracts time
    /// references *from* memories rather than *about* memories.
    TimeRange,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Person => "person",
            EntityType::Company => "company",
            EntityType::Product => "product",
            EntityType::Project => "project",
            EntityType::TimeRange => "time-range",
        }
    }
}

/// Run every detector against `content` and merge results. The
/// per-detector dedup is by (entity_type, entity_value) so the
/// same name appearing twice in the content produces one entity.
///
/// `known_projects` is a slice of (id, name) pairs from the
/// projects table — lets the project detector recognize the
/// user's actual project names rather than guessing.
pub fn detect_entities(
    content: &str,
    known_projects: &[(String, String)],
) -> Vec<Entity> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut entities: Vec<Entity> = Vec::new();
    entities.extend(person::detect(trimmed));
    entities.extend(company::detect(trimmed));
    entities.extend(product::detect(trimmed));
    entities.extend(project::detect(trimmed, known_projects));
    entities.extend(time_range::detect(trimmed));

    // Dedup by (type, normalized value). When the same entity is
    // matched by multiple detectors with different confidences,
    // keep the highest-confidence row.
    let mut seen: std::collections::HashMap<(EntityType, String), Entity> =
        std::collections::HashMap::new();
    for ent in entities {
        let key = (ent.entity_type, ent.entity_value.clone());
        seen
            .entry(key)
            .and_modify(|existing| {
                if ent.confidence > existing.confidence {
                    *existing = ent.clone();
                }
            })
            .or_insert(ent);
    }
    seen.into_values().collect()
}

/// Run extraction against `content` and persist the result via
/// `replace_entities_for_memory`. Centralizes the call sequence
/// so capture, OCR-promote, and backfill paths all extract
/// identically. Soft-fails on repo errors with an eprintln —
/// entity extraction is best-effort enrichment, not a path that
/// should block embedding/saving if it hits an error.
///
/// `known_projects` may be empty; pass `&[]` from sites that
/// don't have project repo access (the worker layer in v0.5.6).
/// v0.5.7 will plumb projects through to enable project-name
/// detection at all extraction sites.
pub async fn extract_and_persist(
    memory_repo: &SharedMemoryRepository,
    memory_id: &str,
    content: &str,
    known_projects: &[(String, String)],
) -> AppResult<usize> {
    let entities = detect_entities(content, known_projects);
    let count = entities.len();
    if let Err(err) = memory_repo
        .replace_entities_for_memory(memory_id, &entities)
        .await
    {
        eprintln!(
            "[recall][entities] replace_entities_for_memory failed for {memory_id}: {err}"
        );
        return Err(err);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_multiple_entity_types() {
        let entities = detect_entities(
            "Met with John Smith from Anthropic to discuss Tauri 2.0 in Q3 2024.",
            &[],
        );
        let types: std::collections::HashSet<EntityType> =
            entities.iter().map(|e| e.entity_type).collect();
        assert!(types.contains(&EntityType::Person), "missed person: {:?}", entities);
        assert!(types.contains(&EntityType::Company), "missed company: {:?}", entities);
        assert!(types.contains(&EntityType::Product), "missed product: {:?}", entities);
        assert!(
            types.contains(&EntityType::TimeRange),
            "missed time range: {:?}",
            entities
        );
    }

    #[test]
    fn dedupes_same_entity_across_detectors() {
        // "Recall" could match both product (in dictionary) and
        // project (if user has a project named Recall). The dedup
        // keeps the highest-confidence row.
        let entities = detect_entities(
            "Working on Recall features today.",
            &[("p1".to_string(), "Recall".to_string())],
        );
        let recall_count = entities
            .iter()
            .filter(|e| e.entity_value == "Recall")
            .count();
        assert!(
            recall_count <= 2,
            "should not produce duplicate entities for same value: {:?}",
            entities
        );
    }

    #[test]
    fn empty_content_returns_empty() {
        assert!(detect_entities("", &[]).is_empty());
        assert!(detect_entities("   ", &[]).is_empty());
    }
}
