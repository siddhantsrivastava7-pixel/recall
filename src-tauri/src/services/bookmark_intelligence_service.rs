use std::collections::{HashMap, HashSet};

use url::Url;

use crate::{
    models::{Memory, MemorySourceType},
    services::link_utils::extract_domain,
};

const MAX_TOPIC_LABELS: usize = 5;

const GENERIC_FOLDER_LABELS: &[&str] = &[
    "bookmark",
    "bookmarks",
    "bookmarks bar",
    "favorites",
    "other bookmarks",
    "mobile bookmarks",
    "synced",
];

const GENERIC_TOPIC_TOKENS: &[&str] = &[
    "and",
    "app",
    "article",
    "bookmark",
    "bookmarks",
    "co",
    "com",
    "dev",
    "for",
    "from",
    "github",
    "guide",
    "how",
    "http",
    "https",
    "info",
    "io",
    "link",
    "net",
    "org",
    "page",
    "post",
    "site",
    "that",
    "the",
    "this",
    "url",
    "use",
    "using",
    "watch",
    "with",
    "www",
    "youtube",
];

const TRACKING_QUERY_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "ref_src",
    "si",
    "spm",
    "utm_campaign",
    "utm_content",
    "utm_id",
    "utm_medium",
    "utm_name",
    "utm_source",
    "utm_term",
];

const LOW_SIGNAL_DOMAINS: &[&str] = &[
    "x.com",
    "twitter.com",
    "t.co",
    "facebook.com",
    "instagram.com",
    "linkedin.com",
];

const HIGH_SIGNAL_DOMAINS: &[&str] = &[
    "github.com",
    "wikipedia.org",
    "stackoverflow.com",
    "stackexchange.com",
    "developer.mozilla.org",
    "docs.rs",
    "arxiv.org",
];

#[derive(Debug, Clone, Default)]
pub struct BookmarkMetadataContext {
    pub url: String,
    pub canonical_url: Option<String>,
    pub resolved_title: Option<String>,
    pub resolved_description: Option<String>,
    pub resolved_image: Option<String>,
    pub resolved_site_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BookmarkIntelligenceOutput {
    pub resolved_domain: Option<String>,
    pub canonical_url: Option<String>,
    pub topic_labels: Vec<String>,
    pub bookmark_quality_score: f64,
    pub is_duplicate_of: Option<String>,
    pub bookmark_folder_path: Option<String>,
}

pub fn normalize_canonical_url(raw_url: &str) -> Option<String> {
    let mut parsed = Url::parse(raw_url).ok()?;

    parsed.set_fragment(None);
    let _ = parsed.set_scheme(&parsed.scheme().to_ascii_lowercase());

    if let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) {
        let normalized_host = host.strip_prefix("www.").unwrap_or(&host).to_string();
        parsed.set_host(Some(&normalized_host)).ok()?;
    }

    if (parsed.scheme() == "https" && parsed.port_or_known_default() == Some(443))
        || (parsed.scheme() == "http" && parsed.port_or_known_default() == Some(80))
    {
        let _ = parsed.set_port(None);
    }

