//! Image resize — cap image size and dimensions before sending to LLM.
//!
//! Anthropic charges `width * height / 750` tokens per image.
//! Capping at 2000×2000 ensures images never exceed ~5333 tokens.
//!
//! Strategy:
//!   1. Images within 2000×2000 and ≤5MB base64 pass through unchanged
//!   2. Oversized images get proportionally scaled to fit 2000×2000
//!   3. After resize, if base64 still >5MB: JPEG q60
//!   4. Still >5MB: resize to 1000px max + JPEG q60

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::ImageEncoder;

/// Maximum dimensions.
const MAX_DIM: u32 = 2000;
/// API hard limit for base64 image data (5 MB).
const MAX_BASE64: usize = 5 * 1024 * 1024;
/// Fallback max dimension when size still exceeds limit after JPEG q60.
const FALLBACK_DIM: u32 = 1000;
const JPEG_QUALITY: u8 = 60;

/// Encode a DynamicImage as JPEG bytes at the given quality.
fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder
        .write_image(
            img.as_bytes(),
            img.width(),
            img.height(),
            img.color().into(),
        )
        .map_err(|e| format!("encode jpeg: {e}"))?;
    Ok(buf)
}

/// Result: (base64_data, mime_type)
pub fn resize_image(data: &str, mime_type: &str) -> Result<(String, String), String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("base64 decode: {e}"))?;

    let mut img = image::load_from_memory(&decoded).map_err(|e| format!("decode image: {e}"))?;

    let (w, h) = (img.width(), img.height());
    let needs_resize = w > MAX_DIM || h > MAX_DIM;

    // Step 1: resize proportionally if dimensions exceed limit
    if needs_resize {
        let ratio = MAX_DIM as f64 / w.max(h) as f64;
        let new_w = (w as f64 * ratio) as u32;
        let new_h = (h as f64 * ratio) as u32;
        img = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
    }

    // Step 2: within both limits — pass through original unchanged
    if !needs_resize && data.len() <= MAX_BASE64 {
        return Ok((data.to_string(), mime_type.to_string()));
    }

    // Step 3: encode JPEG q60, check size
    let bytes = encode_jpeg(&img, JPEG_QUALITY)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    if encoded.len() <= MAX_BASE64 {
        return Ok((encoded, "image/jpeg".to_string()));
    }

    // Step 4: still too big — resize to FALLBACK_DIM max + JPEG q60
    let max_side = img.width().max(img.height());
    if max_side > FALLBACK_DIM {
        let ratio = FALLBACK_DIM as f64 / max_side as f64;
        let new_w = (img.width() as f64 * ratio) as u32;
        let new_h = (img.height() as f64 * ratio) as u32;
        img = img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
    }

    let bytes = encode_jpeg(&img, JPEG_QUALITY)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((encoded, "image/jpeg".to_string()))
}
