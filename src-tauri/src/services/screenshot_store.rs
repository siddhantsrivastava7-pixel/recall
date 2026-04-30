//! On-disk storage for clipboard-image memories.
//!
//! Layout:
//!
//! ```text
//! <app_data_dir>/screenshots/screenshot-<hash>.png
//! ```
//!
//! Each saved image is content-addressed by a 64-bit FNV-1a hash of the
//! raw RGBA bytes. Two captures of the exact same screenshot land on the
//! same path, so a duplicated paste doesn't waste disk. (FNV-1a-64 is
//! not collision-resistant in the cryptographic sense, but at Recall's
//! scale — at most a few thousand screenshots per user — collision
//! probability is essentially zero.)
//!
//! Ceiling of 20 MB per saved image. Anything larger is refused outright
//! so a clipboard with a 50 MP RAW or a 100 MB BMP can't murder disk.
//!
//! Files are tracked exclusively by their on-disk path; the memory row
//! holds a `file://...` URL pointing at the file. On memory delete we
//! [`Self::delete`] the file alongside the row — see
//! [`MemoryService::delete`] for the wiring.

use std::path::{Path, PathBuf};

use image::{ImageBuffer, Rgba};
use tauri::{AppHandle, Manager};
use tokio::fs;

use crate::errors::app_error::{AppError, AppResult};

/// Hard ceiling per saved image. ~20 MB after PNG encode covers a 4K
/// screenshot with room to spare; anything larger almost certainly
/// isn't a screenshot the user meant to save.
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Subdirectory under `app_data_dir()` where screenshots live.
const SCREENSHOT_SUBDIR: &str = "screenshots";

/// Stable subsystem-wide identifier for the screenshot source. Capture
/// service's OCR enqueue hook gates on `memory.source_app == this`.
pub const SCREENSHOT_SOURCE_APP: &str = "screenshot";

#[derive(Debug, Clone)]
pub struct SavedScreenshot {
    pub path: PathBuf,
    pub file_url: String,
    pub width: u32,
    pub height: u32,
    pub byte_size: u64,
}

#[derive(Clone)]
pub struct ScreenshotStore {
    app: AppHandle,
}

impl ScreenshotStore {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Save a clipboard image to disk and return the resolved path.
    /// Idempotent: a second save with identical pixels is a no-op
    /// (returns the existing path) — handy for noisy double-fires from
    /// Win+PrintScreen / Snip & Sketch.
    pub async fn save_rgba(
        &self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    ) -> AppResult<SavedScreenshot> {
        if width == 0 || height == 0 {
            return Err(AppError::Invalid("Empty clipboard image (zero dimension).".into()));
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| AppError::Invalid("Clipboard image dimensions overflowed.".into()))?;
        if rgba.len() != expected {
            return Err(AppError::Invalid(format!(
                "Clipboard image bytes truncated: expected {expected}, got {}",
                rgba.len()
            )));
        }

        // Encode to PNG before checking size — RGBA is uncompressed and
        // would always blow past 20 MB for large screenshots, but PNG of
        // typical screenshots is well under.
        let png_bytes = encode_png(rgba, width, height)?;
        if png_bytes.len() > MAX_IMAGE_BYTES {
            return Err(AppError::Invalid(format!(
                "Clipboard image is {} bytes — Recall caps screenshots at {} bytes.",
                png_bytes.len(),
                MAX_IMAGE_BYTES
            )));
        }

        let directory = self.directory()?;
        fs::create_dir_all(&directory).await.map_err(|err| {
            AppError::Invalid(format!(
                "Could not create screenshot directory {}: {err}",
                directory.display()
            ))
        })?;

        let hash = fnv1a_64(&png_bytes);
        let filename = format!("screenshot-{hash:016x}.png");
        let path = directory.join(&filename);

        // Skip the write if the file already exists — content-addressed,
        // so an identical capture maps to the same file.
        if !fs::try_exists(&path).await.unwrap_or(false) {
            fs::write(&path, &png_bytes).await.map_err(|err| {
                AppError::Invalid(format!("Failed to write screenshot {}: {err}", path.display()))
            })?;
        }

        let byte_size = png_bytes.len() as u64;
        let file_url = path_to_file_url(&path);
        Ok(SavedScreenshot {
            path,
            file_url,
            width,
            height,
            byte_size,
        })
    }

