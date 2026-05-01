use std::{collections::HashMap, sync::Arc, time::Instant};

use chrono::Utc;
use kuchikiki::{parse_html, traits::TendrilSink, NodeRef};
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};
use url::Url;

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::AppResult,
    models::{LinkEnrichmentStatus, LinkEnrichmentUpdate, Memory, MemorySourceType, MemoryType},
    services::{
        bookmark_intelligence_service::{
            derive_bookmark_intelligence, normalize_canonical_url, BookmarkMetadataContext,
        },
        link_utils::{extract_domain, normalize_url_candidate},
    },
};

const MAX_CONCURRENT_ENRICHMENTS: usize = 3;
const ENRICHMENT_TIMEOUT_SECONDS: u64 = 4;
const STARTUP_RETRY_LIMIT: usize = 24;
const MAX_EXTRACTED_TEXT_CHARS: usize = 18_000;

#[derive(Clone, Debug)]
struct CachedEnrichment {
    metadata: ExtractedLinkMetadata,
}

#[derive(Clone, Debug, Default)]
struct ExtractedLinkMetadata {
    canonical_url: Option<String>,
    resolved_title: Option<String>,
    resolved_description: Option<String>,
    resolved_image: Option<String>,
    resolved_site_name: Option<String>,
    extracted_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XEmbedResponse {
    author_name: Option<String>,
    author_url: Option<String>,
    html: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RedditEmbedResponse {
    title: Option<String>,
    author_name: Option<String>,
    html: Option<String>,
    provider_name: Option<String>,
    thumbnail_url: Option<String>,
}

#[derive(Clone, Debug)]
enum EnrichmentOutcome {
    Link {
        metadata: Option<ExtractedLinkMetadata>,
        error: Option<String>,
    },
    Text,
}

#[derive(Clone)]
pub struct LinkEnrichmentService {
    repository: SharedMemoryRepository,
    client: Client,
    inflight_urls: Arc<Mutex<HashMap<String, Vec<String>>>>,
    cache: Arc<Mutex<HashMap<String, CachedEnrichment>>>,
    concurrency: Arc<Semaphore>,
}

impl LinkEnrichmentService {
    pub fn new(repository: SharedMemoryRepository) -> AppResult<Self> {
        let mut default_headers = header::HeaderMap::new();
        default_headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        default_headers.insert(
            header::ACCEPT_LANGUAGE,
            header::HeaderValue::from_static("en-US,en;q=0.9"),
        );
        default_headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache"),
        );
        default_headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(ENRICHMENT_TIMEOUT_SECONDS))
            .redirect(reqwest::redirect::Policy::limited(5))
            .default_headers(default_headers)
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36 Recall/0.1",
            )
            .build()?;

        Ok(Self {
            repository,
            client,
            inflight_urls: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            concurrency: Arc::new(Semaphore::new(MAX_CONCURRENT_ENRICHMENTS)),
        })
    }

    pub async fn schedule_for_memory(&self, app: AppHandle, memory: Memory) {
        if matches!(memory.enrichment_status, Some(LinkEnrichmentStatus::Done))
            && !should_retry_enrichment(&memory)
        {
            return;
        }

        let Some(url) = memory.url.as_deref().and_then(normalize_url_candidate) else {
            let service = self.clone();
            let memory_id = memory.id.clone();
            tauri::async_runtime::spawn(async move {
                service
                    .apply_enrichment_to_memory(app, &memory_id, EnrichmentOutcome::Text)
                    .await;
            });
            return;
        };

        let needs_fresh_fetch = is_x_or_twitter_url(&url) || is_reddit_url(&url);
        if !needs_fresh_fetch {
            if let Some(cached) = self.cache.lock().await.get(&url).cloned() {
                let service = self.clone();
                let memory_id = memory.id.clone();
                tauri::async_runtime::spawn(async move {
                    service
                        .apply_cached_metadata(app, memory_id, cached.metadata)
                        .await;
                });
                return;
            }
        }

        if needs_fresh_fetch {
            self.cache.lock().await.remove(&url);
        }

        let mut inflight = self.inflight_urls.lock().await;
        if let Some(waiting_ids) = inflight.get_mut(&url) {
            if !waiting_ids.iter().any(|memory_id| memory_id == &memory.id) {
                waiting_ids.push(memory.id.clone());
            }
            return;
        }
        inflight.insert(url.clone(), vec![memory.id.clone()]);
        drop(inflight);

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            service.run_enrichment(app, url).await;
        });
    }

    pub async fn resume_incomplete_enrichments(&self, app: AppHandle, memories: Vec<Memory>) {
        let candidates = memories
            .into_iter()
            .filter(|memory| {
                !matches!(memory.enrichment_status, Some(LinkEnrichmentStatus::Done))
                    || should_retry_enrichment(memory)
            })
            .take(STARTUP_RETRY_LIMIT)
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return;
        }

        debug_enrichment_log(format!("startup-resume count={}", candidates.len()));

        for memory in candidates {
            self.schedule_for_memory(app.clone(), memory).await;
        }
    }

    async fn apply_cached_metadata(
        &self,
        app: AppHandle,
        memory_id: String,
        metadata: ExtractedLinkMetadata,
    ) {
        self.apply_enrichment_to_memory(
            app,
            &memory_id,
            EnrichmentOutcome::Link {
                metadata: Some(metadata),
                error: None,
            },
        )
        .await;
    }

    async fn run_enrichment(&self, app: AppHandle, url: String) {
        let permit = match self.concurrency.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return,
        };

        let started_at = Instant::now();
        debug_enrichment_log(format!("started url={url}"));

        let (fetched_metadata, fetch_error) = match self.fetch_enrichment(&url).await {
            Ok(metadata) => {
                debug_enrichment_log(format!(
                    "success url={} duration_ms={} fields={}",
                    url,
                    started_at.elapsed().as_millis(),
                    summarize_metadata(&metadata),
                ));
                (Some(metadata), None)
            }
            Err(error) => {
                let error_message = error.to_string();
                debug_enrichment_log(format!(
                    "failure url={} duration_ms={} error={}",
                    url,
                    started_at.elapsed().as_millis(),
                    error_message,
                ));
                (None, Some(error_message))
            }
        };

        if let Some(metadata) = fetched_metadata.clone() {
            let mut cache = self.cache.lock().await;
            cache.insert(url.clone(), CachedEnrichment { metadata });
        }

        let waiting_ids = {
            let mut inflight = self.inflight_urls.lock().await;
            inflight.remove(&url).unwrap_or_default()
        };

        for memory_id in waiting_ids {
            self.apply_enrichment_to_memory(
                app.clone(),
                &memory_id,
                EnrichmentOutcome::Link {
                    metadata: fetched_metadata.clone(),
                    error: fetch_error.clone(),
                },
            )
            .await;
        }

        drop(permit);
    }

    async fn apply_enrichment_to_memory(
        &self,
        app: AppHandle,
        memory_id: &str,
        outcome: EnrichmentOutcome,
    ) {
        let Some(memory) = self.repository.find(memory_id).await.ok().flatten() else {
            return;
        };
        let all_memories = self.repository.list().await.unwrap_or_default();
        let update = build_link_enrichment_update(&memory, outcome, &all_memories);

        match self
            .repository
            .update_link_enrichment(memory_id, update)
            .await
        {
            Ok(Some(updated_memory)) => {
                let _ = app.emit("recall://memory-saved", &updated_memory);
            }
            Ok(None) => {}
            Err(error) => {
                debug_enrichment_log(format!(
                    "apply-failure memory_id={} error={}",
                    memory_id, error
                ));
            }
        }
    }

    async fn fetch_enrichment(&self, url: &str) -> AppResult<ExtractedLinkMetadata> {
        if is_x_or_twitter_url(url) {
            return match self.fetch_x_embed_metadata(url).await {
                Ok(metadata) => Ok(metadata),
                Err(embed_error) => {
                    debug_enrichment_log(format!(
                        "x-fallback url={} embed_error={}",
                        url, embed_error
                    ));
                    build_x_fallback_metadata(url).ok_or_else(|| {
                        crate::errors::app_error::AppError::Invalid(
                            "No usable X metadata found.".into(),
                        )
                    })
                }
            };
        }

        if is_reddit_url(url) {
            match self.fetch_reddit_embed_metadata(url).await {
                Ok(metadata) => return Ok(metadata),
                Err(embed_error) => {
                    debug_enrichment_log(format!(
                        "reddit-oembed-fallback url={} embed_error={}",
                        url, embed_error
                    ));
                }
            }

            match self.fetch_html_metadata(url).await {
                Ok(metadata) if !is_low_signal_reddit_extracted_metadata(&metadata) => {
                    return Ok(metadata);
                }
                Ok(_) => {
                    debug_enrichment_log(format!(
                        "reddit-html-shell-detected url={} using_url_fallback=true",
                        url
                    ));
                }
                Err(html_error) => {
                    debug_enrichment_log(format!(
                        "reddit-html-fallback url={} html_error={}",
                        url, html_error
                    ));
                }
            }

            return build_reddit_fallback_metadata(url).ok_or_else(|| {
                crate::errors::app_error::AppError::Invalid(
                    "No usable Reddit metadata found.".into(),
                )
            });
        }

        match self.fetch_html_metadata(url).await {
            Ok(metadata) => Ok(metadata),
            Err(primary_error) => Err(primary_error),
        }
    }

    async fn fetch_html_metadata(&self, url: &str) -> AppResult<ExtractedLinkMetadata> {
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(crate::errors::app_error::AppError::Invalid(format!(
                "Request failed with status {status}",
            )));
        }

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if !content_type.is_empty()
            && !content_type.contains("text/html")
            && !content_type.contains("application/xhtml+xml")
        {
            return Err(crate::errors::app_error::AppError::Invalid(format!(
                "Unsupported content type {content_type}",
            )));
        }

        let html = response.text().await?;
        extract_metadata_from_html(url, &html).ok_or_else(|| {
            crate::errors::app_error::AppError::Invalid(
                "No usable metadata found for URL enrichment.".into(),
            )
        })
    }

    async fn fetch_x_embed_metadata(&self, url: &str) -> AppResult<ExtractedLinkMetadata> {
        let endpoint = Url::parse_with_params(
            "https://publish.twitter.com/oembed",
            &[("url", url), ("omit_script", "true"), ("dnt", "true")],
        )?;
        let response = self
            .client
            .get(endpoint)
            .header(header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(crate::errors::app_error::AppError::Invalid(format!(
                "X oEmbed failed with status {status}",
            )));
        }

        let embed = response.json::<XEmbedResponse>().await?;
        build_x_metadata(url, embed).ok_or_else(|| {
            crate::errors::app_error::AppError::Invalid(
                "No usable X oEmbed metadata found.".into(),
            )
        })
    }

    async fn fetch_reddit_embed_metadata(&self, url: &str) -> AppResult<ExtractedLinkMetadata> {
        let endpoint = Url::parse_with_params("https://www.reddit.com/oembed", &[("url", url)])?;
        let response = self
            .client
            .get(endpoint)
            .header(header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(crate::errors::app_error::AppError::Invalid(format!(
                "Reddit oEmbed failed with status {status}",
            )));
        }

        let embed = response.json::<RedditEmbedResponse>().await?;
        build_reddit_metadata(url, embed).ok_or_else(|| {
            crate::errors::app_error::AppError::Invalid(
                "No usable Reddit oEmbed metadata found.".into(),
            )
        })
    }
}