    let retained_query_pairs = parsed
        .query_pairs()
        .filter(|(key, _)| {
            let key = key.to_ascii_lowercase();
            !TRACKING_QUERY_PARAMS.iter().any(|tracked| tracked == &key)
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    parsed.query_pairs_mut().clear();
    if retained_query_pairs.is_empty() {
        parsed.set_query(None);
    } else {
        for (key, value) in retained_query_pairs {
            parsed.query_pairs_mut().append_pair(&key, &value);
        }
    }

    let normalized_path = parsed.path().trim_end_matches('/').to_string();
    if normalized_path.is_empty() {
        parsed.set_path("/");
    } else {
        parsed.set_path(&normalized_path);
    }

    Some(parsed.to_string())
}

pub fn derive_bookmark_intelligence(
    memory: &Memory,
    context: &BookmarkMetadataContext,
    all_memories: &[Memory],
) -> BookmarkIntelligenceOutput {
    let canonical_url = context
        .canonical_url
        .as_deref()
        .and_then(normalize_canonical_url)
        .or_else(|| normalize_canonical_url(&context.url));
    let resolved_domain = canonical_url
        .as_deref()
        .and_then(extract_domain)
        .or_else(|| extract_domain(&context.url))
        .map(clean_domain);
    let bookmark_folder_path = normalize_single_line(
        memory
            .bookmark_folder_path
            .as_deref()
            .or(memory.folder_path.as_deref()),
    );
    let topic_labels = extract_topic_labels(
        memory,
        context,
        resolved_domain.as_deref(),
        canonical_url.as_deref(),
    );
    let is_duplicate_of = find_duplicate_of(
        memory,
        all_memories,
        canonical_url.as_deref(),
        resolved_domain.as_deref(),
        context,
    );
    let bookmark_quality_score = compute_quality_score(
        memory,
        context,
        resolved_domain.as_deref(),
        canonical_url.as_deref(),
        &topic_labels,
        bookmark_folder_path.as_deref(),
        is_duplicate_of.as_deref(),
    );

    BookmarkIntelligenceOutput {
        resolved_domain,
        canonical_url,
        topic_labels,
        bookmark_quality_score,
        is_duplicate_of,
        bookmark_folder_path,
    }
}

fn clean_domain(domain: String) -> String {
    domain
        .trim()
        .to_ascii_lowercase()
        .trim_start_matches("www.")
        .to_string()
}

fn normalize_single_line(value: Option<&str>) -> Option<String> {
    let normalized = value
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 2)
        .filter(|token| !GENERIC_TOPIC_TOKENS.iter().any(|generic| generic == token))
        .collect()
}

fn token_similarity(left: &[String], right: &[String]) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let left_set = left.iter().cloned().collect::<HashSet<_>>();
    let right_set = right.iter().cloned().collect::<HashSet<_>>();
    let intersection = left_set.intersection(&right_set).count() as f64;
    let union = left_set.union(&right_set).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn extract_path_tokens(url: Option<&str>) -> Vec<String> {
    let Some(url) = url else {
        return Vec::new();
    };
    let Ok(parsed) = Url::parse(url) else {
        return Vec::new();
    };
    tokenize(parsed.path())
}

fn score_topic_candidate(scores: &mut HashMap<String, f64>, label: String, score: f64) {
    if label.trim().is_empty() {
        return;
    }
    *scores.entry(label).or_insert(0.0) += score;
}

fn format_topic_label(label: &str) -> String {
    label
        .split_whitespace()
        .map(|segment| match segment {
            "ai" | "api" | "ui" | "ux" | "sql" => segment.to_ascii_uppercase(),
            _ => {
                let mut chars = segment.chars();
                match chars.next() {
                    Some(first) => {
                        first.to_ascii_uppercase().to_string()
                            + &chars.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_topic_labels(
    memory: &Memory,
    context: &BookmarkMetadataContext,
    resolved_domain: Option<&str>,
    canonical_url: Option<&str>,
) -> Vec<String> {
    let mut scores = HashMap::<String, f64>::new();

    let folder_path = normalize_single_line(
        memory
            .bookmark_folder_path
            .as_deref()
            .or(memory.folder_path.as_deref()),
    );
    if let Some(folder_path) = folder_path {
        for segment in folder_path.split('/') {
            let segment = segment.trim();
            let normalized = segment.to_ascii_lowercase();
            if segment.len() >= 3
                && !GENERIC_FOLDER_LABELS
                    .iter()
                    .any(|generic| generic.eq_ignore_ascii_case(&normalized))
            {
                score_topic_candidate(&mut scores, format_topic_label(segment), 8.0);
            }
        }
    }

    if let Some(domain) = resolved_domain {
        for token in tokenize(domain).iter().take(3) {
            score_topic_candidate(&mut scores, format_topic_label(token), 2.4);
        }
    }

    let title_sources = [context.resolved_title.as_deref(), memory.title.as_deref()];
    for source in title_sources.into_iter().flatten() {
        let tokens = tokenize(source);
        for token in tokens.iter().take(4) {
            score_topic_candidate(&mut scores, format_topic_label(token), 3.0);
        }
        for window in tokens.windows(2).take(3) {
            let label = window.join(" ");
            score_topic_candidate(&mut scores, format_topic_label(&label), 5.0);
        }
    }

    if let Some(description) = context.resolved_description.as_deref() {
        let tokens = tokenize(description);
        for window in tokens.windows(2).take(3) {
            let label = window.join(" ");
            score_topic_candidate(&mut scores, format_topic_label(&label), 2.5);
        }
    }

    for source in [Some(memory.content.as_str()), memory.note.as_deref()]
        .into_iter()
        .flatten()
    {
        let tokens = tokenize(source);
        for token in tokens.iter().take(5) {
            score_topic_candidate(&mut scores, format_topic_label(token), 1.2);
        }
        for window in tokens.windows(2).take(3) {
            score_topic_candidate(&mut scores, format_topic_label(&window.join(" ")), 1.8);
        }
    }

    for token in extract_path_tokens(canonical_url.or(Some(&context.url)))
        .iter()
        .take(4)
    {
        if resolved_domain.is_some_and(|domain| token == domain) {
            continue;
        }
        score_topic_candidate(&mut scores, format_topic_label(token), 2.0);
    }

    let mut topic_labels = scores
        .into_iter()
        .collect::<Vec<_>>();

    topic_labels.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.0.len().cmp(&left.0.len()))
            .then_with(|| left.0.cmp(&right.0))
    });

    topic_labels.into_iter().map(|(label, _)| label).fold(
        Vec::<String>::new(),
        |mut output, label| {
            if !output
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&label))
                && output.len() < MAX_TOPIC_LABELS
            {
                output.push(label);
            }
            output
        },
    )
}

fn duplicate_candidate_key(memory: &Memory) -> (&str, &str) {
    (&memory.created_at, &memory.id)
}

fn find_duplicate_of(
    memory: &Memory,
    all_memories: &[Memory],
    canonical_url: Option<&str>,
    resolved_domain: Option<&str>,
    context: &BookmarkMetadataContext,
) -> Option<String> {
    let current_canonical = canonical_url.map(str::to_string);
    let current_title_tokens = tokenize(
        context
            .resolved_title
            .as_deref()
            .or(memory.title.as_deref())
            .unwrap_or_default(),
    );
    let current_path_tokens =
        extract_path_tokens(current_canonical.as_deref().or(Some(&context.url)));

    let mut exact_matches = Vec::new();
    let mut near_matches = Vec::new();

    for candidate in all_memories {
        if candidate.id == memory.id || candidate.url.is_none() {
            continue;
        }

        let candidate_canonical = candidate
            .canonical_url
            .as_deref()
            .and_then(normalize_canonical_url)
            .or_else(|| candidate.url.as_deref().and_then(normalize_canonical_url));

        if current_canonical.is_some()
            && candidate_canonical.is_some()
            && current_canonical == candidate_canonical
        {
            exact_matches.push(candidate);
            continue;
        }

        let candidate_domain = candidate_canonical
            .as_deref()
            .and_then(extract_domain)
            .or_else(|| candidate.url.as_deref().and_then(extract_domain))
            .map(|domain| domain.to_ascii_lowercase())
            .or_else(|| {
                candidate
                    .resolved_domain
                    .as_deref()
                    .or(candidate.domain.as_deref())
                    .map(|domain| domain.to_ascii_lowercase())
            });
        if resolved_domain.map(str::to_ascii_lowercase) != candidate_domain {
            continue;
        }

        let candidate_title_tokens = tokenize(
            candidate
                .resolved_title
                .as_deref()
                .or(candidate.title.as_deref())
                .unwrap_or_default(),
        );
        let title_similarity = token_similarity(&current_title_tokens, &candidate_title_tokens);
        if title_similarity < 0.72 {
            continue;
        }

        let shared_title_tokens = current_title_tokens
            .iter()
            .collect::<HashSet<_>>()
            .intersection(&candidate_title_tokens.iter().collect::<HashSet<_>>())
            .count();

        let candidate_path_tokens =
            extract_path_tokens(candidate_canonical.as_deref().or(candidate.url.as_deref()));
        let path_similarity = token_similarity(&current_path_tokens, &candidate_path_tokens);

        if path_similarity >= 0.55
            || title_similarity >= 0.85
            || (title_similarity >= 0.72 && path_similarity >= 0.25)
            || shared_title_tokens >= 3
        {
            near_matches.push(candidate);
        }
    }

    let choose_primary = |matches: Vec<&Memory>| -> Option<String> {
        let primary = matches.into_iter().min_by(|left, right| {
            duplicate_candidate_key(left).cmp(&duplicate_candidate_key(right))
        })?;
        if duplicate_candidate_key(primary) < duplicate_candidate_key(memory) {
            Some(primary.id.clone())
        } else {
            None
        }
    };

    choose_primary(exact_matches).or_else(|| choose_primary(near_matches))
}

fn domain_quality_score(domain: Option<&str>) -> f64 {
    let Some(domain) = domain else {
        return 0.0;
    };

    if HIGH_SIGNAL_DOMAINS
        .iter()
        .any(|candidate| domain == *candidate || domain.ends_with(&format!(".{candidate}")))
    {
        return 12.0;
    }

    if LOW_SIGNAL_DOMAINS
        .iter()
        .any(|candidate| domain == *candidate || domain.ends_with(&format!(".{candidate}")))
    {
        return -8.0;
    }

    if domain.contains("docs") || domain.contains("developer") || domain.contains("learn") {
        return 8.0;
    }

    if domain.contains("blog") || domain.contains("news") {
        return 4.0;
    }

    1.5
}

fn compute_quality_score(
    memory: &Memory,
    context: &BookmarkMetadataContext,
    resolved_domain: Option<&str>,
    canonical_url: Option<&str>,
    topic_labels: &[String],
    bookmark_folder_path: Option<&str>,
    duplicate_of: Option<&str>,
) -> f64 {
    let mut score = 24.0;

    if memory.source_type == MemorySourceType::Bookmark {
        score += 8.0;
    }

    let resolved_title = context.resolved_title.as_deref().unwrap_or_default();
    let resolved_description = context.resolved_description.as_deref().unwrap_or_default();
    let original_title = memory.title.as_deref().unwrap_or_default();

    if !resolved_title.trim().is_empty() {
        score += 22.0;
    } else if !original_title.trim().is_empty() && !original_title.starts_with("http") {
        score += 8.0;
    }

    if resolved_description.len() >= 120 {
        score += 18.0;
    } else if resolved_description.len() >= 60 {
        score += 12.0;
    } else if resolved_description.len() >= 24 {
        score += 7.0;
    }

    if context
        .resolved_site_name
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        score += 6.0;
    }

    if canonical_url.is_some() {
        score += 8.0;
    }

    if bookmark_folder_path.is_some_and(|value| !value.trim().is_empty()) {
        score += 7.0;
    }

    score += topic_labels.len().min(MAX_TOPIC_LABELS) as f64 * 3.5;
    score += domain_quality_score(resolved_domain);

    if duplicate_of.is_some() {
        score -= 40.0;
    }

    if context
        .resolved_image
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        score += 3.0;
    }

