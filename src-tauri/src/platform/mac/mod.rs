use async_trait::async_trait;
use tauri::{AppHandle, WebviewWindow};

use crate::{
    errors::app_error::AppResult,
    models::{AppContextSnapshot, BookmarkBrowser, RuntimePlatform, ShortcutBinding},
    platform::contracts::{
        AppContextAdapter, BrowserPathResolver, ClipboardAdapter, FileSystemAdapter,
        ShortcutAdapter, StartupAdapter, WindowAdapter,
    },
};

pub struct MacClipboardAdapter;
pub struct MacShortcutAdapter;
pub struct MacWindowAdapter;
pub struct MacAppContextAdapter;
pub struct MacFileSystemAdapter;
pub struct MacBrowserPathResolver;
pub struct MacStartupAdapter;

// macOS completion notes for future work:
// - Search overlay and widget should likely move to NSPanel-style behavior so they can float
//   without feeling like regular app windows.
// - Global shortcut behavior should be revalidated against macOS-specific modifier conventions,
//   app activation rules, and any permission prompts surfaced by the host OS.
// - Frontmost-app / active-window detection should move to a macOS-native implementation that
//   handles Accessibility permission checks explicitly before attempting window-title capture.
// - If Recall is notarized or sandboxed later, revisit any entitlements needed for shortcuts,
//   accessibility, and app-context inspection here rather than in shared UI code.

#[async_trait]
impl ClipboardAdapter for MacClipboardAdapter {
    async fn read_text(&self, _app: &AppHandle) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn write_text(&self, _app: &AppHandle, _text: &str) -> AppResult<()> {
        Ok(())
    }
}

impl ShortcutAdapter for MacShortcutAdapter {
    fn bindings(&self) -> Vec<ShortcutBinding> {
        vec![
            ShortcutBinding {
                action: "open-search".into(),
                accelerator: "Alt+Space".into(),
                editable: true,
                description: "Open search overlay".into(),
            },
            ShortcutBinding {
                action: "open-quick-save".into(),
                accelerator: "Ctrl+Shift+S".into(),
                editable: true,
                description: "Open quick save with clipboard prefill".into(),
            },
            ShortcutBinding {
                action: "open-main-app".into(),
                accelerator: "Ctrl+Shift+O".into(),
                editable: true,
                description: "Open main app".into(),
            },
        ]
    }
}

#[async_trait]
impl WindowAdapter for MacWindowAdapter {
    async fn ensure_widget(&self, _app: &AppHandle, _saved_position: Option<(f64, f64)>) -> AppResult<()> {
        Ok(())
    }

    async fn set_widget_expanded(&self, _app: &AppHandle, _expanded: bool) -> AppResult<()> {
        Ok(())
    }

    async fn open_main(&self, _app: &AppHandle) -> AppResult<()> {
        Ok(())
    }

    async fn open_search_overlay(&self, _app: &AppHandle) -> AppResult<()> {
        Ok(())
    }

    async fn open_quick_save(&self, _app: &AppHandle) -> AppResult<()> {
        Ok(())
    }

    async fn close_window(&self, _window: &WebviewWindow) -> AppResult<()> {
        Ok(())
    }

    async fn open_memory_in_main(&self, _app: &AppHandle, _memory_id: String) -> AppResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AppContextAdapter for MacAppContextAdapter {
    async fn detect_context(&self) -> AppResult<AppContextSnapshot> {
        Ok(AppContextSnapshot {
            source_app: None,
            source_window: None,
        })
    }

    fn platform(&self) -> RuntimePlatform {
        RuntimePlatform::Macos
    }
}

#[async_trait]
impl FileSystemAdapter for MacFileSystemAdapter {
    async fn choose_export_path(&self, _app: &AppHandle) -> AppResult<Option<std::path::PathBuf>> {
        Ok(None)
    }

    async fn choose_import_path(&self, _app: &AppHandle) -> AppResult<Option<std::path::PathBuf>> {
        Ok(None)
    }
}

impl BrowserPathResolver for MacBrowserPathResolver {
    fn resolve_bookmark_file(&self, browser: BookmarkBrowser) -> Option<std::path::PathBuf> {
        let home = std::env::var_os("HOME")?;
        let base = std::path::PathBuf::from(home).join("Library/Application Support");

        let path = match browser {
            BookmarkBrowser::Chrome => base.join("Google/Chrome/Default/Bookmarks"),
            BookmarkBrowser::Edge => base.join("Microsoft Edge/Default/Bookmarks"),
            BookmarkBrowser::Brave => {
                base.join("BraveSoftware/Brave-Browser/Default/Bookmarks")
            }
        };

        // Safari bookmark ingestion should be completed later with a dedicated resolver and
        // permission strategy. Its storage format differs from Chromium-based browsers enough
        // that it should stay out of the shared bookmark ingestion service for now.
        Some(path)
    }
}

#[async_trait]
impl StartupAdapter for MacStartupAdapter {
    async fn apply_launch_on_startup(&self, _app: &AppHandle, _enabled: bool) -> AppResult<()> {
        // macOS startup integration should be completed later with LaunchAgent handling or the
        // official autostart plugin once behavior and permissions are finalized for that platform.
        Ok(())
    }
}