fn debug_enrichment_log(message: String) {
    if cfg!(debug_assertions) {
        eprintln!("[recall][enrichment] {message}");
    }
}

fn summarize_metadata(metadata: &ExtractedLinkMetadata) -> String {
    [
        metadata
            .canonical_url
            .as_ref()
            .map(|_| "canonical")
            .unwrap_or_default(),
        metadata
            .resolved_title
            .as_ref()
            .map(|_| "title")
            .unwrap_or_default(),
        metadata
            .resolved_description
            .as_ref()
            .map(|_| "description")
            .unwrap_or_default(),
        metadata
            .resolved_image
            .as_ref()
            .map(|_| "image")
            .unwrap_or_default(),
        metadata
            .resolved_site_name
            .as_ref()
            .map(|_| "site_name")
            .unwrap_or_default(),
        metadata
            .extracted_text
            .as_ref()
            .map(|_| "extracted_text")
            .unwrap_or_default(),
    ]
    .into_iter()
    .filter(|field| !field.is_empty())
    .collect::<Vec<_>>()
    .join(",")
}

fn collapse_whitespace(value: &str) -> Option<String> {
    let collapsed = decode_basic_entities(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn decode_basic_entities(value: &str) -> String {
    value
        .replace('\u{00a0}', " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&mdash;", "-")
        .replace("&ndash;", "-")
        .replace("&hellip;", "...")
}

fn resolve_url(base_url: &str, value: Option<String>) -> Option<String> {
    let value = value.and_then(|value| collapse_whitespace(&value))?;
    if let Some(normalized) = normalize_url_candidate(&value) {
        return Some(normalized);
    }

    let base = Url::parse(base_url).ok()?;
    let joined = base.join(&value).ok()?;
    normalize_url_candidate(joined.as_str())
}

fn parsed_url(url: &str) -> Option<Url> {
    normalize_url_candidate(url).and_then(|normalized| Url::parse(&normalized).ok())
}

fn is_x_or_twitter_url(url: &str) -> bool {
    parsed_url(url)
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| {
            matches!(
                host.trim_start_matches("www."),
                "x.com" | "twitter.com" | "mobile.twitter.com"
            )
        })
}

fn is_reddit_url(url: &str) -> bool {
    parsed_url(url)
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| {
            let host = host.trim_start_matches("www.");
            host == "redd.it" || host == "reddit.com" || host.ends_with(".reddit.com")
        })
}

fn looks_like_shell_noise(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let lowered = value.to_ascii_lowercase();
    lowered.contains("<style")
        || lowered.contains("</style")
        || lowered.contains("font-family")
        || lowered.contains("background-color")
        || lowered.contains("overflow-style")
        || lowered.contains("errorcontainer")
        || lowered.contains("__next_data__")
        || lowered.contains("window.__")
        || (lowered.matches('{').count() >= 2 && lowered.matches('}').count() >= 2)
}

fn looks_like_reddit_verification_text(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let lowered = value.trim().to_ascii_lowercase();
    lowered.contains("please wait")
        && lowered.contains("verification")
        || lowered.contains("reddit - please wait")
        || lowered.contains("checking your browser")
        || lowered.contains("blocked by network security")
}

fn is_low_signal_x_metadata(memory: &Memory) -> bool {
    let title = memory
        .resolved_title
        .as_deref()
        .or(memory.title.as_deref())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let domain = memory
        .resolved_domain
        .as_deref()
        .or(memory.domain.as_deref())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    title.is_empty()
        || matches!(title.as_str(), "x.com" | "twitter.com" | "mobile.twitter.com")
        || matches!(domain.as_str(), "x.com" | "twitter.com" | "mobile.twitter.com")
            && (memory.resolved_title.is_none()
                || looks_like_shell_noise(memory.resolved_description.as_deref())
                || looks_like_shell_noise(memory.preview_text.as_deref()))
}

fn is_low_signal_reddit_metadata(memory: &Memory) -> bool {
    looks_like_reddit_verification_text(memory.title.as_deref())
        || looks_like_reddit_verification_text(memory.resolved_title.as_deref())
        || looks_like_reddit_verification_text(memory.resolved_description.as_deref())
        || looks_like_reddit_verification_text(memory.preview_text.as_deref())
        || looks_like_shell_noise(memory.resolved_description.as_deref())
        || looks_like_shell_noise(memory.preview_text.as_deref())
}

fn is_low_signal_reddit_extracted_metadata(metadata: &ExtractedLinkMetadata) -> bool {
    looks_like_reddit_verification_text(metadata.resolved_title.as_deref())
        || looks_like_reddit_verification_text(metadata.resolved_description.as_deref())
        || looks_like_shell_noise(metadata.resolved_description.as_deref())
}

fn should_retry_enrichment(memory: &Memory) -> bool {
    let url_candidate = memory
        .url
        .clone()
        .or_else(|| normalize_url_candidate(&memory.content));
    let is_x = url_candidate.as_deref().is_some_and(is_x_or_twitter_url);
    let is_reddit = url_candidate.as_deref().is_some_and(is_reddit_url);
    let is_done = matches!(memory.enrichment_status, Some(LinkEnrichmentStatus::Done));
    let extracted_text_missing = memory
        .extracted_text
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty);
    let link_should_receive_readable_text = url_candidate.is_some()
        && is_done
        && extracted_text_missing
        && (is_x
            || is_reddit
            || matches!(
                memory.memory_type,
                Some(MemoryType::Article | MemoryType::Bookmark | MemoryType::Post)
            ));

    (is_x && is_low_signal_x_metadata(memory))
        || (is_reddit && is_low_signal_reddit_metadata(memory))
        || link_should_receive_readable_text
}

