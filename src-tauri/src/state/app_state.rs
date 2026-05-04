use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, OnceLock},
};

use sqlx::SqlitePool;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    ai::llm::AskRecallAdapter,
    ai::scheduler::AiScheduler,
    db::repositories::{
        SharedAskRecallSessionRepository, SharedLicenseRepository, SharedMemoryRepository,
        SharedProactiveSurfaceRepository, SharedProjectRepository, SharedSettingsRepository,
    },
    platform::factory::PlatformServices,
    services::{
        bookmark_service::BookmarkIngestionService,
        capture_service::CaptureService,
        license_service::{LicenseService, LocalLicenseVerifier},
        link_enrichment_service::LinkEnrichmentService,
        memory_service::MemoryService,
        pairing_service::PairingService,
        project_service::ProjectService,
        receiver_service::DesktopReceiverService,
        screenshot_store::ScreenshotStore,
        settings_service::SettingsService,
        shortcut_service::ShortcutService,
        spoken_transcript_service::SpokenTranscriptService,
    },
};

pub struct AppState {
    pub pool: SqlitePool,
    pub database_path: PathBuf,
    pub memory_repository: SharedMemoryRepository,
    pub project_repository: SharedProjectRepository,
    pub settings_repository: SharedSettingsRepository,
    pub license_repository: SharedLicenseRepository,
    pub ask_recall_session_repository: SharedAskRecallSessionRepository,
    /// v0.5.23: proactive surface storage (Forgotten Gold, Weekly recap, etc.).
    /// Held as a repo trait object so the engine can record/dismiss
    /// surfaces without coupling to SQLite concretely.
    pub proactive_surface_repository: SharedProactiveSurfaceRepository,
    pub memory_service: Arc<MemoryService>,
    pub project_service: Arc<ProjectService>,
    pub settings_service: Arc<SettingsService>,
    pub shortcut_service: Arc<ShortcutService>,
    pub license_service: Arc<LicenseService>,
    pub bookmark_service: Arc<BookmarkIngestionService>,
    pub link_enrichment_service: Arc<LinkEnrichmentService>,
    pub spoken_transcript_service: Arc<SpokenTranscriptService>,
    pub pairing_service: Arc<PairingService>,
    pub receiver_service: Arc<DesktopReceiverService>,
    pub platform: PlatformServices,
    /// AI subsystem handle. Empty until initialized in `setup()` after the
    /// window opens (kept off the bootstrap path so first paint is never
    /// blocked on hardware probing or scheduler spawn). When the host has
    /// no native OCR adapter the handle is still installed and reports
    /// `ocr_engine = "unsupported"`. `OnceLock` lets us write through
    /// `&AppState` (which is all `tauri::State` exposes) without an
    /// unsafe cell or a `Mutex` we'd never lock for writes again.
    ai_scheduler_cell: OnceLock<AiScheduler>,
    /// On-disk store for clipboard-image captures. Installed in
    /// `setup()` once the AppHandle is available. `None` very briefly
    /// during the window between AppState construction and `setup()` —
    /// no clipboard watcher work runs in that window.
    screenshot_store_cell: OnceLock<ScreenshotStore>,
    /// v0.4.0: Ask Recall LLM adapter. Installed alongside the AI
    /// scheduler in `setup()` — same lazy-after-window-opens pattern
    /// to keep first paint cheap. Tier-aware (1.5B/3B/7B Qwen2.5).
    /// `None` when no LLM adapter is configured for this host.
    llm_adapter_cell: OnceLock<Arc<dyn AskRecallAdapter>>,
    /// v0.5.11: per-session cancel handles. Stored separately from
    /// session storage so the cancel command can drop a flag
    /// without taking the sessions lock for the duration of
    /// generation. Cleared after a turn completes (or on cancel).
    /// v0.5.15: the in-memory sessions HashMap moved to SQLite (see
    /// `ask_recall_session_repository`); this map persists only
    /// for the runtime cancel signal.
    pub ask_recall_cancel_handles:
        Arc<AsyncMutex<HashMap<String, crate::ai::ask::session::CancelHandle>>>,
    /// Capture service is exposed on AppState so the AI scheduler hook can
    /// re-use the existing post-save path. Held as Arc for cheap clones.
    pub capture_service: Arc<CaptureService>,
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
        ask_recall_session_repository: SharedAskRecallSessionRepository,
        proactive_surface_repository: SharedProactiveSurfaceRepository,
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
        let spoken_transcript_service =
            Arc::new(SpokenTranscriptService::new(memory_repository.clone()));
        // v0.5.18: wire the post-save Daily recap rebuild hook into
        // the capture service. After this point, every successful
        // create/update fires a fire-and-forget rebuild of today's
        // recap memory so it stays in sync as new captures land
        // (screenshots, bookmarks, notes, etc.).
        capture_service.install_recap_service(spoken_transcript_service.clone());
        let pairing_service = Arc::new(PairingService::new(pool.clone()));
        let receiver_service = Arc::new(DesktopReceiverService::new(
            pairing_service.clone(),
            memory_service.clone(),
            memory_repository.clone(),
            link_enrichment_service.clone(),
        ));
        let bookmark_service = Arc::new(BookmarkIngestionService::new(
            memory_repository.clone(),
            capture_service.clone(),
            settings_repository.clone(),
            platform.browser_paths.clone(),
            platform.browser_bookmarks.clone(),
            link_enrichment_service.clone(),
        ));