    score.clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use sqlx::types::Json;

    use crate::models::{LinkEnrichmentStatus, MemorySourceType, MemoryType};

    use super::{derive_bookmark_intelligence, normalize_canonical_url, BookmarkMetadataContext};

    fn bookmark_memory(
        id: &str,
        title: &str,
        url: &str,
        created_at: &str,
    ) -> crate::models::Memory {
        crate::models::Memory {
            id: id.into(),
            source_type: MemorySourceType::Bookmark,
            title: Some(title.into()),
            content: url.into(),
            note: None,
            project_id: None,
            project_name: None,
            url: Some(url.into()),
            domain: Some("github.com".into()),
            resolved_domain: Some("github.com".into()),
            canonical_url: Some(url.into()),
            resolved_title: None,
            resolved_description: None,
            resolved_image: None,
            resolved_site_name: None,
            preview_text: None,
            memory_type: Some(MemoryType::Bookmark),
            topic_labels: Some(Json(vec![])),
            primary_topic: None,
            quality_score: Some(0.0),
            bookmark_quality_score: Some(0.0),
            is_duplicate_of: None,
            bookmark_folder_path: Some("Bookmarks Bar / Research".into()),
            enrichment_status: Some(LinkEnrichmentStatus::Pending),
            enrichment_error: None,
            enriched_at: None,
            last_enriched_at: None,
            external_id: None,
            folder_path: Some("Bookmarks Bar / Research".into()),
            source_app: Some("chrome".into()),
            source_window: None,
            last_opened_at: None,
            open_count: 0,
            created_at: created_at.into(),
            updated_at: created_at.into(),
        }
    }

