use std::{collections::HashMap, sync::Arc, time::Instant};

use chrono::Utc;
use kuchikiki::{parse_html, traits::TendrilSink, NodeRef};
use reqwest::{header, Client};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};
use url::Url;

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::AppResult,
    models::{LinkEnrichmentStatus, LinkEnrichmentUpdate, Memory},
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
        default_headers.insert(
            header::PRAGMA,
            header::HeaderValue::from_static("no-cache"),
        );

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
        let Some(url) = memory.url.as_deref().and_then(normalize_url_candidate) else {
            return;
        };

        if matches!(memory.enrichment_status, Some(LinkEnrichmentStatus::Done)) {
            return;
        }

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
            .filter(|memory| memory.url.is_some())
            .filter(|memory| !matches!(memory.enrichment_status, Some(LinkEnrichmentStatus::Done)))
            .take(STARTUP_RETRY_LIMIT)
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return;
        }

        debug_enrichment_log(format!(
            "startup-resume count={}",
            candidates.len()
        ));

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
        let all_memories = self.repository.list().await.unwrap_or_default();
        self.apply_metadata_to_memory(app, &memory_id, Some(&metadata), &all_memories)
            .await;
    }

    async fn run_enrichment(&self, app: AppHandle, url: String) {
        let permit = match self.concurrency.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return,
        };

        let started_at = Instant::now();
        debug_enrichment_log(format!("started url={url}"));

        let fetched_metadata = match self.fetch_enrichment(&url).await {
            Ok(metadata) => {
                debug_enrichment_log(format!(
                    "success url={} duration_ms={} fields={}",
                    url,
                    started_at.elapsed().as_millis(),
                    summarize_metadata(&metadata),
                ));
                Some(metadata)
            }
            Err(error) => {
                debug_enrichment_log(format!(
                    "failure url={} duration_ms={} error={}",
                    url,
                    started_at.elapsed().as_millis(),
                    error,
                ));
                None
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
        let all_memories = self.repository.list().await.unwrap_or_default();

        for memory_id in waiting_ids {
            self.apply_metadata_to_memory(
                app.clone(),
                &memory_id,
                fetched_metadata.as_ref(),
                &all_memories,
            )
            .await;
        }

        drop(permit);
    }

    async fn apply_metadata_to_memory(
        &self,
        app: AppHandle,
        memory_id: &str,
        metadata: Option<&ExtractedLinkMetadata>,
        all_memories: &[Memory],
    ) {
        let Some(memory) = self.repository.find(memory_id).await.ok().flatten() else {
            return;
        };
        let update = build_link_enrichment_update(&memory, metadata, all_memories);

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
    ]
    .into_iter()
    .filter(|field| !field.is_empty())
    .collect::<Vec<_>>()
    .join(",")
}

fn collapse_whitespace(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
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
            if let Some(content) = attribute_value(element, "content").and_then(|value| collapse_whitespace(&value)) {
                return Some(content);
            }
        }
    }
    None
}

