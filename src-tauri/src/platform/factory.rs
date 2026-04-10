use std::sync::Arc;

use crate::platform::contracts::{
    AppContextAdapter, BrowserPathResolver, ClipboardAdapter, FileSystemAdapter, ShortcutAdapter,
    StartupAdapter, WindowAdapter,
};

#[cfg(target_os = "macos")]
use crate::platform::mac::{
    MacAppContextAdapter, MacBrowserPathResolver, MacClipboardAdapter, MacFileSystemAdapter,
    MacShortcutAdapter, MacStartupAdapter, MacWindowAdapter,
};

#[cfg(target_os = "windows")]
use crate::platform::windows::{
    WindowsAppContextAdapter, WindowsBrowserPathResolver, WindowsClipboardAdapter,
    WindowsFileSystemAdapter, WindowsShortcutAdapter, WindowsStartupAdapter, WindowsWindowAdapter,
};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use crate::{
    models::{AppContextSnapshot, BookmarkBrowser, RuntimePlatform, ShortcutBinding},
    platform::contracts::{
        AppContextAdapter as _, BrowserPathResolver as _, ClipboardAdapter as _,
        FileSystemAdapter as _, ShortcutAdapter as _, StartupAdapter as _, WindowAdapter as _,
    },
};
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use async_trait::async_trait;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use tauri::{AppHandle, WebviewWindow};

pub struct PlatformServices {
    pub clipboard: Arc<dyn ClipboardAdapter>,
    pub shortcuts: Arc<dyn ShortcutAdapter>,
    pub window: Arc<dyn WindowAdapter>,
    pub app_context: Arc<dyn AppContextAdapter>,
    pub file_system: Arc<dyn FileSystemAdapter>,
    pub browser_paths: Arc<dyn BrowserPathResolver>,
    pub startup: Arc<dyn StartupAdapter>,
}

pub fn create_platform_services() -> PlatformServices {
    #[cfg(target_os = "macos")]
    {
        PlatformServices {
            clipboard: Arc::new(MacClipboardAdapter),
            shortcuts: Arc::new(MacShortcutAdapter),
            window: Arc::new(MacWindowAdapter),
            app_context: Arc::new(MacAppContextAdapter),
            file_system: Arc::new(MacFileSystemAdapter),
            browser_paths: Arc::new(MacBrowserPathResolver),
            startup: Arc::new(MacStartupAdapter),
        }
    }

    #[cfg(target_os = "windows")]
    {
        PlatformServices {
            clipboard: Arc::new(WindowsClipboardAdapter),
            shortcuts: Arc::new(WindowsShortcutAdapter),
            window: Arc::new(WindowsWindowAdapter),
            app_context: Arc::new(WindowsAppContextAdapter),
            file_system: Arc::new(WindowsFileSystemAdapter),
            browser_paths: Arc::new(WindowsBrowserPathResolver),
            startup: Arc::new(WindowsStartupAdapter),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        PlatformServices {
            clipboard: Arc::new(UnsupportedClipboardAdapter),
            shortcuts: Arc::new(UnsupportedShortcutAdapter),
            window: Arc::new(UnsupportedWindowAdapter),
            app_context: Arc::new(UnsupportedAppContextAdapter),
            file_system: Arc::new(UnsupportedFileSystemAdapter),
            browser_paths: Arc::new(UnsupportedBrowserPathResolver),
            startup: Arc::new(UnsupportedStartupAdapter),
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedClipboardAdapter;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedShortcutAdapter;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedWindowAdapter;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedAppContextAdapter;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedFileSystemAdapter;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedBrowserPathResolver;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
struct UnsupportedStartupAdapter;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[async_trait]
impl ClipboardAdapter for UnsupportedClipboardAdapter {
    async fn read_text(
        &self,
        _app: &AppHandle,
    ) -> crate::errors::app_error::AppResult<Option<String>> {
        Ok(None)
    }

    async fn write_text(
        &self,
        _app: &AppHandle,
        _text: &str,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl ShortcutAdapter for UnsupportedShortcutAdapter {
    fn bindings(&self) -> Vec<ShortcutBinding> {
        Vec::new()
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[async_trait]
impl WindowAdapter for UnsupportedWindowAdapter {
    async fn ensure_widget(
        &self,
        _app: &AppHandle,
        _saved_position: Option<(f64, f64)>,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn set_widget_expanded(
        &self,
        _app: &AppHandle,
        _expanded: bool,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn open_main(&self, _app: &AppHandle) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn open_search_overlay(
        &self,
        _app: &AppHandle,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn open_quick_save(&self, _app: &AppHandle) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn close_window(
        &self,
        _window: &WebviewWindow,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }

    async fn open_memory_in_main(
        &self,
        _app: &AppHandle,
        _memory_id: String,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[async_trait]
impl AppContextAdapter for UnsupportedAppContextAdapter {
    async fn detect_context(&self) -> crate::errors::app_error::AppResult<AppContextSnapshot> {
        Ok(AppContextSnapshot {
            source_app: None,
            source_window: None,
        })
    }

    fn platform(&self) -> RuntimePlatform {
        RuntimePlatform::Linux
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[async_trait]
impl FileSystemAdapter for UnsupportedFileSystemAdapter {
    async fn choose_export_path(
        &self,
        _app: &AppHandle,
    ) -> crate::errors::app_error::AppResult<Option<std::path::PathBuf>> {
        Ok(None)
    }

    async fn choose_import_path(
        &self,
        _app: &AppHandle,
    ) -> crate::errors::app_error::AppResult<Option<std::path::PathBuf>> {
        Ok(None)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl BrowserPathResolver for UnsupportedBrowserPathResolver {
    fn resolve_bookmark_file(&self, _browser: BookmarkBrowser) -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[async_trait]
impl StartupAdapter for UnsupportedStartupAdapter {
    async fn apply_launch_on_startup(
        &self,
        _app: &AppHandle,
        _enabled: bool,
    ) -> crate::errors::app_error::AppResult<()> {
        Ok(())
    }
}
