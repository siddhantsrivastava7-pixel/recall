//! Pattern-based auto-tagger for memory content.
//!
//! v0.3.7. Dense embeddings can't infer the *meaning* of opaque
//! tokens that don't appear in natural-language training data —
//! a license key like `RC-2K4F-9X8M-PLAQ` embeds as random
//! uppercase noise, orthogonal to any English query about
//! "license keys". Same problem with URLs, emails, phone numbers,
//! crypto addresses, and code identifiers.
//!
//! The fix is to enrich each memory with descriptive tags at
//! capture time. The tags are stored on `memories.topic_labels`
//! and are folded into the embedded text alongside the title, so
//! queries about the *concept* ("license key", "url", "phone
//! number") match the *literal tag* in the embedding — bridging
//! the semantic gap that the raw content can't bridge alone.
//!
//! Tags are intentionally conservative — we'd rather miss a tag
//! than mis-tag a memory. Each detector is regex-only, no NLP
//! heuristics, no probability thresholds. If the patterns match,
//! the tag fires.

use std::sync::OnceLock;

use regex::Regex;

/// v0.5.7: every tag value the auto-tagger may emit. Used by the
/// backfill path to know which existing tags to scrub before
/// re-running detection — without this list, false positives from
/// earlier (looser) regexes would never be removed.
pub const MANAGED_TAGS: &[&str] = &[
    "license-key",
    "email",
    "url",
    "phone-number",
    "ip-address",
    "code-snippet",
    "hash",
];

/// Pattern detectors run in order; each returns `Some(tag)` when its
/// pattern matches, `None` otherwise. Multiple detectors can fire on
/// the same memory — a captured Discord message containing a URL and
/// a license key gets both tags.
pub fn detect_tags(content: &str) -> Vec<&'static str> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut tags = Vec::new();

    // v0.5.6: gather URL spans first so we can skip license-key
    // detection inside them. URLs frequently contain UUID-like
    // segments (Amazon product IDs, S3 keys, OAuth state) whose
    // shape happens to match the license-key regex's structure
    // but which aren't license keys at all. Without this filter,
    // a single bookmark to amazon.com/stores/page/A2B974BA-C68F-...
    // tags as `license-key` and pollutes the tag-pivot retrieval
    // for "what license keys did I save".
    let url_spans: Vec<(usize, usize)> = url_re()
        .find_iter(trimmed)
        .map(|m| (m.start(), m.end()))
        .collect();
    let in_url = |pos: usize| url_spans.iter().any(|(s, e)| pos >= *s && pos < *e);

    if license_key_re()
        .find_iter(trimmed)
        .any(|m| !in_url(m.start()) && looks_like_real_license_key(m.as_str()))
    {
        tags.push("license-key");
    }
    if email_re().is_match(trimmed) {
        tags.push("email");
    }
    if !url_spans.is_empty() {
        tags.push("url");
    }
    if phone_re().is_match(trimmed) {
        tags.push("phone-number");
    }
    if ip_re().is_match(trimmed) {
        tags.push("ip-address");
    }
    if looks_like_code(trimmed) {
        tags.push("code-snippet");
    }
    if hash_re().is_match(trimmed) {
        tags.push("hash");
    }

    tags
}

/// v0.5.6: post-regex validator for license-key matches. The base
/// pattern allows any alphanumeric blocks, which means UUIDs
/// (8-4-4-4-12 pure hex) match it through their middle segments
/// (`C68F-4742-A7E4` is a valid sub-match of the regex). UUIDs are
/// the dominant false-positive source from URLs and bookmark
/// metadata — we filter them out by requiring at least one non-hex
/// letter (G–Z, ignoring case) somewhere in the matched string.
///
/// Real license keys almost always contain at least one non-hex
/// letter — issuers pick from the full alphabet to maximize the key
/// space. The user's "RC-TRIAL-..." keys all contain T/I/L/R, all
/// of which are non-hex. UUIDs are constrained to 0-9 + A-F by
/// definition. The check is case-insensitive so lowercase keys
/// (rare but possible) still pass.
fn looks_like_real_license_key(matched: &str) -> bool {
    matched
        .chars()
        .any(|c| matches!(c, 'g'..='z' | 'G'..='Z'))
}

