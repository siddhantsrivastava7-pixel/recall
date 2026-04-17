use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    errors::app_error::AppResult,
    models::{AppSettings, LicenseState, LinkEnrichmentUpdate, Memory, MemoryInput, Project},
};

#[async_trait]
pub trait MemoryRepository: Send + Sync {
    async fn list(&self) -> AppResult<Vec<Memory>>;
    async fn find(&self, id: &str) -> AppResult<Option<Memory>>;
    async fn find_by_external_source(
        &self,
        source_app: &str,
        external_id: &str,
    ) -> AppResult<Option<Memory>>;
    async fn create(&self, input: MemoryInput) -> AppResult<Memory>;
    async fn update(&self, id: &str, input: MemoryInput) -> AppResult<Memory>;
    async fn update_link_enrichment(
        &self,
        id: &str,
        enrichment: LinkEnrichmentUpdate,
    ) -> AppResult<Option<Memory>>;
    async fn set_resurface(
        &self,
        id: &str,
        resurface_at: Option<String>,
        updated_at: &str,
    ) -> AppResult<Option<Memory>>;
    async fn dismiss_resurface(
        &self,
        id: &str,
        dismissed_at: &str,
        updated_at: &str,
    ) -> AppResult<Option<Memory>>;
    async fn mark_opened(&self, id: &str, opened_at: &str) -> AppResult<Option<Memory>>;
    async fn delete(&self, id: &str) -> AppResult<()>;
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn list(&self) -> AppResult<Vec<Project>>;
    async fn create(&self, name: &str, description: Option<String>) -> AppResult<Project>;
    async fn update(&self, id: &str, name: &str, description: Option<String>)
        -> AppResult<Project>;
    async fn delete(&self, id: &str) -> AppResult<()>;
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self) -> AppResult<AppSettings>;
    async fn save(&self, settings: &AppSettings) -> AppResult<AppSettings>;
    async fn clear(&self) -> AppResult<()>;
}

#[async_trait]
pub trait LicenseRepository: Send + Sync {
    async fn get(&self) -> AppResult<LicenseState>;
    async fn save(&self, license_state: &LicenseState) -> AppResult<LicenseState>;
    async fn clear(&self) -> AppResult<()>;
}

pub type SharedMemoryRepository = Arc<dyn MemoryRepository>;
pub type SharedProjectRepository = Arc<dyn ProjectRepository>;
pub type SharedSettingsRepository = Arc<dyn SettingsRepository>;
pub type SharedLicenseRepository = Arc<dyn LicenseRepository>;
