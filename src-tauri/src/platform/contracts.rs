use async_trait::async_trait;
use tauri::{AppHandle, WebviewWindow};

use crate::{
    errors::app_error::AppResult,
    models::{AppContextSnapshot, BookmarkBrowser, RuntimePlatform, ShortcutBinding},
};

/// Raw clipboard image, decoded into RGBA pixels. Width/height are in
/// pixels; `rgba` is `width * height * 4` bytes laid out row-major
/// (R, G, B, A per pixel). Adapters return this shape rather than
/// platform-specific image handles so the consumer (clipboard watcher)
/// stays platform-agnostic.
#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[async_trait]
pub trait ClipboardAdapter: Send + Sync {
    async fn read_text(&self, app: &AppHandle) -> AppResult<Option<String>>;
    async fn write_text(&self, app: &AppHandle, text: &str) -> AppResult<()>;
    /// Read the current clipboard image, if any. Returns `Ok(None)` when
    /// no image is on the clipboard or the host can't decode it (the
    /// clipboard plugin classifies the absence of an image as an
    /// error — we map to `None` so the caller can branch cleanly).
    async fn read_image(&self, app: &AppHandle) -> AppResult<Option<ClipboardImage>>;
}

pub trait ShortcutAdapter: Send + Sync {
    fn bindings(&self) -> Vec<ShortcutBinding>;
}

#[async_trait]
pub trait WindowAdapter: Send + Sync {
    async fn ensure_widget(
        &self,
        app: &AppHandle,
        saved_position: Option<(f64, f64)>,
    ) -> AppResult<()>;
    async fn set_widget_expanded(&self, app: &AppHandle, expanded: bool) -> AppResult<()>;
    async fn open_main(&self, app: &AppHandle) -> AppResult<()>;
    async fn open_search_overlay(&self, app: &AppHandle) -> AppResult<()>;
    async fn open_quick_save(&self, app: &AppHandle) -> AppResult<()>;
    async fn close_window(&self, window: &WebviewWindow) -> AppResult<()>;
    async fn open_memory_in_main(&self, app: &AppHandle, memory_id: String) -> AppResult<()>;
}

#[async_trait]
pub trait AppContextAdapter: Send + Sync {
    async fn detect_context(&self) -> AppResult<AppContextSnapshot>;
    fn platform(&self) -> RuntimePlatform;
}

#[async_trait]
pub trait FileSystemAdapter: Send + Sync {
    async fn choose_export_path(&self, app: &AppHandle) -> AppResult<Option<std::path::PathBuf>>;
    async fn choose_import_path(&self, app: &AppHandle) -> AppResult<Option<std::path::PathBuf>>;
}

pub trait BrowserPathResolver: Send + Sync {
    fn resolve_bookmark_file(&self, browser: BookmarkBrowser) -> Option<std::path::PathBuf>;
}

#[derive(Debug, Clone)]
pub struct ParsedBookmarkRecord {
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub folder_path: Option<String>,
    pub created_at: String,
}

#[async_trait]
pub trait BrowserBookmarkReader: Send + Sync {
    async fn read_bookmarks(
        &self,
        browser: BookmarkBrowser,
        path: &std::path::Path,
    ) -> AppResult<Vec<ParsedBookmarkRecord>>;
}

#[async_trait]
pub trait StartupAdapter: Send + Sync {
    async fn apply_launch_on_startup(&self, app: &AppHandle, enabled: bool) -> AppResult<()>;
}