    #[test]
    fn canonical_url_normalization_strips_tracking_and_fragment() {
        let normalized = normalize_canonical_url(
            "https://www.example.com/docs/pricing/?utm_source=test&ref_src=twitter#section",
        )
        .expect("normalized url");

        assert_eq!(normalized, "https://example.com/docs/pricing");
    }

    #[test]
    fn intelligence_derives_topics_and_quality() {
        let memory = bookmark_memory(
            "memory-1",
            "OpenAI pricing guide",
            "https://platform.openai.com/docs/pricing",
            "2026-04-01T09:00:00.000Z",
        );

        let intelligence = derive_bookmark_intelligence(
            &memory,
            &BookmarkMetadataContext {
                url: "https://platform.openai.com/docs/pricing".into(),
                canonical_url: Some("https://platform.openai.com/docs/pricing".into()),
                resolved_title: Some("OpenAI API pricing guide".into()),
                resolved_description: Some(
                    "Review model pricing, token tiers, and usage details for API planning.".into(),
                ),
                resolved_image: Some("https://platform.openai.com/assets/pricing.png".into()),
                resolved_site_name: Some("OpenAI Docs".into()),
            },
            &[],
        );

        assert_eq!(
            intelligence.resolved_domain.as_deref(),
            Some("platform.openai.com")
        );
        assert!(intelligence
            .topic_labels
            .iter()
            .any(|label| label == "Pricing"));
        assert!(intelligence.bookmark_quality_score > 40.0);
    }

