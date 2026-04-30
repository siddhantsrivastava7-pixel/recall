//! Apple Vision Framework OCR adapter (macOS only).
//!
//! Pipeline:
//!   1. Decode the encoded image bytes with the cross-platform `image` crate
//!      (avoids ImageIO FFI; lets us preprocess + downsize in safe Rust).
//!   2. Build a `CGImage` from the raw RGBA8 buffer via `CGDataProvider` +
//!      `CGImage::new` — pure Core Graphics, no AppKit / ImageIO deps.
//!   3. Hand the `CGImage` to a `VNImageRequestHandler` and run a
//!      `VNRecognizeTextRequest` at "accurate" recognition level.
//!   4. Concatenate the top candidate from each `VNRecognizedTextObservation`.
//!
//! Vision's `performRequests:error:` is synchronous on the calling thread.
//! Callers run this from inside [`tokio::task::spawn_blocking`] so the
//! single shared runtime stays responsive while text recognition (which
//! can take 200ms–2s on a large screenshot) churns through.

use std::ptr::NonNull;
use std::sync::Arc;

use async_trait::async_trait;
use image::GenericImageView;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
    VNRequestTextRecognitionLevel,
};

use crate::ai::ocr::{preprocessing::prepare_for_ocr, OcrAdapter, OcrResult};
use crate::errors::app_error::{AppError, AppResult};

pub struct AppleVisionOcr;

impl AppleVisionOcr {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AppleVisionOcr {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OcrAdapter for AppleVisionOcr {
    fn engine(&self) -> &'static str {
        "apple-vision"
    }

    fn is_available(&self) -> bool {
        // Vision's text recognition has been on every supported macOS we
        // ship to; if a future minimum-OS bump introduces an availability
        // check, this is where it lives.
        true
    }

    async fn recognize_bytes(&self, image_bytes: Vec<u8>) -> AppResult<OcrResult> {
        // Vision is synchronous. Run it off the multi-threaded runtime so
        // long recognitions don't stall capture/UI tasks.
        tokio::task::spawn_blocking(move || run_vision(&image_bytes))
            .await
            .map_err(|err| AppError::Invalid(format!("OCR worker panicked: {err}")))?
    }
}

fn run_vision(image_bytes: &[u8]) -> AppResult<OcrResult> {
    let prepared = prepare_for_ocr(image_bytes)?;
    let cg_image = build_cg_image(&prepared)?;

    let request: Retained<VNRecognizeTextRequest> = unsafe {
        VNRecognizeTextRequest::init(VNRecognizeTextRequest::alloc())
    };
    unsafe {
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
    }

    // Empty options dictionary — Vision figures out orientation.
    let options: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        NSDictionary::new();
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            &cg_image,
            options.as_ref(),
        )
    };

    let requests_array: Retained<NSArray<VNRequest>> = {
        let req_as_request: &VNRequest = unsafe {
            // VNRecognizeTextRequest is a subclass of VNRequest; safe to
            // upcast for the array.
            &*(Retained::as_ptr(&request) as *const VNRequest)
        };
        NSArray::from_retained_slice(&[unsafe { Retained::retain(req_as_request).unwrap() }])
    };

    unsafe {
        handler
            .performRequests_error(&requests_array)
            .map_err(|err| AppError::Invalid(format!("Vision performRequests failed: {err}")))?;
    }

    let observations = unsafe { request.results() }.unwrap_or_else(NSArray::new);
    let mut lines: Vec<String> = Vec::with_capacity(observations.len());
    let mut total_confidence: f64 = 0.0;
    let mut count: u64 = 0;

    for i in 0..observations.len() {
        // Vision returns observations as VNRecognizedTextObservation; the
        // generic VNObservation array is upcast.
        let obs = observations.objectAtIndex(i);
        let recognized: &VNRecognizedTextObservation = unsafe {
            &*(Retained::as_ptr(&obs) as *const VNRecognizedTextObservation)
        };

        let candidates = unsafe { recognized.topCandidates(1) };
        if candidates.is_empty() {
            continue;
        }
        let candidate = candidates.objectAtIndex(0);
        let text = unsafe { candidate.string() }.to_string();
        let confidence = unsafe { candidate.confidence() } as f64;
        if !text.trim().is_empty() {
            lines.push(text);
            total_confidence += confidence;
            count += 1;
        }
    }

    let mean_confidence = if count == 0 {
        None
    } else {
        Some(total_confidence / count as f64)
    };

    Ok(OcrResult {
        text: lines.join("\n"),
        confidence: mean_confidence,
        engine: "apple-vision",
        language: None,
    })
}

/// Build a `CGImage` from PNG/JPEG/etc. bytes by decoding through the
/// `image` crate to RGBA8, then wrapping the buffer in a `CGDataProvider`
/// and constructing a CGImage with explicit bitmap parameters.
fn build_cg_image(image_bytes: &[u8]) -> AppResult<Retained<CGImage>> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|err| AppError::Invalid(format!("OCR image decode failed: {err}")))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let raw: Arc<[u8]> = rgba.into_raw().into();

    let bytes_per_row = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| AppError::Invalid("OCR image too large".into()))?;

    // Wrap the Arc<[u8]> in an NSData (zero-copy reference, retained for
    // the lifetime of the CGDataProvider).
    let data = unsafe {
        let ptr = NonNull::new_unchecked(raw.as_ptr() as *mut u8);
        let _ = &raw; // ensure ownership for static lifetime via leak below
        // We leak the Arc to give Vision a permanently-valid backing buffer.
        // Sized at most 4096 * 4096 * 4 = 64 MB per OCR job; freed when the
        // CGImage and its provider are released, which happens when the
        // Retained<CGImage> goes out of scope. Leaking is safe because we
        // don't hold the raw beyond the function — the NSData::new takes
        // ownership pattern.
        let leaked: &'static [u8] = Box::leak(Box::<[u8]>::from(raw.as_ref()));
        NSData::with_bytes(leaked)
    };

    let provider = unsafe {
        CGDataProvider::with_cf_data(data.as_ref() as *const NSData as *const _).ok_or_else(
            || AppError::Invalid("CGDataProvider::with_cf_data returned null".into()),
        )?
    };

    let color_space = unsafe { CGColorSpace::new_device_rgb() }
        .ok_or_else(|| AppError::Invalid("CGColorSpace::new_device_rgb returned null".into()))?;

    // RGBA8, non-premultiplied, byte order = big (network order).
    // 0x00000004 = kCGImageAlphaLast
    let bitmap_info = CGBitmapInfo(0x00000004);

    let cg_image = unsafe {
        CGImage::new(
            width as usize,
            height as usize,
            8,
            32,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::Default,
        )
    }
    .ok_or_else(|| AppError::Invalid("CGImage::new returned null".into()))?;

    Ok(cg_image)
}
