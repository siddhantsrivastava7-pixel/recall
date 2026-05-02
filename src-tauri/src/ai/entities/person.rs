//! Person-name detector.
//!
//! Looks for sequences of 2–3 capitalized words that don't match
//! known non-name capitalizations (months, days of week, common
//! sentence-start words, ALL-CAPS acronyms).
//!
//! Conservative bias — single capitalized words ("John") are too
//! ambiguous (could be a sentence-start verb, a month,
//! a place). We require at least two consecutive capitalized
//! words to fire.

use std::sync::OnceLock;

use regex::Regex;

use super::{Entity, EntityType};

/// Run the person-name detector against `content`. Returns one
/// `Entity` per distinct name found.
pub fn detect(content: &str) -> Vec<Entity> {
    let mut hits: Vec<Entity> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for cap in name_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str();
        if !looks_like_real_name(raw) {
            continue;
        }
        let normalized = raw.trim().to_string();
        if !seen.insert(normalized.clone()) {
            continue;
        }
        // 2-word names are slightly less reliable than 3-word
        // (more first-name+verb collisions like "Mark said").
        let confidence = if raw.split_whitespace().count() >= 3 {
            0.8
        } else {
            0.6
        };
        hits.push(Entity {
            entity_type: EntityType::Person,
            entity_value: normalized.clone(),
            raw_match: normalized,
            confidence,
        });
    }
    hits
}

/// 2–3 consecutive capitalized words. Each word: leading uppercase
/// followed by 2+ lowercase letters (so "JS" or "AI" don't match).
/// The trailing word boundary keeps "Smith. " from over-matching.
fn name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b([A-Z][a-z]{1,}(?:\s+[A-Z][a-z]{1,}){1,2})\b").unwrap()
    })
}

/// Filter out non-name capitalizations the regex can't catch on its
/// own. Months, days, and common bigrams that look like names
/// (e.g. "Last Tuesday", "New York") get rejected here.
fn looks_like_real_name(text: &str) -> bool {
    let lower = text.to_lowercase();

    // Month + day + possible year: "Last Tuesday", "March 2024"
    const NON_NAME_TOKENS: &[&str] = &[
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
        "spring",
        "summer",
        "autumn",
        "winter",
        "today",
        "tomorrow",
        "yesterday",
        "morning",
        "afternoon",
        "evening",
        "night",
    ];
    for token in NON_NAME_TOKENS {
        if lower.contains(token) {
            return false;
        }
    }

    // Two-word phrases that look like names but are place/concept
    // bigrams. Conservative list — we'd rather miss a place than
    // false-positive a person on "New York" or "San Francisco".
    const NON_NAME_BIGRAMS: &[&str] = &[
        "new york",
        "new jersey",
        "san francisco",
        "los angeles",
        "san diego",
        "las vegas",
        "hong kong",
        "south africa",
        "north america",
        "south america",
        "united states",
        "great britain",
        "ask recall",  // our own UI; even if self-capture filter misses
    ];
    for bigram in NON_NAME_BIGRAMS {
        if lower.contains(bigram) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_two_word_name() {
        let entities = detect("Met with Alice Walker today.");
        assert!(entities.iter().any(|e| e.entity_value == "Alice Walker"));
    }

    #[test]
    fn detects_three_word_name_with_higher_confidence() {
        let entities = detect("John Mark Smith joined the call.");
        let hit = entities
            .iter()
            .find(|e| e.entity_value == "John Mark Smith")
            .expect("matched");
        assert!(hit.confidence >= 0.7);
    }

    #[test]
    fn rejects_month_phrases() {
        let entities = detect("Met with John Smith on March 15.");
        assert!(entities.iter().all(|e| !e.entity_value.contains("March")));
    }

    #[test]
    fn rejects_place_bigrams() {
        let entities = detect("Visiting New York with Alice Walker.");
        assert!(entities.iter().all(|e| e.entity_value != "New York"));
        assert!(entities.iter().any(|e| e.entity_value == "Alice Walker"));
    }

    #[test]
    fn rejects_short_uppercase_acronyms() {
        let entities = detect("Discussing AI and ML with John Smith.");
        assert!(entities.iter().all(|e| e.entity_value != "AI ML"));
    }

    #[test]
    fn deduplicates_same_name() {
        let entities = detect("Alice Walker said. Alice Walker also said.");
        let count = entities
            .iter()
            .filter(|e| e.entity_value == "Alice Walker")
            .count();
        assert_eq!(count, 1);
    }
}