fn extract_x_handle(url: &str) -> Option<String> {
    let parsed = parsed_url(url)?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
    if !matches!(host.as_str(), "x.com" | "twitter.com" | "mobile.twitter.com") {
        return None;
    }

    parsed
        .path_segments()?
        .find(|segment| {
            !segment.is_empty()
                && !matches!(
                    segment.to_ascii_lowercase().as_str(),
                    "i" | "status" | "statuses" | "intent" | "share"
                )
        })
        .map(|segment| segment.trim_start_matches('@').to_string())
        .filter(|handle| !handle.is_empty())
}

fn clean_embed_text(value: &str) -> Option<String> {
    clean_display_text(value).map(|text| {
        text.replace("pic.twitter.com", " pic.twitter.com")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn extract_text_from_html_fragment(html: &str) -> Option<String> {
    let document = parse_html().one(html).document_node;
    clean_embed_text(&document.text_contents())
}

fn x_author_label(url: &str, author_name: Option<&str>, author_url: Option<&str>) -> String {
    let author = author_name.and_then(clean_embed_text);
    let handle = author_url
        .and_then(extract_x_handle)
        .or_else(|| extract_x_handle(url));

    match (author, handle) {
        (Some(author), Some(handle)) if !author.eq_ignore_ascii_case(&handle) => {
            format!("{author} (@{handle})")
        }
        (Some(author), _) => author,
        (_, Some(handle)) => format!("@{handle}"),
        _ => "X".into(),
    }
}

fn build_x_metadata(url: &str, embed: XEmbedResponse) -> Option<ExtractedLinkMetadata> {
    if !is_x_or_twitter_url(url) {
        return None;
    }

    let author_label = x_author_label(
        url,
        embed.author_name.as_deref(),
        embed.author_url.as_deref(),
    );
    let extracted_text = embed
        .html
        .as_deref()
        .and_then(extract_text_from_html_fragment)
        .filter(|text| text.len() >= 8);
    let description = extracted_text
        .as_deref()
        .and_then(clean_preview_candidate)
        .or_else(|| extracted_text.clone());
    let canonical_url = normalize_canonical_url(url).or_else(|| normalize_url_candidate(url));

    Some(ExtractedLinkMetadata {
        canonical_url,
        resolved_title: Some(format!("X post by {author_label}")),
        resolved_description: description.or_else(|| {
            Some(format!(
                "Saved X post by {author_label}. Open the source to read the full post."
            ))
        }),
        resolved_image: None,
        resolved_site_name: Some("X".into()),
        extracted_text,
    })
}

fn build_x_fallback_metadata(url: &str) -> Option<ExtractedLinkMetadata> {
    build_x_metadata(
        url,
        XEmbedResponse {
            author_name: None,
            author_url: None,
            html: None,
        },
    )
}

#[derive(Debug)]
struct RedditUrlParts {
    subreddit: Option<String>,
    slug: Option<String>,
}

fn prettify_url_slug(value: &str) -> Option<String> {
    let cleaned = value
        .trim_matches('/')
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(
            cleaned
                .split(' ')
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(first) => {
                            format!("{}{}", first.to_uppercase(), chars.as_str())
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

fn reddit_url_parts(url: &str) -> Option<RedditUrlParts> {
    let parsed = parsed_url(url)?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
    if host == "redd.it" {
        return Some(RedditUrlParts {
            subreddit: None,
            slug: parsed
                .path_segments()
                .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
                .and_then(prettify_url_slug),
        });
    }

    if host != "reddit.com" && !host.ends_with(".reddit.com") {
        return None;
    }

    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let subreddit = segments
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("r"))
        .map(|window| window[1].to_string());
    let slug = segments
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("comments"))
        .and_then(|_| {
            let comment_index = segments
                .iter()
                .position(|segment| segment.eq_ignore_ascii_case("comments"))?;
            segments.get(comment_index + 2).copied()
        })
        .and_then(prettify_url_slug);

    Some(RedditUrlParts { subreddit, slug })
}

fn reddit_description(url: &str, author_name: Option<&str>) -> String {
    let parts = reddit_url_parts(url);
    let subreddit = parts
        .as_ref()
        .and_then(|parts| parts.subreddit.as_deref())
        .map(|subreddit| format!("r/{subreddit}"));
    let author = author_name.and_then(clean_display_text);

    match (subreddit, author) {
        (Some(subreddit), Some(author)) => {
            format!("Saved Reddit post from {subreddit} by {author}. Open the source to read the full discussion.")
        }
        (Some(subreddit), None) => {
            format!("Saved Reddit post from {subreddit}. Open the source to read the full discussion.")
        }
        (None, Some(author)) => {
            format!("Saved Reddit post by {author}. Open the source to read the full discussion.")
        }
        (None, None) => "Saved Reddit post. Open the source to read the full discussion.".into(),
    }
}

fn build_reddit_fallback_metadata(url: &str) -> Option<ExtractedLinkMetadata> {
    if !is_reddit_url(url) {
        return None;
    }

    let parts = reddit_url_parts(url);
    let title = parts
        .as_ref()
        .and_then(|parts| parts.slug.clone())
        .map(|slug| format!("Reddit - {slug}"))
        .or_else(|| {
            parts
                .as_ref()
                .and_then(|parts| parts.subreddit.as_ref())
                .map(|subreddit| format!("Reddit post in r/{subreddit}"))
        })
        .unwrap_or_else(|| "Reddit post".into());

    Some(ExtractedLinkMetadata {
        canonical_url: normalize_canonical_url(url).or_else(|| normalize_url_candidate(url)),
        resolved_title: Some(title),
        resolved_description: Some(reddit_description(url, None)),
        resolved_image: None,
        resolved_site_name: Some("Reddit".into()),
        extracted_text: None,
    })
}

fn build_reddit_metadata(url: &str, embed: RedditEmbedResponse) -> Option<ExtractedLinkMetadata> {
    if !is_reddit_url(url) {
        return None;
    }

    let fallback = build_reddit_fallback_metadata(url)?;
    let title = embed
        .title
        .as_deref()
        .and_then(clean_display_text)
        .filter(|title| !looks_like_reddit_verification_text(Some(title)))
        .or(fallback.resolved_title);
    let extracted_text = embed
        .html
        .as_deref()
        .and_then(extract_text_from_html_fragment)
        .filter(|text| text.len() >= 24)
        .filter(|text| !looks_like_reddit_verification_text(Some(text)));
    let description = extracted_text
        .as_deref()
        .and_then(clean_preview_candidate)
        .or_else(|| extracted_text.clone())
        .or_else(|| Some(reddit_description(url, embed.author_name.as_deref())));
    let resolved_image = embed
        .thumbnail_url
        .and_then(|thumbnail| {
            let lowered = thumbnail.to_ascii_lowercase();
            if lowered == "self" || lowered == "default" || lowered == "nsfw" {
                None
            } else {
                resolve_url(url, Some(thumbnail))
            }
        });
    let resolved_site_name = embed
        .provider_name
        .and_then(|provider| clean_display_text(&provider))
        .or(Some("Reddit".into()));

    Some(ExtractedLinkMetadata {
        canonical_url: fallback.canonical_url,
        resolved_title: title,
        resolved_description: description,
        resolved_image,
        resolved_site_name,
        extracted_text,
    })
}

fn attribute_value(node: &NodeRef, key: &str) -> Option<String> {
    let element = node.as_element()?;
    let attributes = element.attributes.borrow();
    attributes.get(key).map(|value| value.to_string())
}

fn extract_meta_content(document: &NodeRef, attribute: &str, target: &str) -> Option<String> {
    let selector = document.select("meta").ok()?;
    for node in selector {
        let element = node.as_node();
        let matches_target = attribute_value(element, attribute)
            .map(|value| value.eq_ignore_ascii_case(target))
            .unwrap_or(false);
        if matches_target {
            if let Some(content) =
                attribute_value(element, "content").and_then(|value| collapse_whitespace(&value))
            {
                return Some(content);
            }
        }
    }
    None
}

fn extract_first_selector_text(document: &NodeRef, selector: &str) -> Option<String> {
    document
        .select_first(selector)
        .ok()
        .and_then(|node| collapse_whitespace(&node.text_contents()))
}

fn extract_title(document: &NodeRef) -> Option<String> {
    extract_first_selector_text(document, "title")
}

fn clean_title(value: &str) -> Option<String> {
    let cleaned = clean_display_text(value)?
        .split(" | ")
        .next()
        .unwrap_or_default()
        .split(" · ")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();

    if cleaned.len() >= 3 && !looks_like_shell_noise(Some(&cleaned)) {
        Some(truncate_chars(&cleaned, 120))
    } else {
        None
    }
}

fn extract_heading_title(document: &NodeRef) -> Option<String> {
    extract_first_selector_text(document, "h1")
        .or_else(|| extract_first_selector_text(document, "article h1"))
        .or_else(|| extract_first_selector_text(document, "[role='main'] h1"))
}

fn extract_canonical_url(document: &NodeRef, url: &str) -> Option<String> {
    if let Ok(selector) = document.select("link") {
        for node in selector {
            let element = node.as_node();
            let rel = attribute_value(element, "rel")
                .unwrap_or_default()
                .to_ascii_lowercase();
            if rel.split_whitespace().any(|value| value == "canonical") {
                let href = attribute_value(element, "href");
                if let Some(resolved) = resolve_url(url, href) {
                    return Some(resolved);
                }
            }
        }
    }

    resolve_url(
        url,
        extract_meta_content(document, "property", "og:url")
            .or_else(|| extract_meta_content(document, "name", "twitter:url")),
    )
}

fn json_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(collapse_whitespace)
    })
}

fn json_image_field(value: &Value) -> Option<String> {
    let image = value.get("image")?;
    if let Some(image) = image.as_str() {
        return collapse_whitespace(image);
    }
    if let Some(array) = image.as_array() {
        return array.iter().find_map(|item| {
            item.as_str()
                .and_then(collapse_whitespace)
                .or_else(|| json_string_field(item, &["url", "contentUrl"]))
        });
    }
    json_string_field(image, &["url", "contentUrl"])
}

fn merge_json_ld_metadata(
    metadata: &mut ExtractedLinkMetadata,
    value: &Value,
    base_url: &str,
) {
    if metadata.resolved_title.is_none() {
        metadata.resolved_title =
            json_string_field(value, &["headline", "name", "title", "alternativeHeadline"]);
    }
    if metadata.resolved_description.is_none() {
        metadata.resolved_description =
            json_string_field(value, &["description", "abstract", "summary"]);
    }
    if metadata.resolved_image.is_none() {
        metadata.resolved_image = resolve_url(base_url, json_image_field(value));
    }
    if metadata.resolved_site_name.is_none() {
        metadata.resolved_site_name = value
            .get("publisher")
            .and_then(|publisher| {
                publisher
                    .as_str()
                    .and_then(collapse_whitespace)
                    .or_else(|| json_string_field(publisher, &["name"]))
            })
            .or_else(|| {
                value
                    .get("sourceOrganization")
                    .and_then(|organization| json_string_field(organization, &["name"]))
            });
    }

    if let Some(graph) = value.get("@graph").and_then(Value::as_array) {
        for item in graph {
            merge_json_ld_metadata(metadata, item, base_url);
        }
    }
    if let Some(array) = value.as_array() {
        for item in array {
            merge_json_ld_metadata(metadata, item, base_url);
        }
    }
}

fn extract_json_ld_metadata(document: &NodeRef, base_url: &str) -> ExtractedLinkMetadata {
    let mut metadata = ExtractedLinkMetadata::default();
    let Ok(selector) = document.select("script[type='application/ld+json']") else {
        return metadata;
    };

    for node in selector {
        let raw_json = node.text_contents();
        let Ok(value) = serde_json::from_str::<Value>(raw_json.trim()) else {
            continue;
        };
        merge_json_ld_metadata(&mut metadata, &value, base_url);
    }

    metadata
}

fn score_text_candidate(text: &str) -> i32 {
    let word_count = text.split_whitespace().count() as i32;
    let punctuation_count = text
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?' | ';' | ':'))
        .count() as i32;
    let paragraph_like = text.matches(". ").count() as i32;

    word_count + punctuation_count * 3 + paragraph_like * 8
}

fn is_boilerplate_line(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.is_empty()
        || matches!(
            lowered.as_str(),
            "menu"
                | "navigation"
                | "skip to content"
                | "share"
                | "advertisement"
                | "privacy policy"
                | "terms of service"
                | "sign in"
                | "sign up"
                | "log in"
                | "login"
                | "subscribe"
                | "accept"
                | "accept all"
                | "manage cookies"
        )
        || lowered.contains("enable javascript")
        || lowered.contains("cookie")
        || lowered.contains("sign up to")
        || lowered.contains("log in to")
        || lowered.contains("subscribe to")
        || lowered.contains("accept cookies")
        || lowered.contains("we use cookies")
        || lowered.contains("all rights reserved")
        || lowered.contains("please wait")
        || lowered.contains("checking your browser")
        || looks_like_shell_noise(Some(value))
}

fn smart_trim_sentence(value: &str, max_chars: usize) -> String {
    let cleaned = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let first_boundary = cleaned.char_indices().find_map(|(index, character)| {
        if matches!(character, '.' | '!' | '?') {
            Some(index + character.len_utf8())
        } else {
            None
        }
    });
    if let Some(index) = first_boundary.filter(|index| *index >= 48 && *index <= max_chars) {
        return cleaned[..index].trim().to_string();
    }

    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }

    let mut boundary = None;
    for (index, character) in cleaned.char_indices() {
        if index > max_chars {
            break;
        }
        if matches!(character, '.' | '!' | '?') {
            boundary = Some(index + character.len_utf8());
        }
    }

    if let Some(index) = boundary.filter(|index| *index >= 80) {
        return cleaned[..index].trim().to_string();
    }

    let mut end = 0;
    for (index, character) in cleaned.char_indices() {
        if index > max_chars {
            break;
        }
        if character.is_whitespace() {
            end = index;
        }
    }

    let end = if end >= 80 { end } else { max_chars.min(cleaned.len()) };
    format!("{}...", cleaned[..end].trim_end())
}

