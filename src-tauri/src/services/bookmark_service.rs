use std::{collections::HashSet, sync::Arc};

use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use tokio::fs;

use crate::{
    db::repositories::{SharedMemoryRepository, SharedSettingsRepository},
    errors::app_error::{AppError, AppResult},
    models::{BookmarkBrowser, BookmarkImportResult, BookmarkSourceStatus, BookmarkSyncSummary},
    platform::contracts::BrowserPathResolver,
    services::{
        capture_service::{BookmarkCaptureInput, CaptureService},
        link_enrichment_service::LinkEnrichmentService,
    },
};
use tauri::AppHandle;

#[derive(Debug, Deserialize)]
struct BookmarkFile {
    roots: std::collections::HashMap<String, BookmarkNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct BookmarkNode {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    node_type: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    date_added: Option<String>,
    #[serde(default)]
    children: Vec<BookmarkNode>,
}

#[derive(Debug, Clone)]
struct ParsedBookmark {
    external_id: String,
    title: String,
    url: String,
    folder_path: Option<String>,
    created_at: String,
}

pub struct BookmarkIngestionService {
    memory_repository: SharedMemoryRepository,
    capture_service: Arc<CaptureService>,
    settings_repository: SharedSettingsRepository,
    browser_paths: Arc<dyn BrowserPathResolver>,
    link_enrichment_service: Arc<LinkEnrichmentService>,
}

impl BookmarkIngestionService {
    pub fn new(
        memory_repository: SharedMemoryRepository,
        capture_service: Arc<CaptureService>,
        settings_repository: SharedSettingsRepository,
        browser_paths: Arc<dyn BrowserPathResolver>,
        link_enrichment_service: Arc<LinkEnrichmentService>,
    ) -> Self {
        Self {
            memory_repository,
            capture_service,
            settings_repository,
            browser_paths,
            link_enrichment_service,
        }
    }

    pub async fn list_sources(&self) -> AppResult<Vec<BookmarkSourceStatus>> {
        Ok(supported_browsers()
            .into_iter()
            .map(|browser| {
                let path = self.browser_paths.resolve_bookmark_file(browser);
                BookmarkSourceStatus {
                    browser,
                    is_available: path.as_ref().is_some_and(|path| path.exists()),
                    path: path.map(|path| path.display().to_string()),
                }
            })
            .collect())
    }

    pub async fn import_browsers(
        &self,
        app: AppHandle,
        browsers: Vec<BookmarkBrowser>,
    ) -> AppResult<BookmarkSyncSummary> {
        self.import_internal(app, deduplicate_browsers(browsers), true)
            .await
    }

    pub async fn sync_selected_browsers(&self, app: AppHandle) -> AppResult<BookmarkSyncSummary> {
        let settings = self.settings_repository.get().await?;
        self.import_internal(
            app,
            deduplicate_browsers(settings.bookmark_sync_browsers),
            true,
        )
        .await
    }

    async fn import_internal(
        &self,
        app: AppHandle,
        browsers: Vec<BookmarkBrowser>,
        update_last_synced_at: bool,
    ) -> AppResult<BookmarkSyncSummary> {
        let mut results = Vec::new();

        for browser in browsers {
            let result = match self.import_single_browser(app.clone(), browser).await {
                Ok(result) => result,
                Err(error) => BookmarkImportResult {
                    browser,
                    path: self
                        .browser_paths
                        .resolve_bookmark_file(browser)
                        .map(|path| path.display().to_string()),
                    imported_count: 0,
                    skipped_count: 0,
                    message: error.to_string(),
                },
            };
            results.push(result);
        }

        let total_imported = results.iter().map(|result| result.imported_count).sum();
        let total_skipped = results.iter().map(|result| result.skipped_count).sum();
        let synced_at = if update_last_synced_at {
            let timestamp = Utc::now().to_rfc3339();
            let mut settings = self.settings_repository.get().await?;
            settings.bookmark_last_synced_at = Some(timestamp.clone());
            self.settings_repository.save(&settings).await?;
            Some(timestamp)
        } else {
            None
        };

        Ok(BookmarkSyncSummary {
            results,
            total_imported,
            total_skipped,
            synced_at,
        })
    }

