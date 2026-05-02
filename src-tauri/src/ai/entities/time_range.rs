//! Time-range detector for memory content.
//!
//! Distinct from `ai/ask/temporal.rs` which parses temporal
//! intent from queries ("summarize my week"). This detector
//! extracts time references *from* memory content for entity
//! storage — so a memory body that says "Q3 2024 launch" gets
//! a `time-range` entity tagged "Q3 2024", queryable later via
//! entity-pivot retrieval.
//!
//! Patterns covered:
//!   * Quarter-year:        "Q3 2024", "Q1 '25"
//!   * Month-year:          "March 2024", "Sep 2025"
//!   * ISO dates:           "2024-03-15"
//!   * Year alone:          "2024"
//!
//! Skipping for now: relative phrases ("last week", "yesterday")
//! — those carry meaning relative to *now* and don't make great
//! entities for retrieval. Better captured at query time, which
//! is what `ask/temporal.rs` already does.

use std::sync::OnceLock;

use regex::Regex;

use super::{Entity, EntityType};

pub fn detect(content: &str) -> Vec<Entity> {
    let mut hits: Vec<Entity> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Quarters: "Q3 2024", "Q1 '25"
    for cap in quarter_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str().trim();
        let normalized = normalize_quarter(raw);
        if seen.insert(normalized.clone()) {
            hits.push(Entity {
                entity_type: EntityType::TimeRange,
                entity_value: normalized.clone(),
                raw_match: raw.to_string(),
                confidence: 0.9,
            });
        }
    }

    // Month-year: "March 2024", "Sep 2025"
    for cap in month_year_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str().trim();
        let normalized = normalize_month_year(raw);
        if seen.insert(normalized.clone()) {
            hits.push(Entity {
                entity_type: EntityType::TimeRange,
                entity_value: normalized,
                raw_match: raw.to_string(),
                confidence: 0.85,
            });
        }
    }

    // ISO dates: "2024-03-15"
    for cap in iso_date_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str().trim();
        if seen.insert(raw.to_string()) {
            hits.push(Entity {
                entity_type: EntityType::TimeRange,
                entity_value: raw.to_string(),
                raw_match: raw.to_string(),
                confidence: 0.95,
            });
        }
    }

    // Year alone: requires word-boundary digits to look like a year
    // (1900–2099). Lower confidence because bare numbers can be
    // ambiguous with version numbers, prices, etc. We only
    // include it when no quarter/month already captured the year.
    for cap in year_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str().trim();
        // If we already have a quarter or month-year capturing
        // this year, skip — avoid duplicates like "Q3 2024" + "2024".
        if hits.iter().any(|h| h.entity_value.contains(raw)) {
            continue;
        }
        if seen.insert(raw.to_string()) {
            hits.push(Entity {
                entity_type: EntityType::TimeRange,
                entity_value: raw.to_string(),
                raw_match: raw.to_string(),
                confidence: 0.5,
            });
        }
    }

    hits
}

fn quarter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bQ([1-4])\s+(?:'?(\d{2}|\d{4}))").unwrap())
}

fn normalize_quarter(raw: &str) -> String {
    // Take the captured year, expand 2-digit to 20XX.
    let parts: Vec<&str> = raw
        .split(|c: char| c.is_whitespace() || c == '\'')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() == 2 {
        let q = parts[0];
        let y = parts[1];
        let year = if y.len() == 2 {
            format!("20{}", y)
        } else {
            y.to_string()
        };
        return format!("{} {}", q, year);
    }
    raw.to_string()
}

fn month_year_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"\b(?:January|February|March|April|May|June|July|August|September|October|November|December|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sept?|Oct|Nov|Dec)\s+(\d{4})\b",
        )
        .unwrap()
    })
}

fn normalize_month_year(raw: &str) -> String {
    // Expand abbreviated months to full form for consistent
    // entity_value across "Sep 2024" / "September 2024".
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() != 2 {
        return raw.to_string();
    }
    let month_full = expand_month(parts[0]);
    format!("{} {}", month_full, parts[1])
}

fn expand_month(abbr: &str) -> &'static str {
    match abbr.to_lowercase().as_str() {
        "jan" | "january" => "January",
        "feb" | "february" => "February",
        "mar" | "march" => "March",
        "apr" | "april" => "April",
        "may" => "May",
        "jun" | "june" => "June",
        "jul" | "july" => "July",
        "aug" | "august" => "August",
        "sep" | "sept" | "september" => "September",
        "oct" | "october" => "October",
        "nov" | "november" => "November",
        "dec" | "december" => "December",
        _ => "",
    }
}

fn iso_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap())
}

fn year_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Restricted to 1900–2099 to avoid matching version numbers
    // and other 4-digit incidentals.
    RE.get_or_init(|| Regex::new(r"\b(?:19\d{2}|20\d{2})\b").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_quarter_year() {
        let entities = detect("Launch planned for Q3 2024.");
        assert!(entities.iter().any(|e| e.entity_value == "Q3 2024"));
    }

    #[test]
    fn expands_two_digit_year_in_quarter() {
        let entities = detect("Target Q1 '25 for the rollout.");
        assert!(entities.iter().any(|e| e.entity_value == "Q1 2025"));
    }

    #[test]
    fn detects_iso_date() {
        let entities = detect("Meeting on 2024-03-15 at 2pm.");
        assert!(entities.iter().any(|e| e.entity_value == "2024-03-15"));
    }

    #[test]
    fn normalizes_abbreviated_month() {
        let entities = detect("See you in Sep 2024.");
        assert!(entities.iter().any(|e| e.entity_value == "September 2024"));
    }

    #[test]
    fn skips_redundant_year_when_quarter_present() {
        let entities = detect("Q3 2024 review");
        // Should have Q3 2024 but NOT also a separate "2024" entity.
        let count = entities.iter().filter(|e| e.entity_value == "2024").count();
        assert_eq!(count, 0);
    }

    #[test]
    fn rejects_non_year_4digit_numbers() {
        let entities = detect("Total revenue was 1234 dollars.");
        assert!(entities.iter().all(|e| e.entity_value != "1234"));
    }
}
