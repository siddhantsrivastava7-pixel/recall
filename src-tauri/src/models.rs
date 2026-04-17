use sqlx::types::Json;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT")]
#[sqlx(rename_all = "lowercase")]
pub enum MemorySourceType {
    #[default]
    Manual,
    Bookmark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT")]
#[sqlx(rename_all = "lowercase")]
pub enum LinkEnrichmentStatus {
    Pending,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT")]
#[sqlx(rename_all = "snake_case")]
pub enum MemoryType {
    Article,
    Docs,
    Tool,
    Bookmark,
    Note,
    CodeSnippet,
    Video,
    Post,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BookmarkBrowser {
    Chrome,
    Edge,
    Brave,
    Safari,
}

impl BookmarkBrowser {
    pub fn as_source_app(self) -> &'static str {
        match self {
            BookmarkBrowser::Chrome => "chrome",
            BookmarkBrowser::Edge => "edge",
            BookmarkBrowser::Brave => "brave",
            BookmarkBrowser::Safari => "safari",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            BookmarkBrowser::Chrome => "Chrome",
            BookmarkBrowser::Edge => "Edge",
            BookmarkBrowser::Brave => "Brave",
            BookmarkBrowser::Safari => "Safari",
        }
    }

    pub fn default_sync_browsers() -> Vec<Self> {
        #[cfg(target_os = "macos")]
        {
            vec![Self::Chrome, Self::Edge, Self::Brave, Self::Safari]
        }

        #[cfg(not(target_os = "macos"))]
        {
            vec![Self::Chrome, Self::Edge, Self::Brave]
        }
    }

    #[cfg(target_os = "macos")]
    pub fn legacy_default_sync_browsers() -> Vec<Self> {
        vec![Self::Chrome, Self::Edge, Self::Brave]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    #[serde(default)]
    pub source_type: MemorySourceType,
    pub title: Option<String>,
    pub content: String,
    pub note: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    pub resolved_domain: Option<String>,
    pub canonical_url: Option<String>,
    pub resolved_title: Option<String>,
    pub resolved_description: Option<String>,
    pub resolved_image: Option<String>,
    pub resolved_site_name: Option<String>,
    pub preview_text: Option<String>,
    pub summary_text: Option<String>,
    pub memory_type: Option<MemoryType>,
    pub topic_labels: Option<Json<Vec<String>>>,
    pub primary_topic: Option<String>,
    pub quality_score: Option<f64>,
    pub bookmark_quality_score: Option<f64>,
    pub is_duplicate_of: Option<String>,
    pub bookmark_folder_path: Option<String>,
    pub enrichment_status: Option<LinkEnrichmentStatus>,
    pub enrichment_error: Option<String>,
    pub enriched_at: Option<String>,
    pub last_enriched_at: Option<String>,
    pub external_id: Option<String>,
    pub folder_path: Option<String>,
    pub source_app: Option<String>,
    pub source_window: Option<String>,
    pub resurface_at: Option<String>,
    pub resurface_dismissed_at: Option<String>,
    pub last_opened_at: Option<String>,
    pub open_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInput {
    pub source_type: Option<MemorySourceType>,
    pub title: Option<String>,
    pub content: String,
    pub note: Option<String>,
    pub project_id: Option<String>,
    pub url: Option<String>,
    pub external_id: Option<String>,
    pub folder_path: Option<String>,
    pub source_app: Option<String>,
    pub source_window: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkEnrichmentUpdate {
    pub url: Option<String>,
    pub domain: Option<String>,
    pub resolved_domain: Option<String>,
    pub canonical_url: Option<String>,
    pub resolved_title: Option<String>,
    pub resolved_description: Option<String>,
    pub resolved_image: Option<String>,
    pub resolved_site_name: Option<String>,
    pub preview_text: Option<String>,
    pub summary_text: Option<String>,
    pub memory_type: Option<MemoryType>,
    pub topic_labels: Option<Vec<String>>,
    pub primary_topic: Option<String>,
    pub quality_score: Option<f64>,
    pub bookmark_quality_score: Option<f64>,
    pub is_duplicate_of: Option<String>,
    pub bookmark_folder_path: Option<String>,
    pub enrichment_status: LinkEnrichmentStatus,
    pub enrichment_error: Option<String>,
    pub enriched_at: Option<String>,
    pub last_enriched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub floating_widget_enabled: bool,
    pub launch_on_startup_enabled: bool,
    pub update_auto_check_enabled: bool,
    pub bookmark_auto_sync_enabled: bool,
    pub bookmark_sync_interval_minutes: u32,
    pub bookmark_sync_browsers: Vec<BookmarkBrowser>,
    pub bookmark_last_synced_at: Option<String>,
    pub widget_position_x: Option<f64>,
    pub widget_position_y: Option<f64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            floating_widget_enabled: true,
            launch_on_startup_enabled: false,
            update_auto_check_enabled: true,
            bookmark_auto_sync_enabled: true,
            bookmark_sync_interval_minutes: 15,
            bookmark_sync_browsers: BookmarkBrowser::default_sync_browsers(),
            bookmark_last_synced_at: None,
            widget_position_x: None,
            widget_position_y: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LicenseState {
    pub id: String,
    pub license_key: Option<String>,
    pub is_activated: bool,
    pub is_trial: bool,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub last_checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub action: String,
    pub accelerator: String,
    pub editable: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimePlatform {
    Windows,
    Macos,
    Linux,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub platform: RuntimePlatform,
    pub current_window_label: String,
    pub database_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppContextSnapshot {
    pub source_app: Option<String>,
    pub source_window: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub runtime: RuntimeInfo,
    pub settings: AppSettings,
    pub license: LicenseState,
    pub memories: Vec<Memory>,
    pub projects: Vec<Project>,
    pub shortcuts: Vec<ShortcutBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPayload {
    pub exported_at: String,
    pub version: String,
    pub memories: Vec<Memory>,
    pub projects: Vec<Project>,
    pub settings: AppSettings,
    pub license: LicenseState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkSourceStatus {
    pub browser: BookmarkBrowser,
    pub path: Option<String>,
    pub is_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkImportResult {
    pub browser: BookmarkBrowser,
    pub path: Option<String>,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkSyncSummary {
    pub results: Vec<BookmarkImportResult>,
    pub total_imported: usize,
    pub total_skipped: usize,
    pub synced_at: Option<String>,
}