    async fn import_single_browser(
        &self,
        app: AppHandle,
        browser: BookmarkBrowser,
    ) -> AppResult<BookmarkImportResult> {
        let Some(path) = self.browser_paths.resolve_bookmark_file(browser) else {
            return Ok(BookmarkImportResult {
                browser,
                path: None,
                imported_count: 0,
                skipped_count: 0,
                message: format!(
                    "{} bookmarks are not configured on this platform.",
                    browser.display_name()
                ),
            });
        };

        if !path.exists() {
            return Ok(BookmarkImportResult {
                browser,
                path: Some(path.display().to_string()),
                imported_count: 0,
                skipped_count: 0,
                message: format!("{} bookmark file not found.", browser.display_name()),
            });
        }

        let bytes = fs::read(&path).await?;
        let bookmark_file = serde_json::from_slice::<BookmarkFile>(&bytes)?;
        let parsed = parse_bookmark_tree(&bookmark_file)?;

        let mut imported_count = 0;
        let mut skipped_count = 0;

        for bookmark in parsed {
            if self
                .memory_repository
                .find_by_external_source(browser.as_source_app(), &bookmark.external_id)
                .await?
                .is_some()
            {
                skipped_count += 1;
                continue;
            }

            let memory = self
                .capture_service
                .create_bookmark(BookmarkCaptureInput {
                    browser,
                    external_id: bookmark.external_id,
                    title: bookmark.title,
                    url: bookmark.url,
                    folder_path: bookmark.folder_path,
                    created_at: bookmark.created_at,
                })
                .await?;

            self.link_enrichment_service
                .schedule_for_memory(app.clone(), memory)
                .await;

            imported_count += 1;
        }

        Ok(BookmarkImportResult {
            browser,
            path: Some(path.display().to_string()),
            imported_count,
            skipped_count,
            message: if imported_count == 0 && skipped_count == 0 {
                format!("No bookmarks found in {}.", browser.display_name())
            } else {
                format!(
                    "{} import complete: {} new, {} already saved.",
                    browser.display_name(),
                    imported_count,
                    skipped_count
                )
            },
        })
    }
}

fn supported_browsers() -> Vec<BookmarkBrowser> {
    vec![
        BookmarkBrowser::Chrome,
        BookmarkBrowser::Edge,
        BookmarkBrowser::Brave,
    ]
}

fn deduplicate_browsers(browsers: Vec<BookmarkBrowser>) -> Vec<BookmarkBrowser> {
    let mut seen = HashSet::new();
    browsers
        .into_iter()
        .filter(|browser| seen.insert(*browser))
        .collect()
}

fn parse_bookmark_tree(bookmark_file: &BookmarkFile) -> AppResult<Vec<ParsedBookmark>> {
    let mut parsed = Vec::new();

    for (root_key, root_node) in &bookmark_file.roots {
        let root_label = root_label(root_key, &root_node.name);
        collect_bookmarks(root_node, &[root_label], &mut parsed)?;
    }

    Ok(parsed)
}

fn collect_bookmarks(
    node: &BookmarkNode,
    breadcrumbs: &[String],
    parsed: &mut Vec<ParsedBookmark>,
) -> AppResult<()> {
    if node.node_type == "url" {
        let url = node
            .url
            .clone()
            .ok_or_else(|| AppError::Invalid("Bookmark entry is missing a URL.".into()))?;

        parsed.push(ParsedBookmark {
            external_id: if node.id.trim().is_empty() {
                url.clone()
            } else {
                node.id.clone()
            },
            title: if node.name.trim().is_empty() {
                url.clone()
            } else {
                node.name.trim().to_string()
            },
            url,
            folder_path: if breadcrumbs.is_empty() {
                None
            } else {
                Some(breadcrumbs.join(" / "))
            },
            created_at: parse_bookmark_timestamp(node.date_added.as_deref()),
        });

        return Ok(());
    }

    let mut next_breadcrumbs = breadcrumbs.to_vec();
    if !node.name.trim().is_empty()
        && next_breadcrumbs
            .last()
            .is_none_or(|last| last != node.name.trim())
    {
        next_breadcrumbs.push(node.name.trim().to_string());
    }

    for child in &node.children {
        collect_bookmarks(child, &next_breadcrumbs, parsed)?;
    }

    Ok(())
}

fn root_label(root_key: &str, fallback_name: &str) -> String {
    if !fallback_name.trim().is_empty() {
        return fallback_name.trim().to_string();
    }

    match root_key {
        "bookmark_bar" => "Bookmarks Bar".into(),
        "other" => "Other Bookmarks".into(),
        "synced" => "Synced".into(),
        "mobile" => "Mobile Bookmarks".into(),
        _ => "Bookmarks".into(),
    }
}

fn parse_bookmark_timestamp(value: Option<&str>) -> String {
    let Some(raw) = value else {
        return Utc::now().to_rfc3339();
    };

    let Ok(microseconds) = raw.parse::<i64>() else {
        return Utc::now().to_rfc3339();
    };

    let Some(base) = NaiveDate::from_ymd_opt(1601, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0))
    else {
        return Utc::now().to_rfc3339();
    };

    let Some(datetime) = base.checked_add_signed(chrono::TimeDelta::microseconds(microseconds))
    else {
        return Utc::now().to_rfc3339();
    };

    chrono::DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc).to_rfc3339()
}
