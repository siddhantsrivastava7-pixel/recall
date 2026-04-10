use regex::Regex;
use std::sync::OnceLock;
use url::Url;

fn url_regex() -> &'static Regex {
    static URL_REGEX: OnceLock<Regex> = OnceLock::new();
    URL_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)\bhttps?://[^\s<>"'()]+[^\s<>"'().,!?;:]"#)
            .expect("valid url regex")
    })
}

fn trim_url_wrappers(value: &str) -> String {
    value.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

pub fn normalize_url_candidate(value: &str) -> Option<String> {
    let trimmed = trim_url_wrappers(value);
    if trimmed.is_empty() {
        return None;
    }

    let mut parsed = Url::parse(&trimmed).ok()?;
    parsed.set_fragment(None);

    let normalized_host = parsed.host_str().map(|host| host.to_ascii_lowercase())?;
    parsed.set_host(Some(&normalized_host)).ok()?;

    if parsed.path().is_empty() {
        parsed.set_path("/");
    }

    let mut normalized = parsed.to_string();
    if normalized.ends_with('/') && parsed.path() == "/" && parsed.query().is_none() {
        normalized.pop();
    }

    Some(normalized)
}

pub fn detect_primary_url(content: &str, explicit_url: Option<&str>) -> Option<String> {
    if let Some(url) = explicit_url.and_then(normalize_url_candidate) {
        return Some(url);
    }

    let trimmed = content.trim();
    if let Some(url) = normalize_url_candidate(trimmed) {
        return Some(url);
    }

    url_regex()
        .find(trimmed)
        .and_then(|match_| normalize_url_candidate(match_.as_str()))
}

pub fn extract_domain(url: &str) -> Option<String> {
    let normalized = normalize_url_candidate(url)?;
    Url::parse(&normalized)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.trim_start_matches("www.").to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::{detect_primary_url, extract_domain, normalize_url_candidate};

    #[test]
    fn normalizes_url_candidates() {
        let normalized = normalize_url_candidate(" HTTPS://Example.com/Docs/Guide?x=1#top ");
        assert_eq!(
            normalized.as_deref(),
            Some("https://example.com/Docs/Guide?x=1"),
        );
    }

    #[test]
    fn detects_primary_url_inside_text() {
        let detected = detect_primary_url(
            "Keep this handy https://docs.tauri.app/plugin/global-shortcut for later",
            None,
        );

        assert_eq!(
            detected.as_deref(),
            Some("https://docs.tauri.app/plugin/global-shortcut"),
        );
    }

    #[test]
    fn extracts_clean_domain() {
        assert_eq!(
            extract_domain("https://www.nytimes.com/2026/04/09/world"),
            Some("nytimes.com".into()),
        );
    }
}