fn clean_preview_candidate(value: &str) -> Option<String> {
    let without_html = decode_basic_entities(value);
    let cleaned = without_html
        .lines()
        .map(str::trim)
        .filter(|line| !is_boilerplate_line(line))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let word_count = cleaned.split_whitespace().count();
    if word_count >= 4 && cleaned.len() >= 24 && !looks_like_shell_noise(Some(&cleaned)) {
        Some(smart_trim_sentence(&cleaned, 190))
    } else {
        None
    }
}

fn clean_extracted_text_candidate(value: &str) -> Option<String> {
    let decoded = decode_basic_entities(value);
    let mut lines = Vec::new();
    let mut previous = String::new();

    for raw_line in decoded.lines() {
        let line = raw_line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        if line.len() < 2
            || is_boilerplate_line(&line)
            || looks_like_shell_noise(Some(&line))
            || line.eq_ignore_ascii_case(&previous)
        {
            continue;
        }

        previous = line.clone();
        lines.push(line);
    }

    let cleaned = lines.join("\n\n");
    let word_count = cleaned.split_whitespace().count();
    if word_count < 8 || cleaned.len() < 50 || looks_like_shell_noise(Some(&cleaned)) {
        return None;
    }

    if cleaned.chars().count() <= MAX_EXTRACTED_TEXT_CHARS {
        return Some(cleaned);
    }

    let mut end = 0;
    for (index, character) in cleaned.char_indices() {
        if index > MAX_EXTRACTED_TEXT_CHARS {
            break;
        }
        if matches!(character, '.' | '!' | '?' | '\n') {
            end = index + character.len_utf8();
        }
    }

    let end = if end >= 800 {
        end
    } else {
        cleaned
            .char_indices()
            .take_while(|(index, _)| *index <= MAX_EXTRACTED_TEXT_CHARS)
            .last()
            .map(|(index, character)| index + character.len_utf8())
            .unwrap_or(cleaned.len())
    };

    Some(format!("{}...", cleaned[..end].trim_end()))
}

fn remove_non_content_nodes(document: &NodeRef) {
    let selectors = [
        "script",
        "style",
        "noscript",
        "svg",
        "canvas",
        "nav",
        "header",
        "footer",
        "form",
        "aside",
        "[role='navigation']",
        "[aria-label='breadcrumb']",
        ".cookie",
        ".cookies",
        ".cookie-banner",
        ".consent",
        ".modal",
        ".newsletter",
        ".signup",
        ".login",
        ".advertisement",
        ".ads",
        "#cookie-banner",
    ];

    for selector in selectors {
        let Ok(nodes) = document.select(selector) else {
            continue;
        };
        let nodes = nodes
            .map(|node| node.as_node().clone())
            .collect::<Vec<_>>();
        for node in nodes {
            node.detach();
        }
    }
}

fn extract_readable_node_text(node: &NodeRef) -> Option<String> {
    let mut parts = Vec::new();
    if let Ok(nodes) = node.select("h1,h2,h3,p,li,blockquote,pre,code") {
        for child in nodes {
            let text = child
                .as_node()
                .text_contents()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.len() >= 2 && !is_boilerplate_line(&text) {
                parts.push(text);
            }
        }
    }

    if !parts.is_empty() {
        return clean_extracted_text_candidate(&parts.join("\n\n"));
    }

    clean_extracted_text_candidate(&node.text_contents())
}