/// License-key shape: 2–5 uppercase alphanumeric chars, then 2–4
/// dash-separated alphanumeric blocks of 3–8 chars each. Matches
/// `RC-2K4F-9X8M`, `RECALL-XXXX-YYYY-ZZZZ`, etc. Tightened to
/// require all-uppercase blocks so we don't accidentally tag a
/// stray hyphenated word as a key.
fn license_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b[A-Z][A-Z0-9]{1,4}(-[A-Z0-9]{3,8}){2,4}\b").unwrap()
    })
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap())
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^\s]+").unwrap())
}

/// Phone numbers: optional `+` country code, then 10–15 digits with
/// optional spaces, dashes, parentheses, dots between groups.
/// Conservative — won't match bare 10-digit numbers without
/// formatting (those are too easy to confuse with order numbers,
/// timestamps, etc.).
fn phone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\+?\d{1,3}[\s\-.]\(?\d{2,4}\)?[\s\-.]\d{2,4}[\s\-.]\d{2,4}").unwrap()
    })
}

fn ip_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap())
}

/// SHA / MD5 / Bitcoin / Ethereum address shapes. Long-ish strings
/// of hex chars typically mean a hash or address — useful to tag
/// because they're textbook opaque tokens.
fn hash_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:0x)?[a-fA-F0-9]{32,64}\b").unwrap())
}

/// Cheap code heuristic: presence of two or more code-shaped
/// tokens (function calls, keywords, brace blocks, semicolons)
/// in a single memory suggests it's a code snippet rather than
/// prose. Tag-only — doesn't try to detect the language.
fn looks_like_code(content: &str) -> bool {
    let lowercased = content.to_lowercase();
    let mut hits = 0;
    let signals: &[&str] = &[
        "function ",
        "fn ",
        "def ",
        "class ",
        "import ",
        "const ",
        "let ",
        "var ",
        "public ",
        "private ",
        "return ",
        "=> ",
        " => ",
        "// ",
        "/* ",
        "<?php",
        "#!/",
    ];
    for signal in signals {
        if lowercased.contains(signal) {
            hits += 1;
            if hits >= 2 {
                return true;
            }
        }
    }
    // Fallback: high density of structural punctuation.
    let punct = content
        .chars()
        .filter(|c| matches!(c, '{' | '}' | ';' | '(' | ')' | '[' | ']'))
        .count();
    let chars = content.chars().count().max(1);
    punct as f32 / chars as f32 > 0.04
}

