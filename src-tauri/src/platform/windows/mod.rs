use async_trait::async_trait;
use std::process::Command;
use tokio::fs;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

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

pub struct WindowsClipboardAdapter;
pub struct WindowsShortcutAdapter;
pub struct WindowsWindowAdapter;
pub struct WindowsAppContextAdapter;
pub struct WindowsFileSystemAdapter;
pub struct WindowsBrowserPathResolver;
pub struct WindowsBrowserBookmarkReader;
pub struct WindowsStartupAdapter;

const WIDGET_COLLAPSED_WIDTH: f64 = 260.0;
const WIDGET_COLLAPSED_HEIGHT: f64 = 56.0;
const WIDGET_EXPANDED_WIDTH: f64 = 260.0;
const WIDGET_EXPANDED_HEIGHT: f64 = 56.0;
const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const WINDOWS_STARTUP_VALUE_NAME: &str = "Recall";

#[async_trait]
impl ClipboardAdapter for WindowsClipboardAdapter {
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

impl ShortcutAdapter for WindowsShortcutAdapter {
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

fn resize_widget_window(window: &WebviewWindow, expanded: bool) -> AppResult<()> {
    let current_position = window.outer_position()?;
    let current_size = window.outer_size()?;
    let target_width = if expanded {
        WIDGET_EXPANDED_WIDTH
    } else {
        WIDGET_COLLAPSED_WIDTH
    };
    let target_height = if expanded {
        WIDGET_EXPANDED_HEIGHT
    } else {
        WIDGET_COLLAPSED_HEIGHT
    };

    let delta_width = target_width - f64::from(current_size.width);
    let next_x = f64::from(current_position.x) - (delta_width / 2.0);
    let next_y = f64::from(current_position.y);

    window.set_size(Size::Logical(LogicalSize::new(target_width, target_height)))?;
    window.set_position(Position::Logical(LogicalPosition::new(next_x, next_y)))?;

    Ok(())
}

fn build_overlay_window(app: &AppHandle) -> AppResult<()> {
    let window =
        WebviewWindowBuilder::new(app, "search-overlay", WebviewUrl::App("index.html".into()))
            .title("Recall Search")
            .inner_size(860.0, 640.0)
            .resizable(false)
            .visible(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .build()?;
    window.set_shadow(false)?;
    window.center()?;
    Ok(())
}

fn build_quick_save_window(app: &AppHandle) -> AppResult<()> {
    let window = WebviewWindowBuilder::new(app, "quick-save", WebviewUrl::App("index.html".into()))
        .title("Recall Quick Save")
        .inner_size(780.0, 620.0)
        .resizable(false)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()?;
    window.set_shadow(false)?;
    window.center()?;
    Ok(())
}

#[async_trait]
impl WindowAdapter for WindowsWindowAdapter {
    async fn ensure_widget(
        &self,
        app: &AppHandle,
        saved_position: Option<(f64, f64)>,
    ) -> AppResult<()> {
        let window = if let Some(w) = app.get_webview_window("widget") {
            w
        } else {
            // Dynamically create if not pre-defined in config
            let w = WebviewWindowBuilder::new(app, "widget", WebviewUrl::App("index.html".into()))
                .title("Recall Widget")
                .inner_size(WIDGET_COLLAPSED_WIDTH, WIDGET_COLLAPSED_HEIGHT)
                .resizable(false)
                .visible(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .build()?;
            w.set_shadow(false)?;
            w
        };

        // Restore saved position, or default to bottom-center of primary monitor
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

        resize_widget_window(&window, false)?;
        let _ = window.set_shadow(false);
        window.show()?;
        window.set_always_on_top(true)?;
        Ok(())
    }

    async fn set_widget_expanded(&self, app: &AppHandle, expanded: bool) -> AppResult<()> {
        if let Some(window) = app.get_webview_window("widget") {
            resize_widget_window(&window, expanded)?;
        }
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
        if app.get_webview_window("search-overlay").is_none() {
            build_overlay_window(app)?;
        }
        if let Some(window) = app.get_webview_window("search-overlay") {
            window.show()?;
            window.set_focus()?;
            window.center()?;
        }
        Ok(())
    }

    async fn open_quick_save(&self, app: &AppHandle) -> AppResult<()> {
        if app.get_webview_window("quick-save").is_none() {
            build_quick_save_window(app)?;
        }
        if let Some(window) = app.get_webview_window("quick-save") {
            window.show()?;
            window.set_focus()?;
            window.center()?;
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

#[cfg(target_os = "windows")]
fn active_window_snapshot() -> AppContextSnapshot {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, MAX_PATH},
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
            },
        },
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return AppContextSnapshot {
                source_app: None,
                source_window: None,
            };
        }

        let mut title_buffer = vec![0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title_buffer);
        let source_window = if title_len > 0 {
            Some(String::from_utf16_lossy(
                &title_buffer[..title_len as usize],
            ))
        } else {
            None
        };

        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        let source_app = if process_id == 0 {
            None
        } else {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id);
            if let Ok(process) = process {
                let mut buffer = vec![0u16; MAX_PATH as usize];
                let mut size = buffer.len() as u32;
                let ok = QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_FORMAT(0),
                    PWSTR(buffer.as_mut_ptr()),
                    &mut size,
                )
                .is_ok();
                let _ = CloseHandle(process);
                if ok {
                    let path = String::from_utf16_lossy(&buffer[..size as usize]);
                    std::path::Path::new(&path)
                        .file_stem()
                        .map(|name| name.to_string_lossy().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        };

        AppContextSnapshot {
            source_app,
            source_window,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn active_window_snapshot() -> AppContextSnapshot {
    AppContextSnapshot {
        source_app: None,
        source_window: None,
    }
}

#[async_trait]
impl AppContextAdapter for WindowsAppContextAdapter {
    async fn detect_context(&self) -> AppResult<AppContextSnapshot> {
        Ok(active_window_snapshot())
    }

    fn platform(&self) -> RuntimePlatform {
        RuntimePlatform::Windows
    }
}

#[async_trait]
impl FileSystemAdapter for WindowsFileSystemAdapter {
    async fn choose_export_path(&self, app: &AppHandle) -> AppResult<Option<std::path::PathBuf>> {
        Ok(app
            .dialog()
            .file()
            .add_filter("Recall Backup", &["json"])
            .set_file_name("recall-backup.json")
            .blocking_save_file()
            .and_then(|file_path| file_path.into_path().ok()))
    }

    async fn choose_import_path(&self, app: &AppHandle) -> AppResult<Option<std::path::PathBuf>> {
        Ok(app
            .dialog()
            .file()
            .add_filter("Recall Backup", &["json"])
            .blocking_pick_file()
            .and_then(|file_path| file_path.into_path().ok()))
    }
}

impl BrowserPathResolver for WindowsBrowserPathResolver {
    fn resolve_bookmark_file(&self, browser: BookmarkBrowser) -> Option<std::path::PathBuf> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")?;
        let base = std::path::PathBuf::from(local_app_data);

        let path = match browser {
            BookmarkBrowser::Chrome => base.join("Google/Chrome/User Data/Default/Bookmarks"),
            BookmarkBrowser::Edge => base.join("Microsoft/Edge/User Data/Default/Bookmarks"),
            BookmarkBrowser::Brave => {
                base.join("BraveSoftware/Brave-Browser/User Data/Default/Bookmarks")
            }
            BookmarkBrowser::Safari => return None,
        };

        Some(path)
    }
}

#[async_trait]
impl BrowserBookmarkReader for WindowsBrowserBookmarkReader {
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
            BookmarkBrowser::Safari => Ok(Vec::new()),
        }
    }
}

fn startup_command_value() -> AppResult<String> {
    let exe_path = std::env::current_exe()?;
    Ok(format!("\"{}\"", exe_path.display()))
}

fn run_reg_command(args: &[&str]) -> AppResult<std::process::Output> {
    Command::new("reg").args(args).output().map_err(Into::into)
}

#[async_trait]
impl StartupAdapter for WindowsStartupAdapter {
    async fn apply_launch_on_startup(&self, _app: &AppHandle, enabled: bool) -> AppResult<()> {
        if enabled {
            let command_value = startup_command_value()?;
            let output = run_reg_command(&[
                "add",
                WINDOWS_RUN_KEY,
                "/v",
                WINDOWS_STARTUP_VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &command_value,
                "/f",
            ])?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(crate::errors::app_error::AppError::Invalid(format!(
                    "failed to enable launch on startup: {stderr}"
                )));
            }
        } else {
            let output = run_reg_command(&[
                "delete",
                WINDOWS_RUN_KEY,
                "/v",
                WINDOWS_STARTUP_VALUE_NAME,
                "/f",
            ])?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
                if !stderr.contains("unable to find")
                    && !stderr.contains("unable to find the specified registry key or value")
                {
                    return Err(crate::errors::app_error::AppError::Invalid(format!(
                        "failed to disable launch on startup: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }
            }
        }

        Ok(())
    }
}
