//! Decoding page images (JPEG, PNG, WebP, GIF) into grayscale.
//!
//! Behind the `decode` feature: the desktop app uses it; the website decodes
//! with the browser instead and hands raw pixels to the pipeline.

use crate::gray::GrayImage;
use image::ImageEncoder;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("could not decode image: {0}")]
    Image(#[from] image::ImageError),
    #[error("could not read image data: {0}")]
    Io(#[from] std::io::Error),
    #[error("image is {width}x{height}, which is larger than the {limit} megapixel limit")]
    TooLarge { width: u32, height: u32, limit: u32 },
}

/// Refuse absurd images (decompression bombs) before allocating for them.
pub const MAX_MEGAPIXELS: u32 = 120;

/// Decode to 8-bit luma. Transparent areas are composited onto white first,
/// since a transparent background would otherwise read as black.
pub fn decode_to_gray(bytes: &[u8]) -> Result<GrayImage, DecodeError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let (w, h) = reader.into_dimensions()?;
    if (w as u64 * h as u64) > MAX_MEGAPIXELS as u64 * 1_000_000 {
        return Err(DecodeError::TooLarge {
            width: w,
            height: h,
            limit: MAX_MEGAPIXELS,
        });
    }
    let img = image::load_from_memory(bytes)?;
    let (width, height) = (img.width(), img.height());
    let pixels = match img {
        image::DynamicImage::ImageRgb8(rgb) => {
            let lookup = opaque_luma_lookup();
            rgb.as_raw()
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| lookup[((p[0] as usize) << 16) | ((p[1] as usize) << 8) | p[2] as usize])
                .collect()
        }
        image::DynamicImage::ImageLuma8(gray) => gray.into_raw(),
        img => rgba_luma(img),
    };
    Ok(GrayImage::from_pixels(width, height, pixels).expect("sizes match"))
}

/// All opaque 8-bit colors share one 16 MiB table. Building it once avoids
/// repeated float conversion on billions of downloaded RGB pixels, and
/// avoids expanding opaque images into a second four-channel buffer.
///
/// Not replaced by integer arithmetic: the table is rounded from f32 sums, and
/// every fixed-point weighting tried (8, 15, 16 and 20 bits) differs from it by
/// one level for thousands of colors, which would change cached pages.
fn opaque_luma_lookup() -> &'static [u8] {
    static LOOKUP: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut table = Vec::with_capacity(1 << 24);
        for r in 0..256 {
            for g in 0..256 {
                let rg = 0.299 * r as f32 + 0.587 * g as f32;
                for b in 0..256 {
                    table.push((rg + 0.114 * b as f32).round() as u8);
                }
            }
        }
        table
    })
}

fn rgba_luma(img: image::DynamicImage) -> Vec<u8> {
    let rgba = img.to_rgba8();
    rgba.pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            let alpha = a as f32 / 255.0;
            (luma * alpha + 255.0 * (1.0 - alpha)).round() as u8
        })
        .collect()
}

/// Pixel size of an image, read from its header without decoding it.
pub fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32), DecodeError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    Ok(reader.into_dimensions()?)
}

/// Tallest page we keep when the user chooses to save space. Twice the
/// reader's 800-pixel screen height, so nothing visible is lost.
pub const STORAGE_MAX_HEIGHT: u32 = 1600;

/// Narrowest a page is ever made when saving space: twice the reader's
/// 480-pixel width. Without this floor a very tall webtoon strip, shrunk to
/// the height limit, ends up only a hundred pixels wide and is stretched back
/// up (blurry) when it is shown.
pub const STORAGE_MIN_WIDTH: u32 = 960;

/// Re-encode a page smaller for storage, or `None` if it is already small
/// enough (so the original bytes should be kept untouched).
///
/// Portrait pages are limited to `max_height` tall, but never made narrower
/// than [`STORAGE_MIN_WIDTH`], so a long strip is kept at a width the screen
/// can use (and left alone if it is already narrower). Landscape double-page
/// spreads are limited to twice that wide, so each half still has the full
/// height it will be split into. The result is a grayscale JPEG, which is
/// all the reader can show.
pub fn shrink_for_storage(
    bytes: &[u8],
    max_height: u32,
    quality: u8,
) -> Result<Option<Vec<u8>>, DecodeError> {
    shrink_page(
        bytes,
        &StorageLimits {
            max_height,
            min_width: STORAGE_MIN_WIDTH,
            quality,
            always: false,
        },
    )
}