    #[test]
    fn intelligence_marks_exact_duplicate_against_older_bookmark() {
        let older = bookmark_memory(
            "older",
            "Tauri docs",
            "https://tauri.app/start/?utm_source=x",
            "2026-03-01T10:00:00.000Z",
        );
        let newer = bookmark_memory(
            "newer",
            "Tauri getting started",
            "https://tauri.app/start/",
            "2026-04-01T10:00:00.000Z",
        );

        let intelligence = derive_bookmark_intelligence(
            &newer,
            &BookmarkMetadataContext {
                url: "https://tauri.app/start/".into(),
                canonical_url: Some("https://tauri.app/start/".into()),
                resolved_title: Some("Tauri getting started".into()),
                resolved_description: None,
                resolved_image: None,
                resolved_site_name: Some("Tauri".into()),
            },
            &[older],
        );

        assert_eq!(intelligence.is_duplicate_of.as_deref(), Some("older"));
        assert!(intelligence.bookmark_quality_score < 60.0);
    }

    #[test]
    fn intelligence_marks_near_duplicate_for_similar_titles_on_same_domain() {
        let existing = bookmark_memory(
            "existing",
            "React performance checklist",
            "https://react.dev/blog/performance-checklist",
            "2026-03-01T10:00:00.000Z",
        );
        let current = bookmark_memory(
            "current",
            "Performance checklist for React apps",
            "https://react.dev/learn/performance",
            "2026-04-01T10:00:00.000Z",
        );

        let intelligence = derive_bookmark_intelligence(
            &current,
            &BookmarkMetadataContext {
                url: "https://react.dev/learn/performance".into(),
                canonical_url: Some("https://react.dev/learn/performance".into()),
                resolved_title: Some("Performance checklist for React apps".into()),
                resolved_description: Some(
                    "Patterns for profiling and optimizing render work.".into(),
                ),
                resolved_image: None,
                resolved_site_name: Some("React".into()),
            },
            &[existing],
        );

        assert_eq!(intelligence.is_duplicate_of.as_deref(), Some("existing"));
    }
}
