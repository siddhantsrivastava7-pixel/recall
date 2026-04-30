//! Windows.Media.Ocr OCR adapter (Windows only).
//!
//! Uses the system `OcrEngine` resolved from the user's profile languages.
//! All work runs on a blocking pool because the WinRT projection uses
//! `IAsyncOperation::get`, which blocks the calling thread.
//!
//! Pipeline:
//!   1. Decode + downsize via the `image` crate (cross-platform path).
//!   2. Re-encode to PNG and feed into `InMemoryRandomAccessStream`.
//!   3. `BitmapDecoder::CreateAsync` → `SoftwareBitmap`.
//!   4. `OcrEngine::RecognizeAsync` → `OcrResult` with per-line text.
//!   5. Concatenate `Lines()` into a single `\n`-joined string.
//!
//! `OcrEngine::TryCreateFromUserProfileLanguages()` returns null when no
//! recognizer is installed for any of the user's preferred languages — the
//! adapter reports unavailable in that case so the scheduler skips OCR
//! cleanly without raising errors.

use std::sync::OnceLock;

use async_trait::async_trait;
use windows::core::Result as WinResult;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

use crate::ai::ocr::{preprocessing::prepare_for_ocr, OcrAdapter, OcrResult};
use crate::errors::app_error::{AppError, AppResult};

pub struct WindowsMediaOcr;

impl WindowsMediaOcr {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsMediaOcr {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached availability probe — `OcrEngine::TryCreateFromUserProfileLanguages`
/// is cheap but not free, and the result doesn't change for the lifetime of
/// the process under any realistic scenario.
fn probe_engine_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        // Returning a successful but null engine reads as "no language
        // packs available". Both branches map to "unavailable".
        match OcrEngine::TryCreateFromUserProfileLanguages() {
            Ok(_engine) => true,
            Err(_) => false,
        }
    })
}

#[async_trait]
impl OcrAdapter for WindowsMediaOcr {
    fn engine(&self) -> &'static str {
        "windows-media-ocr"
    }

    fn is_available(&self) -> bool {
        probe_engine_available()
    }

    async fn recognize_bytes(&self, image_bytes: Vec<u8>) -> AppResult<OcrResult> {
        // Image preprocessing is CPU-bound; offload to the blocking pool.
        // Each spawn_blocking handle is a thread the runtime can schedule
        // on, so we don't pin the OCR worker to a specific thread.
        let prepared = tokio::task::spawn_blocking(move || prepare_for_ocr(&image_bytes))
            .await
            .map_err(|err| AppError::Invalid(format!("OCR preprocess panicked: {err}")))??;

        // WinRT IAsyncOperation implements `Future` in `windows` 0.62+, so
        // we await it directly rather than calling `.get()` (which was
        // removed when async support landed). Each await yields to the
        // tokio runtime — no thread is blocked while the OCR engine runs.
        let text = recognize_via_winocr(&prepared)
            .await
            .map_err(|err| AppError::Invalid(format!("Windows OCR failed: {err}")))?;

        Ok(OcrResult {
            text,
            confidence: None, // Windows.Media.Ocr doesn't surface a confidence score
            engine: "windows-media-ocr",
            language: None,
        })
    }
}

async fn recognize_via_winocr(image_bytes: &[u8]) -> WinResult<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;

    // Stage the image bytes into an InMemoryRandomAccessStream that
    // BitmapDecoder can consume.
    let stream = InMemoryRandomAccessStream::new()?;
    {
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(image_bytes)?;
        writer.StoreAsync()?.await?;
        writer.FlushAsync()?.await?;
        // Releasing the writer detaches it from the stream so we can
        // re-read from offset 0 below.
        writer.DetachStream()?;
    }
    stream.Seek(0)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?.await?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.await?;

    let result = engine.RecognizeAsync(&bitmap)?.await?;
    let lines = result.Lines()?;
    let count = lines.Size()? as usize;

    let mut parts = Vec::with_capacity(count);
    for i in 0..count {
        let line = lines.GetAt(i as u32)?;
        let text = line.Text()?.to_string();
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }

    Ok(parts.join("\n"))
}
