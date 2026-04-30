//! OCR adapters.
//!
//! v0.2.0 ships native engines only:
//!   * **macOS**: Apple Vision Framework via [`mac_vision`]
//!   * **Windows**: `Windows.Media.Ocr` via [`windows_winocr`]
//!
//! Tesseract / cloud fallbacks are deliberately excluded — see the locked
//! v0.2.0 PRD in the project plan file.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::app_error::AppResult;

pub mod preprocessing;

#[cfg(target_os = "macos")]
pub mod mac_vision;
#[cfg(target_os = "windows")]
pub mod windows_winocr;

/// Result of an OCR run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    /// Recognized text. Lines are joined with `\n`. Empty if Vision/WinOCR
    /// produced no observations — that's a *successful* OCR with empty
    /// output, distinct from a failure.
    pub text: String,
    /// Average confidence across recognized regions, in `[0.0, 1.0]`. None
    /// when the engine doesn't expose a numeric confidence.
    pub confidence: Option<f64>,
    /// Stable engine identifier, persisted on `memories.ocr_engine` for
    /// dedupe and "force re-OCR with a different engine" flows.
    pub engine: &'static str,
    /// BCP-47 language tag the engine resolved to, when available.
    pub language: Option<String>,
}

/// OCR engine abstraction. Adapters take encoded image bytes (PNG / JPEG /
/// WebP / BMP / GIF — anything `image::load_from_memory` can decode) and
/// return recognized text. Adapters are responsible for any platform-side
/// preprocessing; cross-platform helpers live in [`preprocessing`].
#[async_trait]
pub trait OcrAdapter: Send + Sync {
    /// Stable identifier — `"apple-vision"`, `"windows-media-ocr"`, etc.
    fn engine(&self) -> &'static str;

    /// Whether this adapter can run on the current host. Adapters that
    /// require a minimum OS version return `false` on older systems so the
    /// scheduler can skip OCR cleanly without raising errors.
    fn is_available(&self) -> bool {
        true
    }

    /// Run OCR against the provided encoded-image bytes.
    async fn recognize_bytes(&self, image_bytes: Vec<u8>) -> AppResult<OcrResult>;
}

/// Construct the platform-default adapter. Returns `None` when no native
/// engine is available on this host (e.g. an unsupported OS version) — the
/// caller treats this as "OCR not available, skip silently".
pub fn default_adapter() -> Option<std::sync::Arc<dyn OcrAdapter>> {
    #[cfg(target_os = "macos")]
    {
        let adapter = std::sync::Arc::new(mac_vision::AppleVisionOcr::new());
        if adapter.is_available() {
            return Some(adapter);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let adapter = std::sync::Arc::new(windows_winocr::WindowsMediaOcr::new());
        if adapter.is_available() {
            return Some(adapter);
        }
    }
    None
}

/// Stable engine label exposed in Settings and persisted on memories.
pub fn engine_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "apple-vision";
    }
    #[cfg(target_os = "windows")]
    {
        return "windows-media-ocr";
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "unsupported"
    }
}
