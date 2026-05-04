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
    pub extracted_text: Option<String>,
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
    /// OCR-extracted text from screenshot/imported_image memories.
    /// `None` until the AI scheduler runs OCR on the image. v0.2.0+.
    #[serde(default)]
    pub ocr_text: Option<String>,
    /// `NULL | 'pending' | 'running' | 'done' | 'failed'`. v0.2.0+.
    #[serde(default)]
    pub ocr_status: Option<String>,
    #[serde(default)]
    pub ocr_processed_at: Option<String>,
    #[serde(default)]
    pub ocr_engine: Option<String>,
    #[serde(default)]
    pub ocr_error: Option<String>,
    /// Embedding model version that produced the most recent
    /// successful embedding on any chunk of this memory. v0.3.0+.
    #[serde(default)]
    pub embedding_model_version: Option<String>,
    /// Timestamp of the most recent successful embedding for any
    /// chunk of this memory. v0.3.0+.
    #[serde(default)]
    pub embedding_generated_at: Option<String>,
    /// v0.5.18: LLM-generated summary cached on the memory row.
    /// Today only populated for daily-recap memories — the
    /// frontend renders this in place of the rule-based summary
    /// when present + fresh (`ai_summary_generated_at >= updated_at`
    /// modulo a small grace window). Generic shape so future
    /// memory kinds (long bookmarks, voice notes) can reuse it.
    #[serde(default)]
    pub ai_summary: Option<String>,
    /// v0.5.18: when the AI summary was generated. Used for
    /// staleness detection — if the memory's `updated_at` is
    /// newer, the renderer kicks off regeneration on detail open.
    #[serde(default)]
    pub ai_summary_generated_at: Option<String>,
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
    pub extracted_text: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub device_id: String,
    pub pairing_secret: String,
    pub desktop_name: String,
    pub endpoint: Option<String>,
    pub port: Option<u16>,
    pub created_at: String,
    pub receiver_running: bool,
    pub pairing_status: String,
    pub qr_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingQrPayload {
    pub protocol: String,
    pub version: u8,
    pub device_id: String,
    pub desktop_name: String,
    pub endpoint: Option<String>,
    pub secret: String,
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
    /// Master AI subsystem switch. **Off by default** — every AI feature
    /// (OCR in v0.2.0; embeddings/Ask Recall later) gates on this flag.
    pub ai_enabled: bool,
    /// Pause background AI work while the host is on battery. The
    /// scheduler still drains in-flight items; only new claims are blocked.
    pub ai_pause_on_battery: bool,
    /// Heavier AI work (Phase 1 = OCR; Phase 2+ = embedding backfill,
    /// model downloads) only runs while plugged into AC. Independent from
    /// `ai_pause_on_battery` — both defaults to `true`.
    pub ai_heavy_only_on_ac: bool,
    /// v0.5.6: one-shot backfill that re-runs the auto-tagger
    /// (with URL/UUID guards) and the new entity extractor against
    /// every memory. `None` on first launch of v0.5.6 (triggers
    /// the backfill); `Some(true)` afterwards. Stored on the
    /// settings row so the flag survives restarts and never
    /// re-runs on subsequent launches.
    #[serde(default)]
    pub ai_v0_5_6_backfill_done: Option<bool>,
    /// v0.5.7: independent flag for the corrected backfill that
    /// uses replace_auto_tagger_tags (removes stale tags) and
    /// runs is_recall_self_capture against existing memories'
    /// ocr_text. v0.5.6's pass had two bugs that left
    /// contamination in place; this flag forces a fresh run on
    /// upgrade even when v0.5.6's flag is already set.
    #[serde(default)]
    pub ai_v0_5_7_backfill_done: Option<bool>,
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
            // AI is opt-in. Existing users updating to v0.2.0 see zero
            // behavior change until they flip the master toggle.
            ai_enabled: false,
            ai_pause_on_battery: true,
            ai_heavy_only_on_ac: true,
            ai_v0_5_6_backfill_done: None,
            ai_v0_5_7_backfill_done: None,
        }
    }
}

/// One chunk row from `memory_chunks`. v0.3.0+. The `embedding_vector`
/// is the raw little-endian f32 BLOB; callers decode via
/// `EmbeddingVector::from_bytes`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryChunkRow {
    pub id: String,
    pub memory_id: String,
    pub chunk_index: i64,
    pub text: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub byte_size: i64,
    pub token_estimate: Option<i64>,
    pub content_hash: String,
    pub embedding_model: Option<String>,
    pub embedding_dim: Option<i64>,
    pub embedding_vector: Option<Vec<u8>>,
    pub embedding_generated_at: Option<String>,
    pub created_at: String,
}

/// v0.5.15: Lightweight session summary for the chat list in the
/// sidebar. Doesn't include messages — those load on demand when
/// the user opens the session. `display_title()` (frontend)
/// prefers `llm_title` when present, falls back to `title`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallSessionSummary {
    pub session_id: String,
    pub title: String,
    pub llm_title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub message_count: i64,
}

/// v0.5.15: Full session — summary plus the ordered list of
/// messages. Returned by `get_session(id)` when the user opens a
/// chat. The frontend re-hydrates AskView's thread state from
/// this.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallSessionFull {
    pub session_id: String,
    pub title: String,
    pub llm_title: Option<String>,
    pub created_at: String,
    pub last_used_at: String,
    pub messages: Vec<AskRecallMessageRow>,
}

/// v0.5.15: One message row from `ask_recall_messages`. Citations
/// and retrieved sources are JSON-encoded in storage; this struct
/// holds them as `Option<String>` JSON blobs that the command
/// layer decodes when serving to the frontend.
///
/// `role` is `"user"` or `"assistant"`. The other assistant-only
/// fields (tokens_generated / latency_ms / tag_intent / sources /
/// citations) are NULL on user rows.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AskRecallMessageRow {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub role: String,
    pub content: String,
    pub retrieved_sources: Option<String>,
    pub citations: Option<String>,
    pub tokens_generated: Option<i64>,
    pub latency_ms: Option<i64>,
    pub tag_intent: Option<String>,
    pub timestamp: String,
}

/// v0.5.6: row from `memory_entities` — one structured fact
/// extracted from a memory's content. The same memory typically
/// has several rows (one per detected person/company/product/etc.).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntityRow {
    pub id: String,
    pub memory_id: String,
    /// `"person" | "company" | "product" | "project" | "time-range"`.
    pub entity_type: String,
    /// Normalized display value (e.g. "Anthropic", "Q3 2024").
    pub entity_value: String,
    pub raw_match: String,
    pub confidence: f64,
    pub extracted_at: String,
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
