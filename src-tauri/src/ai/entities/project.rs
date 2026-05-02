//! Project detector.
//!
//! Matches against the user's actual project names from the
//! `projects` table — no guessing. The caller passes a slice of
//! (project_id, project_name) pairs; we scan the content for any
//! word-boundary occurrence of each name.
//!
//! Confidence: high (0.9). Project names are user-defined and
//! match against a closed set.

use super::{Entity, EntityType};

pub fn detect(content: &str, known_projects: &[(String, String)]) -> Vec<Entity> {
    if known_projects.is_empty() {
        return Vec::new();
    }
    let lower = content.to_lowercase();
    let mut hits: Vec<Entity> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (_id, name) in known_projects {
        let trimmed = name.trim();
        // Skip 1-2 char project names — too ambiguous (would
        // match every "AI" or "ML" mention as a project).
        if trimmed.chars().count() < 3 {
            continue;
        }
        let needle = trimmed.to_lowercase();
        if find_word(&lower, &needle).is_some()
            && seen.insert(needle.clone())
        {
            hits.push(Entity {
                entity_type: EntityType::Project,
                entity_value: trimmed.to_string(),
                raw_match: trimmed.to_string(),
                confidence: 0.9,
            });
        }
    }
    hits
}

fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(idx) = haystack[start..].find(needle) {
        let absolute = start + idx;
        let before_ok = absolute == 0
            || !haystack
                .as_bytes()
                .get(absolute - 1)
                .map(|b| b.is_ascii_alphanumeric())
                .unwrap_or(false);
        let after_ok = haystack
            .as_bytes()
            .get(absolute + needle.len())
            .map(|b| !b.is_ascii_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return Some(absolute);
        }
        start = absolute + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_user_project_name() {
        let projects = vec![
            ("p1".to_string(), "Recall".to_string()),
            ("p2".to_string(), "Phoenix".to_string()),
        ];
        let entities = detect("Working on Recall today.", &projects);
        assert!(entities.iter().any(|e| e.entity_value == "Recall"));
    }

    #[test]
    fn ignores_substring_matches() {
        let projects = vec![("p1".to_string(), "Phoenix".to_string())];
        let entities = detect("Phoenixville is a town in PA.", &projects);
        assert!(entities.iter().all(|e| e.entity_value != "Phoenix"));
    }

    #[test]
    fn skips_too_short_project_names() {
        // 1-2 char project names get filtered to avoid
        // false-matching "AI"/"ML" mentions everywhere.
        let projects = vec![("p1".to_string(), "AI".to_string())];
        let entities = detect("Discussing AI all day.", &projects);
        assert!(entities.is_empty());
    }

    #[test]
    fn empty_projects_returns_empty() {
        let entities = detect("Working on something today.", &[]);
        assert!(entities.is_empty());
    }
}
