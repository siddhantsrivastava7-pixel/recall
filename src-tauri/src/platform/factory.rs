use std::sync::Arc;

use crate::{
    models::RuntimePlatform,
    platform::{
        contracts::{
            AppContextAdapter, BrowserPathResolver, ClipboardAdapter, FileSystemAdapter,
            ShortcutAdapter, StartupAdapter, WindowAdapter,
        },
        mac::{
            MacAppContextAdapter, MacClipboardAdapter, MacFileSystemAdapter, MacShortcutAdapter,
            MacStartupAdapter, MacWindowAdapter, MacBrowserPathResolver,
        },
        windows::{
            WindowsAppContextAdapter, WindowsClipboardAdapter, WindowsFileSystemAdapter,
            WindowsShortcutAdapter, WindowsStartupAdapter, WindowsWindowAdapter,
            WindowsBrowserPathResolver,
        },
    },
};

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
    match current_platform() {
        RuntimePlatform::Macos => PlatformServices {
            clipboard: Arc::new(MacClipboardAdapter),
            shortcuts: Arc::new(MacShortcutAdapter),
            window: Arc::new(MacWindowAdapter),
            app_context: Arc::new(MacAppContextAdapter),
            file_system: Arc::new(MacFileSystemAdapter),
            browser_paths: Arc::new(MacBrowserPathResolver),
            startup: Arc::new(MacStartupAdapter),
        },
        _ => PlatformServices {
            clipboard: Arc::new(WindowsClipboardAdapter),
            shortcuts: Arc::new(WindowsShortcutAdapter),
            window: Arc::new(WindowsWindowAdapter),
            app_context: Arc::new(WindowsAppContextAdapter),
            file_system: Arc::new(WindowsFileSystemAdapter),
            browser_paths: Arc::new(WindowsBrowserPathResolver),
            startup: Arc::new(WindowsStartupAdapter),
        },
    }
}

fn current_platform() -> RuntimePlatform {
    #[cfg(target_os = "windows")]
    {
        RuntimePlatform::Windows
    }
    #[cfg(target_os = "macos")]
    {
        RuntimePlatform::Macos
    }
    #[cfg(target_os = "linux")]
    {
        RuntimePlatform::Linux
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        RuntimePlatform::Unknown
    }
}
