//! Company-name detector.
//!
//! Two paths:
//!   1. Capitalized word(s) followed by a corporate suffix
//!      (Inc, LLC, Corp, Ltd, GmbH, AG, SA, BV, etc.) — high
//!      confidence, near-zero false positive rate.
//!   2. Whitelist match against a curated list of well-known
//!      tech/business companies — case-insensitive, word-boundary.
//!      Medium confidence because some names ("Apple") have
//!      common-noun collisions; we tag them anyway since they're
//!      almost always meant as the company in personal-memory
//!      context.

use std::sync::OnceLock;

use regex::Regex;

use super::{Entity, EntityType};

pub fn detect(content: &str) -> Vec<Entity> {
    let mut hits: Vec<Entity> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Path 1: suffix-based detection.
    for cap in suffix_re().captures_iter(content) {
        let raw = cap.get(0).unwrap().as_str();
        let normalized = raw.trim().to_string();
        if !seen.insert(normalized.to_lowercase()) {
            continue;
        }
        hits.push(Entity {
            entity_type: EntityType::Company,
            entity_value: normalized.clone(),
            raw_match: normalized,
            confidence: 0.9,
        });
    }

    // Path 2: whitelist match.
    let lower = content.to_lowercase();
    for known in WHITELIST {
        let known_lower = known.to_lowercase();
        // Word-boundary match — "Apple" matches but not "Pineapples".
        if find_word(&lower, &known_lower).is_some()
            && seen.insert(known_lower.clone())
        {
            hits.push(Entity {
                entity_type: EntityType::Company,
                entity_value: (*known).to_string(),
                raw_match: (*known).to_string(),
                confidence: 0.7,
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

/// Match "Foo Inc", "Foo Bar Corp", "Foo, LLC", etc. The leading
/// capitalized run is captured; the suffix kept in the match for
/// context but trimmed when normalizing the name.
fn suffix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"\b(?:[A-Z][A-Za-z0-9&]*(?:\s+[A-Z][A-Za-z0-9&]*){0,3})\s*,?\s+(?:Inc\.?|LLC|Ltd\.?|Corp\.?|Corporation|GmbH|AG|SA|BV|PLC|PBC|S\.A\.|Pty\.?\s*Ltd\.?)\b",
        )
        .unwrap()
    })
}

/// Curated list of well-known companies. Hand-maintained — we'd
/// rather miss obscure ones than pull in a 50k-row dictionary that
/// false-matches common nouns. Ordered roughly by usage frequency
/// in personal-memory contexts.
///
/// Keep the canonical capitalization here — that's what gets
/// stored as `entity_value` so UI shows "OpenAI" not "openai".
const WHITELIST: &[&str] = &[
    // AI / ML
    "Anthropic",
    "OpenAI",
    "Hugging Face",
    "Stability AI",
    "Cohere",
    "Mistral",
    "Perplexity",
    "Replicate",
    "DeepMind",
    "xAI",
    // Big tech
    "Google",
    "Microsoft",
    "Apple",
    "Amazon",
    "Meta",
    "Facebook",
    "Netflix",
    "Tesla",
    "Nvidia",
    "Intel",
    "AMD",
    "Adobe",
    "Salesforce",
    "Oracle",
    "IBM",
    // Dev / infra
    "GitHub",
    "GitLab",
    "Vercel",
    "Netlify",
    "Cloudflare",
    "Stripe",
    "Plaid",
    "Twilio",
    "SendGrid",
    "Linear",
    "Notion",
    "Figma",
    "Slack",
    "Discord",
    "Zoom",
    "Atlassian",
    "Jira",
    "Trello",
    "Asana",
    "ClickUp",
    "Monday",
    // Cloud
    "AWS",
    "GCP",
    "Azure",
    "DigitalOcean",
    "Heroku",
    "Render",
    "Fly.io",
    "Supabase",
    "Firebase",
    "PlanetScale",
    "Neon",
    // Browsers / OS
    "Mozilla",
    "Chrome",
    "Firefox",
    "Safari",
    "Brave",
    "Arc",
    // Finance / payments
    "PayPal",
    "Square",
    "Block",
    "Coinbase",
    "Binance",
    "Robinhood",
    // Media / consumer
    "Spotify",
    "YouTube",
    "Twitter",
    "X",
    "Reddit",
    "TikTok",
    "Snapchat",
    "LinkedIn",
    "Pinterest",
    "Instagram",
    // Hardware / mobile
    "Samsung",
    "Sony",
    "LG",
    "Dell",
    "HP",
    "Lenovo",
    "Asus",
    "Razer",
    // Indian giants (user mentioned amazon.in)
    "Flipkart",
    "Reliance",
    "Tata",
    "Infosys",
    "Wipro",
    "Zomato",
    "Swiggy",
    "Paytm",
    "PhonePe",
    "Razorpay",
    "Freshworks",
    "Zoho",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_company_with_inc_suffix() {
        let entities = detect("Got an offer from Acme Corp yesterday.");
        assert!(entities.iter().any(|e| e.entity_value.starts_with("Acme")));
    }

    #[test]
    fn detects_whitelist_match() {
        let entities = detect("Reading the Anthropic blog post on Claude.");
        assert!(entities.iter().any(|e| e.entity_value == "Anthropic"));
    }

    #[test]
    fn deduplicates_same_company() {
        let entities = detect("Anthropic released. Anthropic announced.");
        let count = entities
            .iter()
            .filter(|e| e.entity_value == "Anthropic")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn rejects_substring_matches() {
        // "apples" should not match "Apple"
        let entities = detect("I bought some apples today.");
        assert!(entities.iter().all(|e| e.entity_value != "Apple"));
    }
}
