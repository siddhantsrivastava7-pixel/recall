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

    if license_key_re().is_match(trimmed) {
        tags.push("license-key");
    }
    if email_re().is_match(trimmed) {
        tags.push("email");
    }
    if url_re().is_match(trimmed) {
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
