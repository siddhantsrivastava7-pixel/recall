//! Cross-platform image preprocessing for OCR.
//!
//! Phase 1 keeps this intentionally light: native Vision and Windows.Media.Ocr
//! both handle full-resolution screenshots well, and aggressive preprocessing
//! tends to hurt rather than help for those engines. We do two things only:
//!
//! 1. **Decode** the bytes via [`image::load_from_memory`] — proves they're
//!    a real image before we hand off to an FFI engine. A bad decode here
//!    fails fast with a clean error instead of a Vision crash.
//! 2. **Cap dimensions** at 4096 px on the longest edge. Phone-camera
//!    screenshots in 2026 routinely run 4032×3024+ which both Vision and
//!    Windows OCR will accept but takes seconds longer than necessary;
//!    downsizing keeps OCR latency bounded.
//!
//! Heavier ops (deskew, contrast normalization) belong in a Phase-2-or-later
//! "OCR quality boost" path opt-in via setting; not here.

use image::{DynamicImage, ImageFormat};

use crate::errors::app_error::{AppError, AppResult};

const MAX_LONG_EDGE: u32 = 4096;

/// Decode raw image bytes, optionally downsize, and re-encode as PNG. PNG
/// because it's lossless (we don't want to JPEG-compress text we're about
/// to OCR) and because both Vision and Windows.Media.Ocr decode it
/// natively without surprises.
pub fn prepare_for_ocr(image_bytes: &[u8]) -> AppResult<Vec<u8>> {
    let img = image::load_from_memory(image_bytes)
        .map_err(|err| AppError::Invalid(format!("OCR image decode failed: {err}")))?;

    let img = clamp_long_edge(img, MAX_LONG_EDGE);

    let mut out = Vec::with_capacity(image_bytes.len());
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|err| AppError::Invalid(format!("OCR image re-encode failed: {err}")))?;
    Ok(out)
}

fn clamp_long_edge(image: DynamicImage, max: u32) -> DynamicImage {
    let (w, h) = (image.width(), image.height());
    let long_edge = w.max(h);
    if long_edge <= max {
        return image;
    }
    let scale = max as f32 / long_edge as f32;
    let new_w = ((w as f32 * scale).round() as u32).max(1);
    let new_h = ((h as f32 * scale).round() as u32).max(1);
    image.resize(new_w, new_h, image::imageops::FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        // 2x2 red PNG, hand-encoded for stability.
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn prepare_passes_small_images_unchanged_in_size_class() {
        let bytes = tiny_png();
        let prepared = prepare_for_ocr(&bytes).unwrap();
        let img = image::load_from_memory(&prepared).unwrap();
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn prepare_clamps_oversized_images() {
        let img = image::RgbaImage::from_pixel(8000, 4000, image::Rgba([0, 0, 0, 255]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();

        let prepared = prepare_for_ocr(&bytes).unwrap();
        let img = image::load_from_memory(&prepared).unwrap();
        assert!(img.width().max(img.height()) <= MAX_LONG_EDGE);
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[test]
    fn prepare_rejects_non_image_bytes() {
        let result = prepare_for_ocr(b"this is not an image");
        assert!(result.is_err());
    }
}
