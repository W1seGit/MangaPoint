//! Pages kept exactly as they are, for a phone or a tablet.
//!
//! The same rules as `crates/pipeline/src/original.rs`: a CBZ keeps every JPEG,
//! PNG and WebP byte for byte (a GIF becomes a PNG, which loses nothing); a PDF
//! embeds a JPEG as it is and writes other pictures without loss. The pages of a
//! long strip are made one width (see `strip_width`).

use super::cbz::CbzBuilder;
use super::pdf::{jpeg_info, PdfBuilder, PdfColor, PdfImage};

/// The quality of the JPEGs made when a picture cannot be kept as it is.
const REENCODE_QUALITY: u8 = 92;
/// The most pixels in a picture we will decode (the same limit as the converter).
const MAX_PIXELS: u64 = 120_000_000;

fn bad_page(what: impl std::fmt::Display) -> String {
    what.to_string()
}

fn is_png(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G'])
}

fn is_jpeg(b: &[u8]) -> bool {
    b.starts_with(&[0xff, 0xd8])
}

fn is_webp(b: &[u8]) -> bool {
    b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP"
}

/// Decode any saved picture to pixels, held to the size limit.
fn decode(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let (w, h) = converter::decode::image_dimensions(bytes).map_err(bad_page)?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(bad_page(format!("a page is too large ({w} x {h} pixels)")));
    }
    image::load_from_memory(bytes).map_err(bad_page)
}

fn encode_jpeg(rgb: &image::RgbImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, REENCODE_QUALITY)
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(bad_page)?;
    Ok(out)
}

fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let rgba = img.to_rgba8();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
        .map_err(bad_page)?;
    Ok(out)
}

/// The picture on white, whatever its transparency.
fn flatten_to_rgb(img: &image::DynamicImage) -> image::RgbImage {
    let rgba = img.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (to, from) in rgb.pixels_mut().zip(rgba.pixels()) {
        let [r, g, b, a] = from.0;
        let blend = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
        *to = image::Rgb([blend(r), blend(g), blend(b)]);
    }
    rgb
}

// ---- one width for a long strip ----------------------------------------------------------------

/// A chapter whose pages are this much taller than wide (the middle of them) is a long strip.
const STRIP_ASPECT: f64 = 2.2;

/// The width every page of a long strip is made, when the pages do not already share one: the
/// width most of them have (the larger when two tie). `None` when the chapter is not a long strip,
/// or its pages are one width already, so nothing is touched. A cover or a credits page of another
/// width is then scaled to match the rest, instead of showing at a size of its own in the reader.
/// `sizes` is each page's `(width, height)`; a page whose size is not known is left out.
pub fn strip_width(sizes: &[(u32, u32)]) -> Option<u32> {
    let known: Vec<(u32, u32)> = sizes.iter().copied().filter(|(w, h)| *w > 0 && *h > 0).collect();
    if known.len() < 2 {
        return None;
    }
    let mut ratios: Vec<f64> = known.iter().map(|(w, h)| f64::from(*h) / f64::from(*w)).collect();
    ratios.sort_by(f64::total_cmp);
    if ratios[ratios.len() / 2] < STRIP_ASPECT {
        return None;
    }
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for (w, _) in &known {
        *counts.entry(*w).or_default() += 1;
    }
    if counts.len() < 2 {
        return None;
    }
    // The most common width; on a tie the larger one (the map is in ascending order).
    counts.into_iter().max_by_key(|(w, n)| (*n, *w)).map(|(w, _)| w)
}

/// The page scaled to `target` wide (its height follows), or `None` when it is that wide already
/// or its size cannot be read. A PNG stays a PNG; anything else becomes a high-quality JPEG.
pub fn fit_width(bytes: &[u8], target: u32) -> Result<Option<Vec<u8>>, String> {
    let Ok((w, h)) = converter::decode::image_dimensions(bytes) else { return Ok(None) };
    if w == target || w == 0 || h == 0 {
        return Ok(None);
    }
    let height = ((f64::from(h) * f64::from(target) / f64::from(w)).round() as u32).max(1);
    let img = decode(bytes)?;
    let scaled = img.resize_exact(target, height, image::imageops::FilterType::Lanczos3);
    if is_png(bytes) {
        encode_png(&scaled).map(Some)
    } else {
        encode_jpeg(&flatten_to_rgb(&scaled)).map(Some)
    }
}

/// Put one saved page into a CBZ: a JPEG, PNG or WebP as it is; anything else
/// (a GIF) as a PNG, which loses nothing.
pub fn add_to_cbz(book: &mut CbzBuilder, bytes: &[u8]) -> Result<(), String> {
    if is_jpeg(bytes) || is_png(bytes) || is_webp(bytes) {
        return book.add_original(bytes);
    }
    book.add_original(&encode_png(&decode(bytes)?)?)
}

/// Put one saved page into a PDF.
pub fn add_to_pdf(book: &mut PdfBuilder, bytes: &[u8]) -> Result<(), String> {
    if is_jpeg(bytes) {
        if let Some(info) = jpeg_info(bytes) {
            let color = match info.components {
                1 => Some(PdfColor::Gray),
                3 => Some(PdfColor::Rgb),
                _ => None,
            };
            if let (Some(color), 8) = (color, info.precision) {
                return book.add_page(PdfImage::Jpeg {
                    data: bytes,
                    width: info.width,
                    height: info.height,
                    color,
                });
            }
        }
        // Four channels (CMYK) or twelve bits: a PDF would show it wrongly as it is.
        let rgb = flatten_to_rgb(&decode(bytes)?);
        let jpeg = encode_jpeg(&rgb)?;
        return book.add_page(PdfImage::Jpeg {
            data: &jpeg,
            width: rgb.width(),
            height: rgb.height(),
            color: PdfColor::Rgb,
        });
    }
    let img = decode(bytes)?;
    if is_webp(bytes) {
        // Pages saved as WebP are lossy nearly always: a high-quality JPEG is
        // smaller than the pixels written out whole.
        let rgb = flatten_to_rgb(&img);
        let jpeg = encode_jpeg(&rgb)?;
        return book.add_page(PdfImage::Jpeg {
            data: &jpeg,
            width: rgb.width(),
            height: rgb.height(),
            color: PdfColor::Rgb,
        });
    }
    let has_alpha = img.color().has_alpha();
    if !has_alpha && img.color().channel_count() <= 2 {
        let gray = img.to_luma8();
        return book.add_page(PdfImage::Raw {
            pixels: gray.as_raw(),
            width: gray.width(),
            height: gray.height(),
            color: PdfColor::Gray,
        });
    }
    let rgb = flatten_to_rgb(&img);
    book.add_page(PdfImage::Raw {
        pixels: rgb.as_raw(),
        width: rgb.width(),
        height: rgb.height(),
        color: PdfColor::Rgb,
    })
}