fn extract_article_text(document: &NodeRef) -> Option<String> {
    let selectors = [
        "article",
        "main",
        "[role='main']",
        ".article",
        ".post",
        ".entry-content",
        ".content",
        "#content",
    ];

    let mut best: Option<(i32, String)> = None;
    for selector in selectors {
        let Ok(nodes) = document.select(selector) else {
            continue;
        };
        for node in nodes {
            let Some(text) = extract_readable_node_text(node.as_node()) else {
                continue;
            };
            let score = score_text_candidate(&text);
            if best.as_ref().is_none_or(|(best_score, _)| score > *best_score) {
                best = Some((score, text));
            }
        }
    }

    best.map(|(_, text)| text).or_else(|| {
        let body_text = extract_first_selector_text(document, "body")?;
        clean_extracted_text_candidate(&body_text)
    })
}

fn build_github_fallback_metadata(url: &str) -> Option<ExtractedLinkMetadata> {
    let parsed = parsed_url(url)?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
    if host != "github.com" {
        return None;
    }

    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .take(4)
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }

    let repo = format!("{}/{}", segments[0], segments[1]);
    let title = if segments.len() == 2 {
        format!("GitHub - {repo}")
    } else {
        format!("GitHub - {} · {}", repo, segments[2..].join(" / "))
    };

    Some(ExtractedLinkMetadata {
        canonical_url: normalize_canonical_url(url).or_else(|| normalize_url_candidate(url)),
        resolved_title: Some(title),
        resolved_description: Some(format!("Saved GitHub page for {repo}.")),
        resolved_image: None,
        resolved_site_name: Some("GitHub".into()),
        extracted_text: None,
    })
}

fn build_url_fallback_title(url: &str) -> Option<String> {
    let parsed = parsed_url(url)?;
    let domain = parsed.host_str()?.trim_start_matches("www.");
    let slug = parsed
        .path_segments()
        .and_then(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .last()
                .and_then(prettify_url_slug)
        })
        .filter(|slug| slug.len() >= 4);

    Some(match slug {
        Some(slug) => format!("{domain} - {slug}"),
        None => domain.to_string(),
    })
}

fn extract_metadata_from_html(url: &str, html: &str) -> Option<ExtractedLinkMetadata> {
    let document = parse_html().one(html).document_node;
    let canonical_url =
        extract_canonical_url(&document, url).or_else(|| normalize_canonical_url(url));
    let json_ld = extract_json_ld_metadata(&document, url);
    remove_non_content_nodes(&document);
    let extracted_text = extract_article_text(&document);

    let title_signal = extract_meta_content(&document, "property", "og:title")
        .or_else(|| extract_meta_content(&document, "name", "twitter:title"))
        .or_else(|| extract_meta_content(&document, "itemprop", "headline"))
        .or_else(|| extract_meta_content(&document, "itemprop", "name"))
        .or(json_ld.resolved_title)
        .or_else(|| extract_heading_title(&document))
        .or_else(|| extract_title(&document))
        .and_then(|title| clean_title(&title));
    let resolved_title = title_signal.clone()
        .or_else(|| build_github_fallback_metadata(url).and_then(|metadata| metadata.resolved_title))
        .or_else(|| build_url_fallback_title(url));
    let resolved_description = extract_meta_content(&document, "property", "og:description")
        .or_else(|| extract_meta_content(&document, "name", "description"))
        .or_else(|| extract_meta_content(&document, "name", "twitter:description"))
        .or_else(|| extract_meta_content(&document, "itemprop", "description"))
        .or(json_ld.resolved_description)
        .and_then(|description| clean_preview_candidate(&description))
        .or_else(|| extracted_text.as_deref().and_then(clean_preview_candidate))
        .or_else(|| {
            extract_first_selector_text(&document, "body")
                .and_then(|text| clean_preview_candidate(&text))
        });
    let resolved_image = resolve_url(
        url,
        extract_meta_content(&document, "property", "og:image")
            .or_else(|| extract_meta_content(&document, "name", "twitter:image")),
    )
    .or(json_ld.resolved_image);
    let resolved_site_name = extract_meta_content(&document, "property", "og:site_name")
        .or_else(|| extract_meta_content(&document, "name", "application-name"))
        .or_else(|| extract_meta_content(&document, "name", "twitter:site"))
        .or(json_ld.resolved_site_name);

    let metadata = ExtractedLinkMetadata {
        canonical_url,
        resolved_title,
        resolved_description,
        resolved_image,
        resolved_site_name,
        extracted_text,
    };

    if title_signal.is_none()
        && metadata.resolved_description.is_none()
        && metadata.resolved_image.is_none()
        && metadata.resolved_site_name.is_none()
    {
        build_github_fallback_metadata(url)
    } else {
        Some(metadata)
    }
}

fn build_link_enrichment_update(
    memory: &Memory,
    outcome: EnrichmentOutcome,
    all_memories: &[Memory],
) -> LinkEnrichmentUpdate {
    let (metadata, fetch_error, is_text_only) = match outcome {
        EnrichmentOutcome::Link { metadata, error } => (metadata, error, false),
        EnrichmentOutcome::Text => (None, None, true),
    };
    let metadata_ref = metadata.as_ref();
    let canonical_url = metadata_ref
        .and_then(|metadata| metadata.canonical_url.as_deref())
        .and_then(normalize_canonical_url)
        .or_else(|| {
            memory
                .canonical_url
                .as_deref()
                .and_then(normalize_canonical_url)
        })
        .or_else(|| memory.url.as_deref().and_then(normalize_canonical_url));
    let effective_url = canonical_url
        .clone()
        .or_else(|| memory.url.clone())
        .or_else(|| normalize_url_candidate(&memory.content));
    let resolved_domain = canonical_url
        .as_deref()
        .and_then(extract_domain)
        .or_else(|| memory.url.as_deref().and_then(extract_domain))
        .or_else(|| memory.domain.clone());
    let resolved_title = metadata_ref
        .and_then(|metadata| metadata.resolved_title.clone())
        .or_else(|| memory.resolved_title.clone());
    let resolved_description = metadata_ref
        .and_then(|metadata| metadata.resolved_description.clone())
        .or_else(|| memory.resolved_description.clone());
    let extracted_text = metadata_ref
        .and_then(|metadata| metadata.extracted_text.clone())
        .or_else(|| memory.extracted_text.clone());
    let resolved_image = metadata_ref
        .and_then(|metadata| metadata.resolved_image.clone())
        .or_else(|| memory.resolved_image.clone());
    let resolved_site_name = metadata_ref
        .and_then(|metadata| metadata.resolved_site_name.clone())
        .or_else(|| memory.resolved_site_name.clone());

    let intelligence = derive_bookmark_intelligence(
        memory,
        &BookmarkMetadataContext {
            url: effective_url.clone().unwrap_or_else(|| memory.content.clone()),
            canonical_url: canonical_url.clone(),
            resolved_title: resolved_title.clone(),
            resolved_description: resolved_description.clone(),
            resolved_image: resolved_image.clone(),
            resolved_site_name: resolved_site_name.clone(),
        },
        all_memories,
    );
    let canonical_url = intelligence.canonical_url.or(canonical_url);
    let resolved_domain = intelligence.resolved_domain.or(resolved_domain);
    let preview_text = build_preview_text(
        memory,
        resolved_description.as_deref(),
        extracted_text.as_deref(),
    );
    let summary_text = build_summary_text(
        memory,
        resolved_title.as_deref(),
        resolved_description.as_deref(),
        extracted_text.as_deref(),
        preview_text.as_deref(),
        resolved_domain.as_deref(),
    );
    let memory_type = classify_memory_type(
        memory,
        resolved_title.as_deref(),
        resolved_description.as_deref(),
        resolved_site_name.as_deref(),
        effective_url.as_deref().or(memory.url.as_deref()),
    );
    let quality_score = score_memory_quality(
        memory,
        resolved_title.as_deref(),
        resolved_description.as_deref(),
        extracted_text.as_deref(),
        preview_text.as_deref(),
        resolved_domain.as_deref(),
        resolved_image.as_deref(),
        intelligence.is_duplicate_of.is_some(),
        metadata.is_some() || is_text_only,
    );
    let primary_topic = intelligence.topic_labels.first().cloned();
    let timestamp = Some(Utc::now().to_rfc3339());

    LinkEnrichmentUpdate {
        url: effective_url,
        domain: resolved_domain.clone(),
        resolved_domain,
        canonical_url,
        resolved_title,
        resolved_description,
        resolved_image,
        resolved_site_name,
        preview_text,
        summary_text,
        extracted_text,
        memory_type: Some(memory_type),
        topic_labels: Some(intelligence.topic_labels),
        primary_topic,
        quality_score: Some(quality_score),
        bookmark_quality_score: Some(intelligence.bookmark_quality_score),
        is_duplicate_of: intelligence.is_duplicate_of,
        bookmark_folder_path: intelligence.bookmark_folder_path,
        enrichment_status: if metadata.is_some() || is_text_only {
            LinkEnrichmentStatus::Done
        } else {
            LinkEnrichmentStatus::Failed
        },
        enrichment_error: fetch_error,
        enriched_at: timestamp.clone(),
        last_enriched_at: timestamp,
    }
}

