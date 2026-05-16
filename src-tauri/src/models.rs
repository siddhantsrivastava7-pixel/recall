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
    /// Master AI subsystem switch. v0.5.21 flipped the default to
    /// **true** for fresh installs — by this point OCR + embeddings +
    /// Ask Recall + recap have all baked, and "open the app, AI just
    /// works" is the right out-of-box experience for new users.
    /// Existing users keep whatever they had persisted; this only
    /// affects the initial settings row.
    pub ai_enabled: bool,
    /// Pause background AI work while the host is on battery. The
    /// scheduler still drains in-flight items; only new claims are blocked.
    pub ai_pause_on_battery: bool,
    /// Heavier AI work (Phase 1 = OCR; Phase 2+ = embedding backfill,
    /// model downloads) only runs while plugged into AC. Independent from
    /// `ai_pause_on_battery` — both defaults to `true`.
    pub ai_heavy_only_on_ac: bool,
    /// v0.5.21: how long the LLM stays loaded after the last call
    /// before the idle reaper unloads it. `0` = never unload (model
    /// stays resident). The reaper reads this value at the start of
    /// each tick so changes take effect within ~60 seconds without
    /// a restart. Default 5 minutes — cheap-enough cold reload but
    /// frees ~3.5 GB RAM when the user walks away.
    #[serde(default = "default_ai_llm_idle_minutes")]
    pub ai_llm_idle_minutes: u32,
    /// v0.5.21: optional override for the auto-detected hardware
    /// tier. `None` = use whatever `ai::hardware::detect()` reports;
    /// `Some(tier)` = pin to that tier (forces the matching LLM
    /// model on next app launch). Set from the AI Settings tab.
    /// Persisted as a lowercase letter ("a" / "b" / "c") in the
    /// flat key-value settings table; absence is treated as None.
    /// Changing this requires a restart to take effect because the
    /// LLM adapter is selected at boot from the tier value.
    #[serde(default)]
    pub ai_tier_override: Option<crate::ai::hardware::HardwareTier>,
    /// v0.5.22: pause background AI work when battery percent drops
    /// below this threshold. `0` = disabled (never pause based on
    /// battery percent). Independent from `ai_pause_on_battery`
    /// (that one fires whenever the host is on battery, ignoring
    /// charge level). Default 20 — matches the threshold most OS
    /// "low battery" warnings fire at. Has no effect on platforms
    /// where battery percent isn't readable (macOS today; desktops
    /// without batteries).
    #[serde(default = "default_ai_pause_below_battery_pct")]
    pub ai_pause_below_battery_pct: u32,
    /// v0.5.32: days a screenshot file lives on disk before the
    /// retention GC purges it. Memory rows + OCR text + content
    /// stay forever — only `memory.url` is cleared so the detail
    /// view stops trying to render a missing image. Default 60
    /// matches the v0.2.3-v0.5.31 hardcoded behavior. `0` =
    /// disabled (never purge), for power users who value image
    /// previews over disk space. Read by the GC loop on each
    /// pass — changes apply on the next 24-hour cycle.
    #[serde(default = "default_ai_screenshot_retention_days")]
    pub ai_screenshot_retention_days: u32,
    /// v0.5.38: per-file size cap for the file ingestion path.
    /// Files larger than this are skipped (no extraction
    /// attempted). Default 50 MB — bigger PDFs are usually
    /// scanned images where extraction is slow + low-value.
    /// `0` disables the cap.
    #[serde(default = "default_file_ingest_size_cap_mb")]
    pub file_ingest_size_cap_mb: u32,
    /// v0.5.38: max files imported on a single one-shot folder
    /// ingest. Beyond this we stop walking and report the
    /// remainder. Default 500. Watched-folder mode (v0.5.39+)
    /// has no equivalent cap — it streams over time, not in
    /// one go.
    #[serde(default = "default_folder_ingest_file_cap")]
    pub folder_ingest_file_cap: u32,
    /// v0.5.38: how deep we recurse into a folder on one-shot
    /// ingest. Default 8. Beyond this, the user should add the
    /// nested location separately rather than letting one
    /// ingest run blow up.
    #[serde(default = "default_folder_ingest_depth_cap")]
    pub folder_ingest_depth_cap: u32,
    /// v0.5.38: skip dot-prefixed (Unix hidden) and `Library/`,
    /// `node_modules/`, etc. (system-ish) folders on walk. The
    /// user can opt out per-walk later if needed; default-on
    /// is the safer trust posture.
    #[serde(default = "default_skip_hidden_folders")]
    pub skip_hidden_folders: bool,
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
            // v0.5.21: AI is **on by default** for fresh installs.
            // The features have baked through 20+ patch releases; the
            // out-of-box experience should be "open the app, AI just
            // works." Existing users who explicitly turned it off keep
            // their setting — the persisted row overrides this default
            // on settings load, so this flip only applies when there
            // is no `ai_enabled` key in the DB yet (= new install).
            ai_enabled: true,
            ai_pause_on_battery: true,
            ai_heavy_only_on_ac: true,
            ai_llm_idle_minutes: default_ai_llm_idle_minutes(),
            ai_tier_override: None,
            ai_pause_below_battery_pct: default_ai_pause_below_battery_pct(),
            ai_screenshot_retention_days: default_ai_screenshot_retention_days(),
            file_ingest_size_cap_mb: default_file_ingest_size_cap_mb(),
            folder_ingest_file_cap: default_folder_ingest_file_cap(),
            folder_ingest_depth_cap: default_folder_ingest_depth_cap(),
            skip_hidden_folders: default_skip_hidden_folders(),
            ai_v0_5_6_backfill_done: None,
            ai_v0_5_7_backfill_done: None,
        }
    }
}