        Self {
            pool,
            database_path,
            memory_repository,
            project_repository,
            settings_repository,
            license_repository,
            ask_recall_session_repository,
            proactive_surface_repository,
            memory_service,
            project_service,
            settings_service,
            shortcut_service,
            license_service,
            bookmark_service,
            link_enrichment_service,
            spoken_transcript_service,
            pairing_service,
            receiver_service,
            platform,
            ai_scheduler_cell: OnceLock::new(),
            screenshot_store_cell: OnceLock::new(),
            llm_adapter_cell: OnceLock::new(),
            ask_recall_cancel_handles: Arc::new(AsyncMutex::new(HashMap::new())),
            capture_service,
            init_error: None,
            startup_bookmark_sync_completed: AtomicBool::new(false),
        }
    }

    /// Read the AI scheduler handle, if it has been installed by the
    /// startup hook. Returns `None` for the brief window between window
    /// open and `start_ai_scheduler` running, and forever on hosts where
    /// scheduler init failed (extremely rare — only an OOM at boot).
    pub fn ai_scheduler(&self) -> Option<&AiScheduler> {
        self.ai_scheduler_cell.get()
    }

    /// Install the AI scheduler. Idempotent: a second call is a no-op
    /// and returns the previously-installed handle. Called exactly once
    /// at startup from `lib.rs::start_ai_scheduler`.
    pub fn install_ai_scheduler(&self, scheduler: AiScheduler) -> &AiScheduler {
        let _ = self.ai_scheduler_cell.set(scheduler);
        self.ai_scheduler_cell
            .get()
            .expect("scheduler should be present after set/get_or_init")
    }

    /// Read the screenshot store handle. `None` until installed at
    /// startup; nothing depends on it being present synchronously
    /// during AppState::new because the clipboard watcher doesn't run
    /// until after `setup()` finishes.
    pub fn screenshot_store(&self) -> Option<&ScreenshotStore> {
        self.screenshot_store_cell.get()
    }

    /// Install the screenshot store. Idempotent.
    pub fn install_screenshot_store(&self, store: ScreenshotStore) {
        let _ = self.screenshot_store_cell.set(store);
    }

    /// v0.4.0: read the Ask Recall LLM adapter. `None` until
    /// installed at startup, or forever on hosts where init failed.
    pub fn llm_adapter(&self) -> Option<&Arc<dyn AskRecallAdapter>> {
        self.llm_adapter_cell.get()
    }

    /// v0.4.0: install the Ask Recall LLM adapter. Idempotent.
    pub fn install_llm_adapter(&self, adapter: Arc<dyn AskRecallAdapter>) {
        let _ = self.llm_adapter_cell.set(adapter);
    }
}
