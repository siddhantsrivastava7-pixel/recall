//! Tag-intent detector for Ask Recall queries.
//!
//! v0.5.5. The auto-tagger (v0.3.7) tags opaque content with topic
//! labels — `license-key`, `url`, `email`, `phone-number`,
//! `ip-address`, `code-snippet`, `hash`. The semantic+keyword
//! ranker uses those tags as one signal among many. But for
//! enumeration questions ("what license keys did I save", "list
//! the URLs I bookmarked"), cosine similarity between the query
//! and the actual content is too weak to consistently surface
//! every member of the class — the model picks the one or two
//! that happen to embed nearest the query phrase and drops the
//! rest below the SEMANTIC_FLOOR.
//!
//! The fix is a separate retrieval path: when the query phrase
//! semantically matches a known tag class, fetch *every* memory
//! with that tag directly from the DB (no cosine, no floor) and
//! merge them into the candidate set ahead of the semantic ranks.
//!
//! Detection is a hand-curated phrase list — same approach as the
//! temporal detector. We could embed the query and cosine-match
//! against tag descriptions but that re-introduces the same
//! "embeddings can't tell similar concepts apart" problem we're
//! trying to escape.

/// A query that matched a known tag intent. The frontend never
/// sees this — it's pure backend routing.
#[derive(Debug, Clone)]
pub struct TagIntent {
    /// The auto-tag class we should pivot on (e.g. "license-key").
    /// Matches the values returned by `auto_tagger::detect_tags`.
    pub tag: &'static str,
    /// Human-readable label for the system prompt's enumeration
    /// instruction (e.g. "license keys"). Plural form.
    pub label: &'static str,
}

/// Match the query against known tag-intent phrases. Returns the
/// first match (longest phrase first via the lookup table's
/// ordering). Case-insensitive substring match.
///
/// Returns `None` for queries that don't reference a known class —
/// those continue through pure semantic+keyword ranking.
pub fn detect(question: &str) -> Option<TagIntent> {
    let lower = question.to_lowercase();

    // Longest, most specific phrases first so "license keys" matches
    // before a hypothetical bare "keys" (which we deliberately don't
    // include — too ambiguous).
    const PHRASES: &[(&str, &str, &str)] = &[
        // (phrase, tag, label)
        ("license keys", "license-key", "license keys"),
        ("license key", "license-key", "license keys"),
        ("activation keys", "license-key", "license keys"),
        ("activation key", "license-key", "license keys"),
        ("product keys", "license-key", "license keys"),
        ("product key", "license-key", "license keys"),
        ("phone numbers", "phone-number", "phone numbers"),
        ("phone number", "phone-number", "phone numbers"),
        ("contact numbers", "phone-number", "phone numbers"),
        ("ip addresses", "ip-address", "IP addresses"),
        ("ip address", "ip-address", "IP addresses"),
        ("email addresses", "email", "email addresses"),
        ("emails", "email", "email addresses"),
        ("code snippets", "code-snippet", "code snippets"),
        ("code snippet", "code-snippet", "code snippets"),
        ("scripts", "code-snippet", "code snippets"),
        ("urls", "url", "URLs"),
        ("websites", "url", "URLs"),
        ("links", "url", "URLs"),
        ("hashes", "hash", "hashes"),
        ("checksums", "hash", "hashes"),
    ];

    for (phrase, tag, label) in PHRASES {
        if lower.contains(phrase) {
            return Some(TagIntent { tag, label });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_license_key() {
        let intent = detect("what license keys did i save").expect("detected");
        assert_eq!(intent.tag, "license-key");
    }

    #[test]
    fn detects_license_key_singular() {
        let intent = detect("license key i saved?").expect("detected");
        assert_eq!(intent.tag, "license-key");
    }

    #[test]
    fn detects_url() {
        let intent = detect("show me the urls i saved").expect("detected");
        assert_eq!(intent.tag, "url");
    }

    #[test]
    fn detects_links_synonym() {
        let intent = detect("what links have i bookmarked").expect("detected");
        assert_eq!(intent.tag, "url");
    }

    #[test]
    fn detects_phone() {
        let intent = detect("phone numbers i have saved").expect("detected");
        assert_eq!(intent.tag, "phone-number");
    }

    #[test]
    fn no_match_for_general_query() {
        assert!(detect("what was that meeting about pricing").is_none());
    }

    #[test]
    fn does_not_match_bare_keys() {
        // "keys" alone is too ambiguous (could mean keyboard keys,
        // map keys, etc.) — only specific phrases match.
        assert!(detect("the keys to success").is_none());
    }
}