/// How far a downloaded page is made smaller for storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageLimits {
    /// Tallest a portrait page is kept (a landscape spread may be twice this wide).
    pub max_height: u32,
    /// Narrowest a page is made, so a long strip is not squeezed into a sliver.
    pub min_width: u32,
    /// JPEG quality of the grayscale result.
    pub quality: u8,
    /// Re-encode every page, even one already within the limits, and keep the
    /// result when it is smaller than the original. Without this a page that
    /// is already small enough is left exactly as downloaded.
    pub always: bool,
}

impl StorageLimits {
    /// Tall pages are shrunk to twice the reader's screen; nothing visible is lost.
    pub const SAVE: Self = Self {
        max_height: STORAGE_MAX_HEIGHT,
        min_width: STORAGE_MIN_WIDTH,
        quality: 88,
        always: false,
    };

    /// As small as still reads well on a small e-ink screen: a page about
    /// 1.25 times the 800-pixel screen height (and never narrower than 600),
    /// as a low-quality grayscale JPEG, whatever it was before. On pages from a
    /// real series this is about a tenth of the downloaded size.
    pub const ULTRA: Self = Self {
        max_height: 1000,
        min_width: 600,
        quality: 60,
        always: true,
    };
}

/// [`shrink_for_storage`] with every limit given. `None` means keep the
/// original bytes.
pub fn shrink_page(
    bytes: &[u8],
    limits: &StorageLimits,
) -> Result<Option<Vec<u8>>, DecodeError> {
    let (w, h) = image_dimensions(bytes)?;
    let landscape = w as f32 > h as f32 * 1.1;
    let scale = if landscape {
        (2.0 * limits.max_height as f64) / w as f64
    } else {
        let by_height = limits.max_height as f64 / h as f64;
        let by_width = limits.min_width as f64 / w as f64;
        by_height.max(by_width)
    };
    if scale >= 1.0 && !limits.always {
        return Ok(None);
    }
    let scale = scale.min(1.0);
    let gray = decode_to_gray(bytes)?;
    let new_w = ((gray.width as f64 * scale).round() as u32).max(1);
    let new_h = ((gray.height as f64 * scale).round() as u32).max(1);
    let small = if scale < 1.0 {
        crate::process::resize(&gray, new_w, new_h)
    } else {
        gray
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, limits.quality).write_image(
        &small.pixels,
        new_w,
        new_h,
        image::ExtendedColorType::L8,
    )?;
    if limits.always && out.len() >= bytes.len() {
        return Ok(None);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        out
    }

    #[test]
    #[ignore = "manual throughput check on the export benchmark machine; run in release"]
    fn opaque_rgb_decode_throughput() {
        let rgb = image::RgbImage::from_pixel(2048, 2048, image::Rgb([20, 100, 180]));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(rgb.as_raw(), 2048, 2048, image::ExtendedColorType::Rgb8)
            .unwrap();
        std::hint::black_box(decode_to_gray(&bytes).unwrap());
        let started = std::time::Instant::now();
        for _ in 0..5 {
            std::hint::black_box(decode_to_gray(std::hint::black_box(&bytes)).unwrap());
        }
        let elapsed = started.elapsed();
        eprintln!("Five 4-megapixel opaque RGB decodes: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_millis(60),
            "first-export RGB throughput target missed: {elapsed:?}"
        );
    }

    #[test]
    fn opaque_rgb_and_partial_alpha_keep_the_original_luma_math() {
        let values = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [20, 100, 180],
            [17, 18, 19],
            [255, 255, 255],
        ];
        let rgb = image::RgbImage::from_fn(6, 1, |x, _| image::Rgb(values[x as usize]));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(rgb.as_raw(), 6, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(
            decode_to_gray(&bytes).unwrap().pixels,
            [76, 150, 29, 85, 18, 255]
        );
        let rgba = RgbaImage::from_fn(3, 1, |x, _| match x {
            0 => Rgba([255, 0, 0, 128]),
            1 => Rgba([0, 0, 0, 128]),
            _ => Rgba([0, 0, 255, 0]),
        });
        assert_eq!(decode_to_gray(&png(&rgba)).unwrap().pixels, [165, 127, 255]);
    }

    #[test]
    fn decodes_png_to_luma() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
        let g = decode_to_gray(&png(&img)).unwrap();
        assert_eq!((g.width, g.height), (2, 1));
        assert_eq!(g.pixels, vec![0, 255]);
    }

    #[test]
    fn transparent_pixels_become_white() {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        assert_eq!(decode_to_gray(&png(&img)).unwrap().pixels, vec![255]);
    }

    fn plain_png(w: u32, h: u32) -> Vec<u8> {
        png(&RgbaImage::from_pixel(w, h, Rgba([120, 120, 120, 255])))
    }

    #[test]
    fn reads_dimensions_without_decoding() {
        assert_eq!(image_dimensions(&plain_png(37, 91)).unwrap(), (37, 91));
        assert!(image_dimensions(b"nope").is_err());
    }

    #[test]
    fn small_pages_are_kept_exactly_as_downloaded() {
        assert_eq!(
            shrink_for_storage(&plain_png(900, 1600), 1600, 85).unwrap(),
            None
        );
        assert_eq!(
            shrink_for_storage(&plain_png(500, 700), 1600, 85).unwrap(),
            None
        );
    }

    #[test]
    fn tall_pages_shrink_as_far_as_the_width_floor_allows_as_gray_jpeg() {
        let out = shrink_for_storage(&plain_png(1200, 3200), 1600, 85)
            .unwrap()
            .expect("taller than the limit, so it must shrink");
        assert_eq!(&out[..3], &[0xFF, 0xD8, 0xFF], "JPEG");
        // Shrunk until the width reaches its floor (960), not down to 1600 tall.
        assert_eq!(
            image_dimensions(&out).unwrap(),
            (960, 2560),
            "aspect ratio kept"
        );
        let back = decode_to_gray(&out).unwrap();
        // Flat mid-gray stays mid-gray.
        assert!(back.pixels.iter().all(|&p| (115..=125).contains(&p)));
    }

    #[test]
    fn a_long_strip_is_never_made_narrower_than_the_screen_needs() {
        // 800 wide is already narrower than the floor: keep the original.
        assert_eq!(
            shrink_for_storage(&plain_png(800, 6000), 1600, 85).unwrap(),
            None
        );
        // Wider strips shrink only as far as the width floor, not to 1600 tall.
        let out = shrink_for_storage(&plain_png(1200, 12000), 1600, 85)
            .unwrap()
            .unwrap();
        assert_eq!(image_dimensions(&out).unwrap(), (STORAGE_MIN_WIDTH, 9600));
        // The case that went wrong: about 1000 wide and 9000 tall.
        let out = shrink_for_storage(&plain_png(1000, 9000), 1600, 85)
            .unwrap()
            .unwrap();
        let (w, _) = image_dimensions(&out).unwrap();
        assert!(w >= STORAGE_MIN_WIDTH, "only {w} wide");
    }

    #[test]
    fn spreads_may_be_twice_as_wide_so_each_half_keeps_full_height() {
        // 2400x1500 fits (limit is 3200 wide); 4000x2500 does not.
        assert_eq!(
            shrink_for_storage(&plain_png(2400, 1500), 1600, 85).unwrap(),
            None
        );
        let out = shrink_for_storage(&plain_png(4000, 2500), 1600, 85)
            .unwrap()
            .unwrap();
        assert_eq!(image_dimensions(&out).unwrap(), (3200, 2000));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_to_gray(b"definitely not an image").is_err());
        assert!(decode_to_gray(&[]).is_err());
    }
}
