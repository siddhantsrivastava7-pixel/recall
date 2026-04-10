use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use sqlx::SqlitePool;

use crate::{
    db::repositories::{
        SharedLicenseRepository, SharedMemoryRepository, SharedProjectRepository,
        SharedSettingsRepository,
    },
    platform::factory::PlatformServices,
    services::{
        bookmark_service::BookmarkIngestionService,
        capture_service::CaptureService,
        license_service::{LicenseService, LocalLicenseVerifier},
        link_enrichment_service::LinkEnrichmentService,
        memory_service::MemoryService,
        project_service::ProjectService,
        settings_service::SettingsService,
        shortcut_service::ShortcutService,
    },
};

pub struct AppState {
    pub pool: SqlitePool,
    pub database_path: PathBuf,
    pub memory_repository: SharedMemoryRepository,
    pub project_repository: SharedProjectRepository,
    pub settings_repository: SharedSettingsRepository,
    pub license_repository: SharedLicenseRepository,
    pub memory_service: Arc<MemoryService>,
    pub project_service: Arc<ProjectService>,
    pub settings_service: Arc<SettingsService>,
    pub shortcut_service: Arc<ShortcutService>,
    pub license_service: Arc<LicenseService>,
    pub bookmark_service: Arc<BookmarkIngestionService>,
    pub link_enrichment_service: Arc<LinkEnrichmentService>,
    pub platform: PlatformServices,
    /// Set if initialization failed — bootstrap_app returns this as an error
    pub init_error: Option<String>,
    pub startup_bookmark_sync_completed: AtomicBool,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: SqlitePool,
        database_path: PathBuf,
        memory_repository: SharedMemoryRepository,
        project_repository: SharedProjectRepository,
        settings_repository: SharedSettingsRepository,
        license_repository: SharedLicenseRepository,
        platform: PlatformServices,
    ) -> Self {
        let capture_service =
            Arc::new(CaptureService::new(pool.clone(), memory_repository.clone()));
        let memory_service = Arc::new(MemoryService::new(
            memory_repository.clone(),
            capture_service.clone(),
        ));
        let project_service = Arc::new(ProjectService::new(project_repository.clone()));
        let settings_service = Arc::new(SettingsService::new(settings_repository.clone()));
        let shortcut_service = Arc::new(ShortcutService::new(pool.clone()));
        let license_service = Arc::new(LicenseService::new(
            license_repository.clone(),
            Box::new(LocalLicenseVerifier),
        ));
        let link_enrichment_service = Arc::new(
            LinkEnrichmentService::new(memory_repository.clone())
                .expect("link enrichment service should initialize"),
        );
        let bookmark_service = Arc::new(BookmarkIngestionService::new(
            memory_repository.clone(),
            capture_service.clone(),
            settings_repository.clone(),
            platform.browser_paths.clone(),
            link_enrichment_service.clone(),
        ));

        Self {
            pool,
            database_path,
            memory_repository,
            project_repository,
            settings_repository,
            license_repository,
            memory_service,
            project_service,
            settings_service,
            shortcut_service,
            license_service,
            bookmark_service,
            link_enrichment_service,
            platform,
            init_error: None,
            startup_bookmark_sync_completed: AtomicBool::new(false),
        }
    }
}