/// Format a chunk's enriched embedding text. Title and tags are
/// prepended (when present) so the embedded vector reflects the
/// memory's semantic context rather than just the raw chunk text.
/// The chunk row's `text` field stays unchanged — only the
/// embedded vector and `content_hash` see the enriched form.
pub fn enriched_embedding_text(
    title: Option<&str>,
    tags: &[String],
    chunk_text: &str,
) -> String {
    let title_clean = title
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let mut prefix_parts: Vec<String> = Vec::new();
    if let Some(t) = title_clean {
        prefix_parts.push(t.to_string());
    }
    let tag_parts: Vec<String> = tags
        .iter()
        .filter(|t| !t.trim().is_empty())
        .cloned()
        .collect();
    if !tag_parts.is_empty() {
        prefix_parts.push(format!("Tags: {}", tag_parts.join(", ")));
    }
    if prefix_parts.is_empty() {
        chunk_text.to_string()
    } else {
        format!("{}\n\n{}", prefix_parts.join("\n"), chunk_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_recall_license_key() {
        let tags = detect_tags("RC-2K4F-9X8M-PLAQ");
        assert!(tags.contains(&"license-key"), "got {:?}", tags);
    }

    #[test]
    fn detects_long_license_key() {
        let tags = detect_tags("RECALL-AAAA-BBBB-CCCC-DDDD");
        assert!(tags.contains(&"license-key"));
    }

    #[test]
    fn does_not_tag_normal_hyphenated_word() {
        let tags = detect_tags("up-to-date documentation");
        assert!(
            !tags.contains(&"license-key"),
            "false positive on hyphenated word: {:?}",
            tags
        );
    }

    #[test]
    fn does_not_tag_uuid_inside_url() {
        // v0.5.6: Amazon product URL with a UUID-shaped path segment.
        // Pre-fix this got tagged license-key because the substring
        // C68F-4742-A7E4 matches the regex (4 hex + 2 dash-blocks).
        let tags = detect_tags(
            "https://www.amazon.in/stores/page/A2B974BA-C68F-4742-A7E4-C68631167C1C?ingress=0",
        );
        assert!(tags.contains(&"url"), "should still tag URL: {:?}", tags);
        assert!(
            !tags.contains(&"license-key"),
            "false positive on UUID inside URL: {:?}",
            tags
        );
    }

    #[test]
    fn does_not_tag_bare_uuid() {
        // UUIDs by themselves (no URL wrapper) are also rejected
        // because they're pure hex — no real letters G-Z. License
        // keys almost always include letters outside the hex range.
        let tags = detect_tags("550E8400-E29B-41D4-A716-446655440000");
        assert!(
            !tags.contains(&"license-key"),
            "false positive on pure-hex UUID: {:?}",
            tags
        );
    }

    #[test]
    fn still_tags_license_key_alongside_url() {
        // A memory containing BOTH a real license key and a URL
        // should still get tagged license-key. The URL filter only
        // suppresses matches that fall WITHIN url substrings, not
        // matches in surrounding prose.
        let tags = detect_tags(
            "Your activation key is RC-TRIAL-5102-65C6 — see https://recall.app/keys",
        );
        assert!(tags.contains(&"license-key"), "missed real key: {:?}", tags);
        assert!(tags.contains(&"url"), "missed URL: {:?}", tags);
    }

    #[test]
    fn detects_url() {
        let tags = detect_tags("Saved this: https://example.com/foo");
        assert!(tags.contains(&"url"));
    }

    #[test]
    fn detects_email() {
        let tags = detect_tags("Reach me at someone@example.com please");
        assert!(tags.contains(&"email"));
    }

    #[test]
    fn detects_phone() {
        let tags = detect_tags("call +1 555-123-4567 today");
        assert!(tags.contains(&"phone-number"), "got {:?}", tags);
    }

    #[test]
    fn detects_ip_address() {
        let tags = detect_tags("server is at 192.168.1.42");
        assert!(tags.contains(&"ip-address"));
    }

    #[test]
    fn detects_code_via_keywords() {
        let tags =
            detect_tags("function greet() { return 42; } const x = greet();");
        assert!(tags.contains(&"code-snippet"));
    }

    #[test]
    fn does_not_tag_pure_prose() {
        let tags = detect_tags(
            "We're meeting on Tuesday to discuss the launch plan and review feedback.",
        );
        assert!(tags.is_empty(), "false positives on prose: {:?}", tags);
    }

    #[test]
    fn detects_long_hex_hash() {
        let tags = detect_tags("commit 1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b");
        assert!(tags.contains(&"hash"));
    }

    #[test]
    fn enriched_text_prepends_title_and_tags() {
        let out = enriched_embedding_text(
            Some("Recall license key"),
            &["license-key".to_string()],
            "RC-2K4F-9X8M-PLAQ",
        );
        assert!(out.starts_with("Recall license key"));
        assert!(out.contains("Tags: license-key"));
        assert!(out.ends_with("RC-2K4F-9X8M-PLAQ"));
    }

    #[test]
    fn enriched_text_with_no_title_or_tags_is_passthrough() {
        let out = enriched_embedding_text(None, &[], "Some content here");
        assert_eq!(out, "Some content here");
    }

    #[test]
    fn enriched_text_skips_blank_title() {
        let out = enriched_embedding_text(Some("   "), &["url".to_string()], "body");
        assert!(out.starts_with("Tags: url"));
        assert!(!out.contains("   "));
    }
}
