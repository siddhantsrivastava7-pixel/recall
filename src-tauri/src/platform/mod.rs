pub mod contracts;
pub mod factory;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod clipboard_image;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use clipboard_image::read_image_via_plugin;

/// v0.5.65 — cross-platform seam for Recall Pointer's auto-copy.
///
/// Windows: synthesize Ctrl+C (permission-free SendInput) so the
/// hotkey grabs the live selection. Returns `true` when input was
/// injected — the caller then waits briefly and reads the
/// clipboard.
///
/// macOS / other: no-op returning `false`. Synthetic key events
/// on macOS require the Accessibility permission + a request
/// flow; that's a deliberate v2+ item. On `false` the caller
/// falls back to the existing clipboard contents and the
/// "copy first" hint — exactly the v1 behavior, unchanged for
/// mac users.
#[cfg(target_os = "windows")]
pub(crate) fn try_synthesize_copy() -> bool {
    windows::synthesize_copy().is_ok()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn try_synthesize_copy() -> bool {
    false
}
