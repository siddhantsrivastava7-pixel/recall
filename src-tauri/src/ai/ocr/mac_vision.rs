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

use async_trait::async_trait;
use image::GenericImageView;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
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

    // VNRecognizeTextRequest is a subclass of VNRequest; clone-retain the
    // pointer at the parent type so we can stuff it into `NSArray<VNRequest>`.
    // `Retained::retain` increments the refcount and returns a fresh
    // Retained<VNRequest>; the original `request` owner is unaffected.
    let request_as_vn: Retained<VNRequest> = unsafe {
        Retained::retain(Retained::as_ptr(&request) as *mut VNRequest)
            .ok_or_else(|| AppError::Invalid("VNRequest retain returned null".into()))?
    };
    let requests_array: Retained<NSArray<VNRequest>> =
        NSArray::from_retained_slice(&[request_as_vn]);

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
///
/// Returns `CFRetained<CGImage>` rather than `Retained<CGImage>` because
/// CGImage is a Core Foundation type — its retain count is managed via
/// `CFRetain`/`CFRelease`, not the Objective-C runtime. The two smart
/// pointers are not interchangeable and forcing `Retained` here is a
/// type error.
fn build_cg_image(image_bytes: &[u8]) -> AppResult<CFRetained<CGImage>> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|err| AppError::Invalid(format!("OCR image decode failed: {err}")))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let raw: Vec<u8> = rgba.into_raw();

    let bytes_per_row = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| AppError::Invalid("OCR image too large".into()))?;

    // Copy the pixel buffer into a CFData. The CFData owns the bytes and
    // outlives the CGImage that wraps it (CGDataProvider takes a strong
    // reference). At most ~64 MB for a 4096×4096 OCR-prepped image —
    // we accept the one extra allocation per recognition since Vision
    // itself is the speed bottleneck.
    //
    // `CFData::new` is a thin shim over `CFDataCreate`: 3 args
    // (allocator, *const u8, CFIndex) — no `&[u8]` sugar yet in
    // objc2-core-foundation 0.3, so we hand it the raw pointer + length.
    let raw_len = raw.len() as isize; // CFIndex is isize on 64-bit Apple
    let cf_data: CFRetained<CFData> =
        unsafe { CFData::new(None, raw.as_ptr(), raw_len) }
            .ok_or_else(|| AppError::Invalid("CFData::new returned null".into()))?;

    let provider = unsafe { CGDataProvider::with_cf_data(Some(&cf_data)) }
        .ok_or_else(|| AppError::Invalid("CGDataProvider::with_cf_data returned null".into()))?;

    let color_space = unsafe { CGColorSpace::new_device_rgb() }
        .ok_or_else(|| AppError::Invalid("CGColorSpace::new_device_rgb returned null".into()))?;

    // RGBA8, non-premultiplied, byte order = big (network order).
    // 0x00000004 = kCGImageAlphaLast
    let bitmap_info = CGBitmapInfo(0x00000004);

    // `kCGRenderingIntentDefault = 0`. The associated-constant name of
    // this variant differs across objc2-core-graphics revisions, but
    // the underlying ABI is stable; constructing the wrapper struct
    // directly with the documented value is portable.
    let rendering_intent = CGColorRenderingIntent(0);

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
            rendering_intent,
        )
    }
    .ok_or_else(|| AppError::Invalid("CGImage::new returned null".into()))?;

    Ok(cg_image)
}
