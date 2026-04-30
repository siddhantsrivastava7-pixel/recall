//! Cross-platform helper that pulls the current clipboard image via
//! `tauri-plugin-clipboard-manager` and normalizes the bytes into the
//! `ClipboardImage` shape the watcher consumes.
//!
//! Lives outside the per-OS modules because both macOS and Windows
//! delegate to the same plugin call — only the trait wiring differs.

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::platform::contracts::ClipboardImage;

/// Read the current clipboard image, if any. The plugin returns an
/// error both when the clipboard truly has no image *and* when the host
/// rejected the read; we collapse both to `None` so the caller can do a
/// single boolean branch. The watcher polls every ~900ms — a noisy
/// "no image" is the common case here, not an actual problem.
pub(crate) fn read_image_via_plugin(app: &AppHandle) -> Option<ClipboardImage> {
    let image = app.clipboard().read_image().ok()?;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }
    let rgba = image.rgba().to_vec();
    // Sanity check: defensively bail if the buffer length doesn't match
    // width × height × 4. Better to drop one capture than feed truncated
    // bytes into the OCR pipeline downstream.
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if rgba.len() != expected {
        return None;
    }
    Some(ClipboardImage {
        rgba,
        width,
        height,
    })
}