/// v0.5.21: serde default for `ai_llm_idle_minutes`. Five minutes
/// is the empirical sweet spot — long enough to span follow-up
/// Ask Recall turns, short enough that walking away frees the
/// 3.5 GB resident model promptly.
fn default_ai_llm_idle_minutes() -> u32 {
    5
}

/// v0.5.22: serde default for `ai_pause_below_battery_pct`. 20%
/// matches the threshold most OS "low battery" warnings fire at.
fn default_ai_pause_below_battery_pct() -> u32 {
    20
}

/// v0.5.32: serde default for `ai_screenshot_retention_days`. 60
/// matches the v0.2.3-v0.5.31 hardcoded `RETENTION_DAYS`. Power
/// users who flip the dropdown to "Never" get the value `0`
/// stored, which the GC interprets as "skip the pass entirely."
fn default_ai_screenshot_retention_days() -> u32 {
    60
}

/// v0.5.38 file/folder ingestion caps. Defaults locked in
/// design discussion: 50 MB per file, 500 files per one-shot
/// folder, 8 levels of recursion, hidden/system folders skipped.
fn default_file_ingest_size_cap_mb() -> u32 {
    50
}
fn default_folder_ingest_file_cap() -> u32 {
    500
}
fn default_folder_ingest_depth_cap() -> u32 {
    8
}
fn default_skip_hidden_folders() -> bool {
    true
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

/// v0.5.38: File row from the `files` table.
///
/// Files aren't memories — they have filesystem lifecycle,
/// rich metadata, and (later) chunk-level embeddings. But they
/// surface in the existing search/recap/Ask Recall paths via
/// the shadow-memory bridge: each file row has a sibling
/// memory row with `source_app = "file"` and
/// `external_id = file.id`.
///
/// `path` is normalized + absolute. `parent_folder` is the
/// directory containing this file (also normalized). Hashing
/// content lets us detect "file touched but not actually
/// changed" so re-ingest doesn't re-embed unnecessarily.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub id: String,
    pub path: String,
    pub filename: String,
    pub extension: Option<String>,
    pub parent_folder: String,
    pub size_bytes: Option<i64>,
    pub file_created_at: Option<String>,
    pub file_modified_at: Option<String>,
    pub indexed_at: String,
    pub content_hash: Option<String>,
    pub extracted_text: Option<String>,
    pub summary_text: Option<String>,
    pub source_app: Option<String>,
    pub project_id: Option<String>,
    /// Bridge id back to the corresponding `memories` row. Null
    /// only during a brief window between file insert and
    /// shadow-memory create — repaired on next ingest cycle.
    pub shadow_memory_id: Option<String>,
}

/// v0.5.38: Folder row from the `folders` table. Aggregate
/// fields (top entities, centroid embedding, summary) land in
/// v0.5.39 — v0.5.38 ships only the structural columns so the
/// schema is stable from day one.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub parent_path: Option<String>,
    pub child_count: i64,
    /// JSON array of the most common file extensions inside this
    /// folder, sorted by count descending. e.g. `[".pdf",".md"]`.
    pub dominant_extensions: Option<String>,
    pub indexed_at: String,
    pub project_id: Option<String>,
}

/// v0.5.23: One row in the `proactive_surfaces` table — a card
/// shown at the top of Home. The selection engine in
/// `ai/surfaces/engine.rs` decides which `kind` wins for a given
/// session (Weekly recap on Monday / first-of-week, Forgotten Gold
/// otherwise). Dismissed or expired rows never render; the schema
/// keeps history rather than deleting so we can debug "what did we
/// surface to the user yesterday" if a surface lands wrong.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProactiveSurfaceRow {
    pub id: String,
    /// Surface kind. v0.5.23 ships with `'forgotten_gold'` and
    /// `'weekly_recap'`. v0.5.24+ may add `'project_briefing'`,
    /// `'researched_before'`, etc.
    pub kind: String,
    /// Memory referenced by the card. For Forgotten Gold this is
    /// the rediscovered memory; for Weekly recap this is the
    /// auto-generated weekly recap memory.
    pub memory_id: String,
    /// Higher = stronger signal. Comparable within a `kind` only.
    pub score: f64,
    /// Short, user-facing explanation rendered as the card's
    /// subtitle. e.g. "Saved 3 weeks ago. Related to your Acme
    /// deal work this week." `None` when the surface kind doesn't
    /// have a reasoning model yet.
    pub reason: Option<String>,
    pub surfaced_at: String,
    /// Set when the user clicks the dismiss button on the card.
    /// Once non-NULL, the row never renders again.
    pub dismissed_at: Option<String>,
    /// Optional hard expiry. Engines set this for time-bound
    /// surfaces (e.g. "this week's recap" expires the following
    /// Monday). `None` = never auto-expires; only dismissal
    /// removes it from the candidate set.
    pub expires_at: Option<String>,
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

/// v0.5.61 — Recall Pointer selection. Captured when the user
/// triggers the Pointer hotkey: the current clipboard text plus
/// whatever app context the platform adapter could resolve.
/// Stashed in AppState (single slot, overwritten each trigger)
/// and pulled exactly once by the frontend via
/// `pointer_take_selection` — the take semantics mean a stale
/// selection never leaks into a later, unrelated overlay open.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerSelection {
    /// The selected (clipboard) text. Always present — the
    /// trigger no-ops when the clipboard has no text.
    pub text: String,
    /// Frontmost app when the hotkey fired, if the platform
    /// adapter could resolve it (e.g. "Safari", "Code").
    pub source_app: Option<String>,
    /// Foreground window title, if available. Often carries the
    /// page title in browsers / the file name in editors.
    pub source_window: Option<String>,
    /// RFC3339 capture timestamp. Used for the "just now" label
    /// and as the memory's created_at if the user saves.
    pub captured_at: String,
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
