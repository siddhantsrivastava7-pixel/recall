mod safari_bookmarks;

use async_trait::async_trait;
use tokio::fs;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, Position, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    errors::app_error::AppResult,
    models::{AppContextSnapshot, BookmarkBrowser, RuntimePlatform, ShortcutBinding},
    platform::contracts::{
        AppContextAdapter, BrowserBookmarkReader, BrowserPathResolver, ClipboardAdapter,
        ClipboardImage, FileSystemAdapter, ParsedBookmarkRecord, ShortcutAdapter, StartupAdapter,
        WindowAdapter,
    },
    services::bookmark_parser::parse_chromium_bookmark_bytes,
};
use safari_bookmarks::parse_safari_bookmarks;

pub struct MacClipboardAdapter;
pub struct MacShortcutAdapter;
pub struct MacWindowAdapter;
pub struct MacAppContextAdapter;
pub struct MacFileSystemAdapter;
pub struct MacBrowserPathResolver;
pub struct MacBrowserBookmarkReader;
pub struct MacStartupAdapter;

const WIDGET_COLLAPSED_WIDTH: f64 = 142.0;
const WIDGET_COLLAPSED_HEIGHT: f64 = 48.0;

// macOS completion notes for future work:
// - Search overlay and widget currently use normal transparent Tauri windows. A later pass should
//   move them to NSPanel-style behavior so they can float above fullscreen Spaces more naturally.
// - Global shortcut behavior should be revalidated against macOS-specific modifier conventions,
//   app activation rules, and any permission prompts surfaced by the host OS.
// - Frontmost-app / active-window detection should move to a macOS-native implementation that
//   handles Accessibility permission checks explicitly before attempting window-title capture.
// - If Recall is notarized or sandboxed later, revisit any entitlements needed for shortcuts,
//   accessibility, and app-context inspection here rather than in shared UI code.

#[async_trait]
impl ClipboardAdapter for MacClipboardAdapter {
    async fn read_text(&self, app: &AppHandle) -> AppResult<Option<String>> {
        Ok(app.clipboard().read_text().ok())
    }

    async fn write_text(&self, app: &AppHandle, text: &str) -> AppResult<()> {
        app.clipboard()
            .write_text(text.to_string())
            .map_err(|error| crate::errors::app_error::AppError::Invalid(error.to_string()))?;
        Ok(())
    }

    async fn read_image(&self, app: &AppHandle) -> AppResult<Option<ClipboardImage>> {
        Ok(crate::platform::read_image_via_plugin(app))
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
                accelerator: "Super+Shift+S".into(),
                editable: true,
                description: "Open quick save with clipboard prefill".into(),
            },
            ShortcutBinding {
                action: "open-main-app".into(),
                accelerator: "Super+Shift+O".into(),
                editable: true,
                description: "Open main app".into(),
            },
            ShortcutBinding {
                action: "open-pointer".into(),
                accelerator: "Super+Shift+P".into(),
                editable: true,
                description: "Recall Pointer — bridge copied text to your saved memories".into(),
            },
        ]
    }
}

#[async_trait]
impl WindowAdapter for MacWindowAdapter {
    async fn ensure_widget(
        &self,
        app: &AppHandle,
        saved_position: Option<(f64, f64)>,
    ) -> AppResult<()> {
        let Some(window) = app.get_webview_window("widget") else {
            return Ok(());
        };

        if let Some((x, y)) = saved_position {
            window.set_position(Position::Logical(LogicalPosition::new(x, y)))?;
        } else if let Ok(Some(monitor)) = window.primary_monitor() {
            let scale = monitor.scale_factor();
            let screen_w = monitor.size().width as f64 / scale;
            let screen_h = monitor.size().height as f64 / scale;
            let x = (screen_w - WIDGET_COLLAPSED_WIDTH) / 2.0;
            let y = screen_h - WIDGET_COLLAPSED_HEIGHT - 32.0;
            window.set_position(Position::Logical(LogicalPosition::new(x, y)))?;
        }

        let _ = window.set_shadow(false);
        window.show()?;
        window.set_always_on_top(true)?;
        Ok(())
    }

    async fn set_widget_expanded(&self, _app: &AppHandle, _expanded: bool) -> AppResult<()> {
        // The current floating pill keeps the same 260x56 shell in collapsed and expanded states.
        Ok(())
    }

    async fn open_main(&self, app: &AppHandle) -> AppResult<()> {
        if let Some(window) = app.get_webview_window("main") {
            if window.is_minimized()? {
                window.unminimize()?;
            }
            window.show()?;
            window.set_focus()?;
        }
        Ok(())
    }

    async fn open_search_overlay(&self, app: &AppHandle) -> AppResult<()> {
        if let Some(window) = app.get_webview_window("search-overlay") {
            // macOS NSWindow draws a rectangular drop shadow by default —
            // visible as a "box" around the overlay's rounded panel. Disable
            // it here so the panel's own CSS box-shadow defines the silhouette.
            let _ = window.set_shadow(false);
            window.show()?;
            window.set_focus()?;
            window.center()?;
            window.set_always_on_top(true)?;
        }
        Ok(())
    }

    async fn open_quick_save(&self, app: &AppHandle) -> AppResult<()> {
        if let Some(window) = app.get_webview_window("quick-save") {
            let _ = window.set_shadow(false);
            window.show()?;
            window.set_focus()?;
            window.center()?;
            window.set_always_on_top(true)?;
        }
        Ok(())
    }

    async fn close_window(&self, window: &WebviewWindow) -> AppResult<()> {
        let label = window.label().to_string();
        if label == "main" {
            window.app_handle().exit(0);
        } else {
            window.hide()?;
        }
        Ok(())
    }

    async fn open_memory_in_main(&self, app: &AppHandle, memory_id: String) -> AppResult<()> {
        self.open_main(app).await?;
        if let Some(main_window) = app.get_webview_window("main") {
            main_window.emit("recall://open-memory", memory_id)?;
        }
        if let Some(overlay) = app.get_webview_window("search-overlay") {
            overlay.hide()?;
        }
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
        let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
        let app_support = home.join("Library/Application Support");

        Some(match browser {
            BookmarkBrowser::Chrome => app_support.join("Google/Chrome/Default/Bookmarks"),
            BookmarkBrowser::Edge => app_support.join("Microsoft Edge/Default/Bookmarks"),
            BookmarkBrowser::Brave => {
                app_support.join("BraveSoftware/Brave-Browser/Default/Bookmarks")
            }
            BookmarkBrowser::Safari => home.join("Library/Safari/Bookmarks.plist"),
        })
    }
}

#[async_trait]
impl BrowserBookmarkReader for MacBrowserBookmarkReader {
    async fn read_bookmarks(
        &self,
        browser: BookmarkBrowser,
        path: &std::path::Path,
    ) -> AppResult<Vec<ParsedBookmarkRecord>> {
        match browser {
            BookmarkBrowser::Chrome | BookmarkBrowser::Edge | BookmarkBrowser::Brave => {
                let bytes = fs::read(path).await?;
                parse_chromium_bookmark_bytes(&bytes)
            }
            BookmarkBrowser::Safari => parse_safari_bookmarks(path).await,
        }
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
