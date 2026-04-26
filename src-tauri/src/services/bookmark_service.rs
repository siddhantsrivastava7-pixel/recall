use std::{collections::HashSet, sync::Arc};

use chrono::Utc;

use crate::{
    db::repositories::{SharedMemoryRepository, SharedSettingsRepository},
    errors::app_error::AppResult,
    models::{BookmarkBrowser, BookmarkImportResult, BookmarkSourceStatus, BookmarkSyncSummary},
    platform::contracts::{BrowserBookmarkReader, BrowserPathResolver},
    services::{
        capture_service::{BookmarkCaptureInput, CaptureService},
        link_enrichment_service::LinkEnrichmentService,
    },
};
use tauri::AppHandle;

pub struct BookmarkIngestionService {
    memory_repository: SharedMemoryRepository,
    capture_service: Arc<CaptureService>,
    settings_repository: SharedSettingsRepository,
    browser_paths: Arc<dyn BrowserPathResolver>,
    browser_bookmarks: Arc<dyn BrowserBookmarkReader>,
    link_enrichment_service: Arc<LinkEnrichmentService>,
}

impl BookmarkIngestionService {
    pub fn new(
        memory_repository: SharedMemoryRepository,
        capture_service: Arc<CaptureService>,
        settings_repository: SharedSettingsRepository,
        browser_paths: Arc<dyn BrowserPathResolver>,
        browser_bookmarks: Arc<dyn BrowserBookmarkReader>,
        link_enrichment_service: Arc<LinkEnrichmentService>,
    ) -> Self {
        Self {
            memory_repository,
            capture_service,
            settings_repository,
            browser_paths,
            browser_bookmarks,
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

        let parsed = self.browser_bookmarks.read_bookmarks(browser, &path).await?;

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
    #[cfg(target_os = "macos")]
    {
        vec![
            BookmarkBrowser::Chrome,
            BookmarkBrowser::Edge,
            BookmarkBrowser::Brave,
            BookmarkBrowser::Safari,
        ]
    }

    #[cfg(not(target_os = "macos"))]
    {
        vec![
            BookmarkBrowser::Chrome,
            BookmarkBrowser::Edge,
            BookmarkBrowser::Brave,
        ]
    }
}

fn deduplicate_browsers(browsers: Vec<BookmarkBrowser>) -> Vec<BookmarkBrowser> {
    let mut seen = HashSet::new();
    browsers
        .into_iter()
        .filter(|browser| seen.insert(*browser))
        .collect()
}