fn strip_code_fence_markers(value: &str) -> &str {
    value
        .trim()
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

fn clean_display_text(value: &str) -> Option<String> {
    let collapsed = strip_code_fence_markers(value)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn build_preview_text(
    memory: &Memory,
    resolved_description: Option<&str>,
    extracted_text: Option<&str>,
) -> Option<String> {
    resolved_description
        .and_then(clean_preview_candidate)
        .or_else(|| extracted_text.and_then(clean_preview_candidate))
        .or_else(|| memory.note.as_deref().and_then(clean_display_text))
        .or_else(|| clean_display_text(&memory.content))
        .map(|value| smart_trim_sentence(&value, 190))
}

fn looks_like_url_or_domain(value: &str) -> bool {
    let trimmed = value.trim().trim_start_matches("www.").to_ascii_lowercase();
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || (trimmed.contains('.') && !trimmed.contains(' '))
}

fn build_summary_text(
    memory: &Memory,
    resolved_title: Option<&str>,
    resolved_description: Option<&str>,
    extracted_text: Option<&str>,
    preview_text: Option<&str>,
    resolved_domain: Option<&str>,
) -> Option<String> {
    let content = clean_display_text(&memory.content);
    let content_is_url = content
        .as_deref()
        .is_some_and(looks_like_url_or_domain);

    let candidates = [
        resolved_description,
        extracted_text,
        preview_text,
        memory.note.as_deref(),
        if !content_is_url {
            content.as_deref()
        } else {
            None
        },
        resolved_title.filter(|value| !looks_like_url_or_domain(value)),
        memory
            .title
            .as_deref()
            .filter(|value| !looks_like_url_or_domain(value)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Some(cleaned) = clean_preview_candidate(candidate).or_else(|| clean_display_text(candidate)) {
            return Some(smart_trim_sentence(&cleaned, 220));
        }
    }

    resolved_domain
        .or(memory.resolved_domain.as_deref())
        .or(memory.domain.as_deref())
        .map(|domain| format!("Saved link from {domain}. Open the source to view the saved page."))
}

fn classify_memory_type(
    memory: &Memory,
    resolved_title: Option<&str>,
    resolved_description: Option<&str>,
    resolved_site_name: Option<&str>,
    effective_url: Option<&str>,
) -> MemoryType {
    if memory.source_type == MemorySourceType::Bookmark {
        return MemoryType::Bookmark;
    }

    let haystack = [
        memory.title.as_deref(),
        resolved_title,
        resolved_description,
        resolved_site_name,
        memory.note.as_deref(),
        Some(memory.content.as_str()),
        effective_url,
        memory.folder_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    let domain = effective_url.and_then(extract_domain).unwrap_or_default();
    if looks_like_code(&memory.content) {
        return MemoryType::CodeSnippet;
    }
    if domain.contains("youtube.com")
        || domain.contains("youtu.be")
        || haystack.contains(" watch ")
        || haystack.contains(" video ")
    {
        return MemoryType::Video;
    }
    if domain.contains("x.com")
        || domain.contains("twitter.com")
        || domain.contains("reddit.com")
        || domain.contains("linkedin.com")
        || domain.contains("threads.net")
    {
        return MemoryType::Post;
    }
    if haystack.contains("/docs")
        || haystack.contains(" docs ")
        || haystack.contains(" documentation")
        || domain.contains("docs.")
        || domain.contains("developer.")
    {
        return MemoryType::Docs;
    }
    if domain.contains("github.com")
        || domain.contains("npmjs.com")
        || domain.contains("figma.com")
        || haystack.contains(" tool ")
        || haystack.contains(" dashboard ")
    {
        return MemoryType::Tool;
    }
    if effective_url.is_some() {
        return MemoryType::Article;
    }

    MemoryType::Note
}

fn looks_like_code(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    content.contains("```")
        || lowered.contains("function ")
        || lowered.contains("const ")
        || lowered.contains("let ")
        || lowered.contains("import ")
        || lowered.contains("class ")
        || lowered.contains("fn ")
        || lowered.contains("select ")
        || lowered.contains("<div")
        || lowered.contains("</")
        || (content.lines().count() >= 3 && content.contains('{') && content.contains('}'))
}

fn score_memory_quality(
    memory: &Memory,
    resolved_title: Option<&str>,
    resolved_description: Option<&str>,
    extracted_text: Option<&str>,
    preview_text: Option<&str>,
    resolved_domain: Option<&str>,
    resolved_image: Option<&str>,
    is_duplicate: bool,
    enrichment_succeeded: bool,
) -> f64 {
    let mut score: f64 = 18.0;

    if memory
        .title
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.len() >= 8)
        || resolved_title.is_some_and(|value| value.trim().len() >= 8)
    {
        score += 18.0;
    }
    if resolved_description.is_some_and(|value| value.trim().len() >= 32) {
        score += 18.0;
    }
    if extracted_text.is_some_and(|value| value.split_whitespace().count() >= 16) {
        score += 12.0;
    }
    if preview_text.is_some_and(|value| value.trim().len() >= 40) {
        score += 12.0;
    }
    if resolved_domain.is_some() {
        score += 10.0;
    }
    if resolved_image.is_some() {
        score += 6.0;
    }
    if memory
        .note
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.len() >= 12)
    {
        score += 8.0;
    }
    if memory
        .folder_path
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
        || memory
            .bookmark_folder_path
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    {
        score += 5.0;
    }
    if enrichment_succeeded {
        score += 4.0;
    }
    if looks_like_code(&memory.content) {
        score += 4.0;
    }
    if is_duplicate {
        score -= 24.0;
    }

    score.clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::Utc;
    use sqlx::types::Json;

    use crate::{
        db::repositories::MemoryRepository,
        errors::app_error::{AppError, AppResult},
    };
    use crate::models::{
        LinkEnrichmentStatus, LinkEnrichmentUpdate, Memory, MemoryInput, MemorySourceType,
        MemoryType,
    };

    use super::{
        build_link_enrichment_update, build_reddit_fallback_metadata, build_reddit_metadata,
        build_x_fallback_metadata, build_x_metadata, extract_metadata_from_html,
        parsed_url, should_retry_enrichment, summarize_metadata, EnrichmentOutcome,
        ExtractedLinkMetadata, LinkEnrichmentService, RedditEmbedResponse, XEmbedResponse,
    };

    struct NoopMemoryRepository;

    #[async_trait]
    impl MemoryRepository for NoopMemoryRepository {
        async fn list(&self) -> AppResult<Vec<Memory>> {
            Ok(vec![])
        }

        async fn find(&self, _id: &str) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn find_by_external_source(
            &self,
            _source_app: &str,
            _external_id: &str,
        ) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn create(&self, _input: MemoryInput) -> AppResult<Memory> {
            Err(AppError::Invalid("noop repository".into()))
        }

        async fn update(&self, _id: &str, _input: MemoryInput) -> AppResult<Memory> {
            Err(AppError::Invalid("noop repository".into()))
        }

        async fn update_link_enrichment(
            &self,
            _id: &str,
            _enrichment: LinkEnrichmentUpdate,
        ) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn set_resurface(
            &self,
            _id: &str,
            _resurface_at: Option<String>,
            _updated_at: &str,
        ) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn dismiss_resurface(
            &self,
            _id: &str,
            _dismissed_at: &str,
            _updated_at: &str,
        ) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn mark_opened(&self, _id: &str, _opened_at: &str) -> AppResult<Option<Memory>> {
            Ok(None)
        }

        async fn set_ocr_status(
            &self,
            _id: &str,
            _status: &str,
            _text: Option<&str>,
            _engine: Option<&str>,
            _processed_at: Option<&str>,
        ) -> AppResult<()> {
            Ok(())
        }

        async fn promote_ocr_to_content(
            &self,
            _id: &str,
            _ocr_text: &str,
            _derived_title: &str,
        ) -> AppResult<bool> {
            Ok(false)
        }

        async fn clear_url_for_purged_screenshots(
            &self,
            _purged_paths: &[String],
        ) -> AppResult<u64> {
            Ok(0)
        }

        async fn list_chunks_for_memory(
            &self,
            _memory_id: &str,
        ) -> AppResult<Vec<crate::models::MemoryChunkRow>> {
            Ok(Vec::new())
        }

        async fn replace_chunks_hash_aware(
            &self,
            _memory_id: &str,
            _chunks: &[crate::db::repositories::ChunkUpsert<'_>],
            _active_embedding_model: Option<&str>,
        ) -> AppResult<Vec<String>> {
            Ok(Vec::new())
        }

        async fn set_chunk_embedding(
            &self,
            _chunk_id: &str,
            _model: &str,
            _dim: u32,
            _vector_bytes: &[u8],
            _generated_at: &str,
        ) -> AppResult<()> {
            Ok(())
        }

        async fn list_embedded_chunks(&self) -> AppResult<Vec<crate::models::MemoryChunkRow>> {
            Ok(Vec::new())
        }

        async fn list_embedded_chunks_for_model(
            &self,
            _model_id: &str,
        ) -> AppResult<Vec<crate::models::MemoryChunkRow>> {
            Ok(Vec::new())
        }

        async fn count_embedded_chunks_for_model(&self, _model_id: &str) -> AppResult<u64> {
            Ok(0)
        }

        async fn merge_topic_labels(
            &self,
            _memory_id: &str,
            _additional_tags: &[&str],
        ) -> AppResult<Vec<String>> {
            Ok(Vec::new())
        }

        async fn topic_labels_for_memory(&self, _memory_id: &str) -> AppResult<Vec<String>> {
            Ok(Vec::new())
        }

        async fn embedding_coverage(
            &self,
        ) -> AppResult<crate::db::repositories::EmbeddingCoverage> {
            Ok(Default::default())
        }

        async fn delete(&self, _id: &str) -> AppResult<()> {
            Ok(())
        }

        async fn clear(&self) -> AppResult<()> {
            Ok(())
        }
    }

    fn test_memory(content: &str) -> Memory {
        Memory {
            id: "memory-1".into(),
            source_type: MemorySourceType::Manual,
            title: None,
            content: content.into(),
            note: None,
            project_id: None,
            project_name: None,
            url: None,
            domain: None,
            resolved_domain: None,
            canonical_url: None,
            resolved_title: None,
            resolved_description: None,
            resolved_image: None,
            resolved_site_name: None,
            preview_text: None,
            summary_text: None,
            extracted_text: None,
            memory_type: None,
            topic_labels: Some(Json(vec![])),
            primary_topic: None,
            quality_score: Some(0.0),
            bookmark_quality_score: Some(0.0),
            is_duplicate_of: None,
            bookmark_folder_path: None,
            enrichment_status: Some(LinkEnrichmentStatus::Pending),
            enrichment_error: None,
            enriched_at: None,
            last_enriched_at: None,
            external_id: None,
            folder_path: None,
            source_app: None,
            source_window: None,
            resurface_at: None,
            resurface_dismissed_at: None,
            last_opened_at: None,
            open_count: 0,
            ocr_text: None,
            ocr_status: None,
            ocr_processed_at: None,
            ocr_engine: None,
            ocr_error: None,
            embedding_model_version: None,
            embedding_generated_at: None,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn extracts_og_metadata_and_fallbacks() {
        let html = r#"
          <html>
            <head>
              <title>Fallback title</title>
              <meta property="og:title" content="OpenAI pricing">
              <meta property="og:description" content="See the latest model pricing.">
              <meta property="og:image" content="/images/pricing.png">
              <meta property="og:site_name" content="OpenAI Docs">
            </head>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://platform.openai.com/docs/pricing", html)
            .expect("metadata should be extracted");

        assert_eq!(metadata.resolved_title.as_deref(), Some("OpenAI pricing"));
        assert_eq!(
            metadata.resolved_description.as_deref(),
            Some("See the latest model pricing."),
        );
        assert_eq!(
            metadata.resolved_image.as_deref(),
            Some("https://platform.openai.com/images/pricing.png"),
        );
        assert_eq!(metadata.resolved_site_name.as_deref(), Some("OpenAI Docs"));
    }

    #[test]
    fn extracts_title_and_description_fallbacks() {
        let html = r#"
          <html>
            <head>
              <title>Example article</title>
              <meta name="description" content="A concise summary for search and cards.">
            </head>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://example.com/article", html)
            .expect("fallback metadata should be extracted");

        assert_eq!(metadata.resolved_title.as_deref(), Some("Example article"));
        assert_eq!(
            metadata.resolved_description.as_deref(),
            Some("A concise summary for search and cards."),
        );
        assert!(metadata.resolved_image.is_none());
    }

    #[test]
    fn cleans_preview_boilerplate_and_trims_to_sentence() {
        let html = r#"
          <html>
            <head>
              <title>Pricing playbook | Example</title>
            </head>
            <body>
              <nav>Menu Navigation Sign in</nav>
              <div class="cookie">We use cookies to improve your experience. Accept all</div>
              <main>
                <p>Revenue teams can use usage-based pricing to align customer value with expansion. This guide explains when to introduce metered packaging and how to avoid surprise bills.</p>
                <p>Sign up to receive updates.</p>
              </main>
            </body>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://example.com/guides/pricing-playbook", html)
            .expect("clean metadata");

        assert_eq!(metadata.resolved_title.as_deref(), Some("Pricing playbook"));
        assert_eq!(
            metadata.resolved_description.as_deref(),
            Some("Revenue teams can use usage-based pricing to align customer value with expansion."),
        );
    }

    #[test]
    fn generates_title_from_url_when_page_title_is_missing() {
        let html = r#"
          <html>
            <body>
              <main>
                <p>This article explains how local first software keeps important user data available, private, and reliable without requiring cloud sync.</p>
              </main>
            </body>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://example.com/posts/local-first-recall", html)
            .expect("metadata with url fallback title");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("example.com - Local First Recall"),
        );
    }

    #[test]
    fn extracts_json_ld_metadata_when_open_graph_is_missing() {
        let html = r#"
          <html>
            <head>
              <script type="application/ld+json">
                {
                  "@type": "Article",
                  "headline": "Local-first memory systems",
                  "description": "A practical guide to reliable local-first capture and recall.",
                  "image": {"url": "/og/local-first.png"},
                  "publisher": {"name": "Recall Research"}
                }
              </script>
            </head>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://example.com/research/local-first", html)
            .expect("json-ld metadata should be extracted");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("Local-first memory systems"),
        );
        assert_eq!(
            metadata.resolved_description.as_deref(),
            Some("A practical guide to reliable local-first capture and recall."),
        );
        assert_eq!(
            metadata.resolved_image.as_deref(),
            Some("https://example.com/og/local-first.png"),
        );
        assert_eq!(
            metadata.resolved_site_name.as_deref(),
            Some("Recall Research"),
        );
    }

    #[test]
    fn extracts_article_text_when_description_is_missing() {
        let html = r#"
          <html>
            <body>
              <article>
                <h1>Durable capture without forms</h1>
                <p>Recall should save useful context quickly and quietly.</p>
                <p>The enrichment layer can then shape raw links into searchable memory objects.</p>
              </article>
            </body>
          </html>
        "#;

        let metadata = extract_metadata_from_html("https://example.com/capture", html)
            .expect("article text should be extracted");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("Durable capture without forms"),
        );
        assert!(metadata
            .resolved_description
            .as_deref()
            .unwrap_or_default()
            .contains("Recall should save useful context"));
        let extracted = metadata.extracted_text.as_deref().unwrap_or_default();
        assert!(extracted.contains("Recall should save useful context"));
        assert!(extracted.contains("shape raw links into searchable memory objects"));
    }

    #[test]
    fn github_fallback_creates_metadata_when_html_has_no_signals() {
        let metadata =
            extract_metadata_from_html("https://github.com/D4Vinci/Scrapling", "<html></html>")
                .expect("github fallback metadata");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("GitHub - D4Vinci/Scrapling"),
        );
        assert_eq!(metadata.resolved_site_name.as_deref(), Some("GitHub"));
    }

    #[test]
    fn text_enrichment_shapes_preview_and_classifies_note() {
        let memory = test_memory("   Capture pipeline idea\r\n\r\n\r\nKeep saves fast and searchable.   ");
        let update = build_link_enrichment_update(&memory, EnrichmentOutcome::Text, &[]);

        assert_eq!(update.enrichment_status, LinkEnrichmentStatus::Done);
        assert_eq!(
            update.preview_text.as_deref(),
            Some("Capture pipeline idea Keep saves fast and searchable."),
        );
        assert_eq!(update.memory_type, Some(MemoryType::Note));
        assert_eq!(update.primary_topic.as_deref(), Some("Capture Pipeline"));
        assert!(update.quality_score.unwrap_or_default() > 20.0);
        assert!(update.enrichment_error.is_none());
    }

    #[test]
    fn code_snippet_enrichment_classifies_code() {
        let memory = test_memory("```ts\nconst saveFast = true;\nfunction capture() { return saveFast; }\n```");
        let update = build_link_enrichment_update(&memory, EnrichmentOutcome::Text, &[]);

        assert_eq!(update.memory_type, Some(MemoryType::CodeSnippet));
        assert!(
            update
                .preview_text
                .as_deref()
                .unwrap_or_default()
                .contains("const saveFast"),
        );
    }

    #[test]
    fn url_enrichment_adds_metadata_preview_and_quality() {
        let mut memory = test_memory("https://docs.tauri.app/plugin/global-shortcut");
        memory.url = Some("https://docs.tauri.app/plugin/global-shortcut".into());
        memory.domain = Some("docs.tauri.app".into());

        let update = build_link_enrichment_update(
            &memory,
            EnrichmentOutcome::Link {
                metadata: Some(ExtractedLinkMetadata {
                    canonical_url: Some("https://docs.tauri.app/plugin/global-shortcut".into()),
                    resolved_title: Some("Global Shortcut Plugin".into()),
                    resolved_description: Some("Register global shortcuts in a Tauri app.".into()),
                    resolved_image: Some("https://docs.tauri.app/og.png".into()),
                    resolved_site_name: Some("Tauri Docs".into()),
                    extracted_text: Some(
                        "Register global shortcuts in a Tauri app without blocking the main UI thread."
                            .into(),
                    ),
                }),
                error: None,
            },
            &[],
        );

        assert_eq!(update.enrichment_status, LinkEnrichmentStatus::Done);
        assert_eq!(update.memory_type, Some(MemoryType::Docs));
        assert_eq!(update.resolved_title.as_deref(), Some("Global Shortcut Plugin"));
        assert_eq!(update.resolved_domain.as_deref(), Some("docs.tauri.app"));
        assert!(update.quality_score.unwrap_or_default() >= 70.0);
    }

    #[test]
    fn x_url_fallback_creates_searchable_metadata() {
        let metadata = build_x_fallback_metadata("https://x.com/VaibhavSainty/status/204846683083919547")
            .expect("x fallback metadata");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("X post by @VaibhavSainty"),
        );
        assert_eq!(metadata.resolved_site_name.as_deref(), Some("X"));
        assert!(metadata
            .resolved_description
            .as_deref()
            .unwrap_or_default()
            .contains("@VaibhavSainty"));
    }

    #[test]
    fn bad_x_shell_memory_is_marked_for_retry() {
        let mut memory = test_memory("https://x.com/example/status/123");
        memory.url = Some("https://x.com/example/status/123".into());
        memory.title = Some("x.com".into());
        memory.domain = Some("x.com".into());
        memory.resolved_domain = Some("x.com".into());
        memory.preview_text =
            Some("<style> body { font-family: Helvetica; background-color: #fff; }".into());
        memory.enrichment_status = Some(LinkEnrichmentStatus::Done);

        assert!(should_retry_enrichment(&memory));
    }

    #[test]
    fn done_link_without_extracted_text_is_retried_for_readable_body() {
        let mut memory = test_memory("https://example.com/article");
        memory.url = Some("https://example.com/article".into());
        memory.domain = Some("example.com".into());
        memory.memory_type = Some(MemoryType::Article);
        memory.enrichment_status = Some(LinkEnrichmentStatus::Done);
        memory.resolved_description = Some("A short existing card preview.".into());
        memory.extracted_text = None;

        assert!(should_retry_enrichment(&memory));
    }

    #[test]
    fn reddit_fallback_creates_searchable_metadata_from_url() {
        let metadata = build_reddit_fallback_metadata(
            "https://www.reddit.com/r/LocalLLaMA/comments/1abc234/best_local_first_memory_app/",
        )
        .expect("reddit fallback metadata");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("Reddit - Best Local First Memory App"),
        );
        assert_eq!(metadata.resolved_site_name.as_deref(), Some("Reddit"));
        assert!(metadata
            .resolved_description
            .as_deref()
            .unwrap_or_default()
            .contains("r/LocalLLaMA"));
    }

    #[test]
    fn reddit_oembed_metadata_rejects_verification_title() {
        let metadata = build_reddit_metadata(
            "https://www.reddit.com/r/rust/comments/1abc234/tauri_app_architecture/",
            RedditEmbedResponse {
                title: Some("Reddit - Please wait for verification".into()),
                author_name: Some("u/recallbuilder".into()),
                html: Some("<blockquote>Discussion about clean Tauri app architecture.</blockquote>".into()),
                provider_name: Some("Reddit".into()),
                thumbnail_url: None,
            },
        )
        .expect("reddit metadata");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("Reddit - Tauri App Architecture"),
        );
        assert!(metadata
            .resolved_description
            .as_deref()
            .unwrap_or_default()
            .contains("Tauri app architecture"));
        assert!(metadata
            .extracted_text
            .as_deref()
            .unwrap_or_default()
            .contains("Tauri app architecture"));
    }

    #[test]
    fn bad_reddit_verification_memory_is_marked_for_retry() {
        let mut memory = test_memory("https://www.reddit.com/r/rust/comments/1abc234/tauri_app_architecture/");
        memory.url =
            Some("https://www.reddit.com/r/rust/comments/1abc234/tauri_app_architecture/".into());
        memory.title = Some("Reddit - Please wait for verification".into());
        memory.domain = Some("reddit.com".into());
        memory.resolved_domain = Some("reddit.com".into());
        memory.enrichment_status = Some(LinkEnrichmentStatus::Done);

        assert!(should_retry_enrichment(&memory));
    }

    #[test]
    fn x_oembed_metadata_uses_embedded_post_text() {
        let metadata = build_x_metadata(
            "https://x.com/RecallHQ/status/123",
            XEmbedResponse {
                author_name: Some("Recall".into()),
                author_url: Some("https://x.com/RecallHQ".into()),
                html: Some("<blockquote>Save fast, find fast. <a>April 17, 2026</a></blockquote>".into()),
            },
        )
        .expect("x embed metadata");

        assert_eq!(
            metadata.resolved_title.as_deref(),
            Some("X post by Recall (@RecallHQ)"),
        );
        assert!(metadata
            .resolved_description
            .as_deref()
            .unwrap_or_default()
            .contains("Save fast"));
        assert!(metadata
            .extracted_text
            .as_deref()
            .unwrap_or_default()
            .contains("Save fast"));
    }

    #[tokio::test]
    #[ignore = "live network smoke check; run manually when validating parser coverage"]
    async fn live_popular_sites_description_smoke_check() {
        let service = LinkEnrichmentService::new(Arc::new(NoopMemoryRepository))
            .expect("service should initialize");
        let urls = [
            "https://github.com/tauri-apps/tauri",
            "https://docs.tauri.app/plugin/global-shortcut/",
            "https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API",
            "https://en.wikipedia.org/wiki/Local-first_software",
            "https://www.npmjs.com/package/react",
            "https://stackoverflow.com/questions/11828270/how-do-i-exit-vim",
            "https://stripe.com/docs",
            "https://vercel.com/blog",
            "https://www.figma.com/blog/",
            "https://www.producthunt.com/",
            "https://news.ycombinator.com/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.reddit.com/r/ClaudeAI/comments/1sg4x27/codex_vs_claude_brutal/",
            "https://x.com/VaibhavSisinty/status/204846683083919547",
            "https://www.linkedin.com/",
        ];

        let mut successes = 0usize;
        let mut description_successes = 0usize;

        println!(
            "\n{:<28} | {:<8} | {:<32} | {}",
            "site", "status", "fields", "description"
        );
        println!("{}", "-".repeat(118));

        for url in urls {
            let site = parsed_url(url)
                .and_then(|parsed| parsed.host_str().map(|host| host.to_string()))
                .unwrap_or_else(|| url.to_string());

            match service.fetch_enrichment(url).await {
                Ok(metadata) => {
                    successes += 1;
                    let description = metadata
                        .resolved_description
                        .as_deref()
                        .unwrap_or_default()
                        .replace('\n', " ");
                    if description.len() >= 24 {
                        description_successes += 1;
                    }

                    println!(
                        "{:<28} | {:<8} | {:<32} | {}",
                        site,
                        "OK",
                        summarize_metadata(&metadata),
                        description.chars().take(110).collect::<String>()
                    );
                }
                Err(error) => {
                    println!(
                        "{:<28} | {:<8} | {:<32} | {}",
                        site,
                        "FAIL",
                        "",
                        error
                    );
                }
            }
        }

        println!(
            "\nSummary: metadata_success={}/{} description_success={}/{}",
            successes,
            urls.len(),
            description_successes,
            urls.len()
        );
    }
}
