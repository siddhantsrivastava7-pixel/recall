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