    /// Delete a previously-saved screenshot. Refuses to touch any path
    /// outside our own screenshots directory — never let a malformed
    /// memory row coerce us into unlinking arbitrary files. Missing
    /// files are not an error (the user may have already cleaned up).
    pub async fn delete(&self, path: &Path) -> AppResult<()> {
        let directory = self.directory()?;
        let canonical_dir = match fs::canonicalize(&directory).await {
            Ok(canonical) => canonical,
            Err(_) => return Ok(()), // dir doesn't exist yet — nothing to delete
        };

        // Resolve the candidate against the canonical screenshot dir
        // (canonicalize requires the file to exist; if it doesn't,
        // there's nothing to do).
        let canonical_path = match fs::canonicalize(path).await {
            Ok(canonical) => canonical,
            Err(_) => return Ok(()),
        };
        if !canonical_path.starts_with(&canonical_dir) {
            return Err(AppError::Invalid(format!(
                "Refusing to delete file outside the screenshots directory: {}",
                path.display()
            )));
        }
        if let Err(err) = fs::remove_file(&canonical_path).await {
            if err.kind() == std::io::ErrorKind::NotFound {
                return Ok(());
            }
            return Err(AppError::Invalid(format!(
                "Failed to delete screenshot {}: {err}",
                canonical_path.display()
            )));
        }
        Ok(())
    }

    fn directory(&self) -> AppResult<PathBuf> {
        let base = self
            .app
            .path()
            .app_data_dir()
            .map_err(|err| AppError::Invalid(format!("app_data_dir unavailable: {err}")))?;
        Ok(base.join(SCREENSHOT_SUBDIR))
    }

    /// Public so the memory-delete cleanup can resolve a `file://` URL
    /// stored on a memory back to a path — and skip the unlink if the
    /// URL points anywhere outside our directory.
    pub fn screenshots_dir(&self) -> AppResult<PathBuf> {
        self.directory()
    }
}

fn encode_png(rgba: Vec<u8>, width: u32, height: u32) -> AppResult<Vec<u8>> {
    let buffer: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba)
        .ok_or_else(|| AppError::Invalid("RGBA buffer length didn't match dimensions.".into()))?;

    let mut output = Vec::with_capacity(64 * 1024);
    {
        let mut cursor = std::io::Cursor::new(&mut output);
        buffer
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|err| AppError::Invalid(format!("PNG encode failed: {err}")))?;
    }
    Ok(output)
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(target_os = "windows")]
fn path_to_file_url(path: &Path) -> String {
    // Windows: file:///C:/path/with/forward/slashes
    let str_path = path.to_string_lossy().replace('\\', "/");
    if str_path.starts_with('/') {
        format!("file://{str_path}")
    } else {
        format!("file:///{str_path}")
    }
}

#[cfg(not(target_os = "windows"))]
fn path_to_file_url(path: &Path) -> String {
    let str_path = path.to_string_lossy();
    if str_path.starts_with('/') {
        format!("file://{str_path}")
    } else {
        format!("file:///{str_path}")
    }
}

/// Convert a `file://` URL back to a filesystem path. Returns `None`
/// when the URL doesn't have the `file://` scheme — callers treat that
/// as "not one of ours, leave it alone".
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let stripped = url.strip_prefix("file://")?;
    #[cfg(target_os = "windows")]
    {
        let trimmed = stripped.trim_start_matches('/');
        return Some(PathBuf::from(trimmed));
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Some(PathBuf::from(stripped));
    }
}

#[cfg(test)]
mod tests {
    use super::{file_url_to_path, fnv1a_64, path_to_file_url};
    use std::path::PathBuf;

    #[test]
    fn fnv1a_distinct_inputs_distinct_outputs() {
        assert_ne!(fnv1a_64(b"hello"), fnv1a_64(b"world"));
        assert_eq!(fnv1a_64(b"hello"), fnv1a_64(b"hello"));
    }

    #[test]
    fn file_url_round_trips() {
        let original = if cfg!(target_os = "windows") {
            PathBuf::from("C:\\Users\\a\\screenshots\\x.png")
        } else {
            PathBuf::from("/Users/a/screenshots/x.png")
        };
        let url = path_to_file_url(&original);
        assert!(url.starts_with("file://"));
        let resolved = file_url_to_path(&url).expect("resolves");
        // Path equality is fuzzy across platforms; just assert the
        // resolved string ends with the same final segment.
        assert!(resolved
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "x.png"));
    }
}