fn extract_title(document: &NodeRef) -> Option<String> {
    document
        .select_first("title")
        .ok()
        .and_then(|node| collapse_whitespace(&node.text_contents()))
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

fn extract_metadata_from_html(url: &str, html: &str) -> Option<ExtractedLinkMetadata> {
    let document = parse_html().one(html).document_node;
    let canonical_url =
        extract_canonical_url(&document, url).or_else(|| normalize_canonical_url(url));

    let resolved_title = extract_meta_content(&document, "property", "og:title")
        .or_else(|| extract_meta_content(&document, "name", "twitter:title"))
        .or_else(|| extract_title(&document));
    let resolved_description = extract_meta_content(&document, "property", "og:description")
        .or_else(|| extract_meta_content(&document, "name", "description"))
        .or_else(|| extract_meta_content(&document, "name", "twitter:description"));
    let resolved_image = resolve_url(
        url,
        extract_meta_content(&document, "property", "og:image")
            .or_else(|| extract_meta_content(&document, "name", "twitter:image")),
    );
    let resolved_site_name = extract_meta_content(&document, "property", "og:site_name")
        .or_else(|| extract_meta_content(&document, "name", "application-name"));

    let metadata = ExtractedLinkMetadata {
        canonical_url,
        resolved_title,
        resolved_description,
        resolved_image,
        resolved_site_name,
    };

    if metadata.resolved_title.is_none()
        && metadata.resolved_description.is_none()
        && metadata.resolved_image.is_none()
        && metadata.resolved_site_name.is_none()
    {
        None
    } else {
        Some(metadata)
    }
}

fn build_link_enrichment_update(
    memory: &Memory,
    metadata: Option<&ExtractedLinkMetadata>,
    all_memories: &[Memory],
) -> LinkEnrichmentUpdate {
    let canonical_url = metadata
        .and_then(|metadata| metadata.canonical_url.as_deref())
        .and_then(normalize_canonical_url)
        .or_else(|| memory.canonical_url.as_deref().and_then(normalize_canonical_url))
        .or_else(|| memory.url.as_deref().and_then(normalize_canonical_url));
    let effective_url = canonical_url
        .clone()
        .or_else(|| memory.url.clone())
        .unwrap_or_else(|| memory.content.clone());
    let resolved_domain = canonical_url
        .as_deref()
        .and_then(extract_domain)
        .or_else(|| memory.url.as_deref().and_then(extract_domain))
        .or_else(|| memory.domain.clone());

    let intelligence = derive_bookmark_intelligence(
        memory,
        &BookmarkMetadataContext {
            url: effective_url.clone(),
            canonical_url: canonical_url.clone(),
            resolved_title: metadata
                .and_then(|metadata| metadata.resolved_title.clone())
                .or_else(|| memory.resolved_title.clone()),
            resolved_description: metadata
                .and_then(|metadata| metadata.resolved_description.clone())
                .or_else(|| memory.resolved_description.clone()),
            resolved_image: metadata
                .and_then(|metadata| metadata.resolved_image.clone())
                .or_else(|| memory.resolved_image.clone()),
            resolved_site_name: metadata
                .and_then(|metadata| metadata.resolved_site_name.clone())
                .or_else(|| memory.resolved_site_name.clone()),
        },
        all_memories,
    );
    let timestamp = Some(Utc::now().to_rfc3339());

    LinkEnrichmentUpdate {
        url: effective_url,
        domain: resolved_domain.clone(),
        resolved_domain: intelligence.resolved_domain.or(resolved_domain),
        canonical_url,
        resolved_title: metadata
            .and_then(|metadata| metadata.resolved_title.clone())
            .or_else(|| memory.resolved_title.clone()),
        resolved_description: metadata
            .and_then(|metadata| metadata.resolved_description.clone())
            .or_else(|| memory.resolved_description.clone()),
        resolved_image: metadata
            .and_then(|metadata| metadata.resolved_image.clone())
            .or_else(|| memory.resolved_image.clone()),
        resolved_site_name: metadata
            .and_then(|metadata| metadata.resolved_site_name.clone())
            .or_else(|| memory.resolved_site_name.clone()),
        topic_labels: Some(intelligence.topic_labels),
        bookmark_quality_score: Some(intelligence.bookmark_quality_score),
        is_duplicate_of: intelligence.is_duplicate_of,
        bookmark_folder_path: intelligence.bookmark_folder_path,
        enrichment_status: if metadata.is_some() {
            LinkEnrichmentStatus::Done
        } else {
            LinkEnrichmentStatus::Failed
        },
        enriched_at: timestamp.clone(),
        last_enriched_at: timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::extract_metadata_from_html;

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

        let metadata =
            extract_metadata_from_html("https://platform.openai.com/docs/pricing", html)
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
}
