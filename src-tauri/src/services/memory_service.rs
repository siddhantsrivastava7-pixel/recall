use std::sync::Arc;

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{Memory, MemoryInput},
    services::capture_service::CaptureService,
};

pub struct MemoryService {
    repository: SharedMemoryRepository,
    capture_service: Arc<CaptureService>,
}

impl MemoryService {
    pub fn new(repository: SharedMemoryRepository, capture_service: Arc<CaptureService>) -> Self {
        Self {
            repository,
            capture_service,
        }
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
        self.repository.delete(id).await
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
