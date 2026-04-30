use std::sync::{Arc, OnceLock};

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{Memory, MemoryInput},
    services::{
        capture_service::CaptureService,
        screenshot_store::{file_url_to_path, ScreenshotStore, SCREENSHOT_SOURCE_APP},
    },
};

pub struct MemoryService {
    repository: SharedMemoryRepository,
    capture_service: Arc<CaptureService>,
    /// Screenshot store handle, used to clean up the on-disk file when
    /// a screenshot memory is deleted. Empty until installed at boot;
    /// when missing, we silently skip the file unlink — the row still
    /// deletes, we just leak the file (a v0.2.x model GC pass cleans
    /// orphans on app upgrade).
    screenshot_store: OnceLock<ScreenshotStore>,
}

impl MemoryService {
    pub fn new(repository: SharedMemoryRepository, capture_service: Arc<CaptureService>) -> Self {
        Self {
            repository,
            capture_service,
            screenshot_store: OnceLock::new(),
        }
    }

    /// Install the screenshot store so that deleting a screenshot
    /// memory also deletes the underlying file. Idempotent.
    pub fn install_screenshot_store(&self, store: ScreenshotStore) {
        let _ = self.screenshot_store.set(store);
    }

    pub async fn list(&self) -> AppResult<Vec<Memory>> {
        self.repository.list().await
    }

    pub async fn get(&self, id: &str) -> AppResult<Option<Memory>> {
        self.repository.find(id).await
    }

    pub async fn create(&self, input: MemoryInput) -> AppResult<Memory> {
        self.capture_service.create(input).await
    }

    pub async fn update(&self, id: &str, input: MemoryInput) -> AppResult<Memory> {
        self.capture_service.update(id, input).await
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        // If the row is a screenshot memory and it lives in our
        // screenshots dir, unlink the file *after* the row deletes
        // (so a botched delete still leaves the row intact and
        // recoverable). Best-effort: a stray file is much better than
        // a row that won't delete because of a permissions hiccup on
        // the file.
        let target = self.repository.find(id).await?;
        self.repository.delete(id).await?;
        if let Some(memory) = target {
            if memory.source_app.as_deref() == Some(SCREENSHOT_SOURCE_APP) {
                if let (Some(store), Some(url)) =
                    (self.screenshot_store.get(), memory.url.as_deref())
                {
                    if let Some(path) = file_url_to_path(url) {
                        if let Err(error) = store.delete(&path).await {
                            eprintln!(
                                "[recall][memory-service] screenshot file cleanup failed for {id}: {error}"
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn duplicate(&self, id: &str) -> AppResult<Memory> {
        let original = self
            .repository
            .find(id)
            .await?
            .ok_or_else(|| AppError::Invalid("Memory not found.".into()))?;

        self.capture_service.duplicate_from_memory(original).await
    }

    pub async fn mark_opened(&self, id: &str) -> AppResult<Option<Memory>> {
        self.repository.mark_opened(id, &chrono::Utc::now().to_rfc3339()).await
    }

    pub async fn set_resurface(
        &self,
        id: &str,
        resurface_at: Option<String>,
    ) -> AppResult<Option<Memory>> {
        self.repository
            .set_resurface(id, resurface_at, &chrono::Utc::now().to_rfc3339())
            .await
    }

    pub async fn dismiss_resurface(&self, id: &str) -> AppResult<Option<Memory>> {
        let now = chrono::Utc::now().to_rfc3339();
        self.repository.dismiss_resurface(id, &now, &now).await
    }
}
