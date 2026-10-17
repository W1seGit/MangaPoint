//! PDF: books whose pages are pictures (scanned or comic PDFs). The picture
//! of each page is taken out as it is: a JPEG as it stands, other pictures
//! turned into PNGs. Pages drawn from text or vector art have no picture and
//! are not imported; nothing is rendered.

use super::names::{chapter_name, chapter_number, clean_name, series_title};
use super::{fail, BookSource, Format, ImportBook, ImportChapter, PageRef};
use crate::error::{PipelineError, Result};
use image::{DynamicImage, ImageFormat};
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

/// The largest PDF read, and the most a single picture may inflate to.
const MAX_PDF_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PICTURE_BYTES: usize = 400 * 1024 * 1024;

fn load(path: &Path) -> Result<Document> {
    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let file = std::fs::File::open(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    let mut bytes = Vec::new();
    file.take(MAX_PDF_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    if bytes.len() as u64 > MAX_PDF_BYTES {
        return Err(fail(format!("{label} is too large to import")));
    }
    let doc = Document::load_mem(&bytes).map_err(|e| fail(format!("{label} is not a readable PDF ({e})")))?;
    if doc.is_encrypted() {
        return Err(fail(format!("{label} is protected by a password, which MangaPoint cannot use.")));
    }
    Ok(doc)
}

/// A text of the document information: UTF-16 with a mark, else Latin-1.
fn pdf_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        String::from_utf16_lossy(&units)
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        text.to_string()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn info_field(doc: &Document, name: &[u8]) -> String {
    doc.trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|id| doc.get_dictionary(id).ok())
        .and_then(|d| d.get(name).ok())
        .and_then(|o| o.as_str().ok())
        .map(|b| pdf_text(b).trim().to_string())
        .unwrap_or_default()
}

/// The picture of a page, if it has one that can be taken out. The largest
/// picture on the page is the page.
fn page_picture(doc: &Document, page: ObjectId) -> Result<Vec<u8>> {
    let images = doc.get_page_images(page).map_err(|e| fail(e.to_string()))?;
    let image = images
        .iter()
        .max_by_key(|i| i.width.saturating_mul(i.height))
        .ok_or_else(|| fail("the page has no picture"))?;
    let filters: Vec<&str> = image.filters.as_deref().unwrap_or(&[]).iter().map(String::as_str).collect();
    if filters == ["DCTDecode"] {
        return Ok(image.content.to_vec());
    }
    if filters.iter().any(|f| matches!(*f, "JPXDecode" | "CCITTFaxDecode" | "JBIG2Decode" | "DCTDecode")) {
        return Err(fail(format!("the page picture uses {}, which is not supported", filters.join(" + "))));
    }
    let stream = doc
        .get_object(image.id)
        .and_then(Object::as_stream)
        .map_err(|e| fail(e.to_string()))?;
    let data = stream
        .decompressed_content_with_limit(MAX_PICTURE_BYTES)
        .map_err(|e| fail(format!("the page picture cannot be decoded ({e})")))?;
    let (w, h) = (image.width as u32, image.height as u32);
    let bpc = image.bits_per_component.unwrap_or(8);
    let inverted = image
        .origin_dict
        .get(b"Decode")
        .ok()
        .and_then(|d| d.as_array().ok())
        .is_some_and(|a| a.first().and_then(|v| v.as_float().ok()).is_some_and(|v| v > 0.5));
    let picture = to_image(doc, image.color_space.as_deref(), image.origin_dict, w, h, bpc, inverted, &data)
        .ok_or_else(|| fail("the page picture is of a kind that is not supported"))?;
    let mut png = Cursor::new(Vec::new());
    picture.write_to(&mut png, ImageFormat::Png).map_err(|e| fail(e.to_string()))?;
    Ok(png.into_inner())
}

/// Build a picture from raw samples. `None` for kinds not handled.
#[allow(clippy::too_many_arguments)]
fn to_image(
    doc: &Document,
    space: Option<&str>,
    dict: &lopdf::Dictionary,
    w: u32,
    h: u32,
    bpc: i64,
    inverted: bool,
    data: &[u8],
) -> Option<DynamicImage> {
    // How many samples make a pixel, and how to read an indexed one.
    let components = match space? {
        "DeviceGray" | "CalGray" => 1,
        "DeviceRGB" | "CalRGB" => 3,
        "DeviceCMYK" => 4,
        "ICCBased" => {
            let array = dict.get(b"ColorSpace").ok()?.as_array().ok()?;
            let profile = doc.get_object(array.get(1)?.as_reference().ok()?).ok()?.as_stream().ok()?;
            profile.dict.get(b"N").ok()?.as_i64().ok()? as usize
        }
        "Indexed" => 1,
        _ => return None,
    };
    let mut palette: Option<(usize, Vec<u8>)> = None;
    if space == Some("Indexed") {
        let array = dict.get(b"ColorSpace").ok()?.as_array().ok()?;
        let base = match array.get(1)? {
            Object::Name(n) => String::from_utf8_lossy(n).to_string(),
            Object::Reference(id) => {
                let o = doc.get_object(*id).ok()?;
                match o {
                    Object::Name(n) => String::from_utf8_lossy(n).to_string(),
                    Object::Array(a) => String::from_utf8_lossy(a.first()?.as_name().ok()?).to_string(),
                    _ => return None,
                }
            }
            Object::Array(a) => String::from_utf8_lossy(a.first()?.as_name().ok()?).to_string(),
            _ => return None,
        };
        let base_components = match base.as_str() {
            "DeviceGray" | "CalGray" => 1,
            "DeviceRGB" | "CalRGB" => 3,
            _ => return None,
        };
        let table = match array.get(3)? {
            Object::String(s, _) => s.clone(),
            Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok()?.decompressed_content().ok()?,
            _ => return None,
        };
        palette = Some((base_components, table));
    }
    if components == 0 {
        return None;
    }
    // 1, 2, 4 and 8 bits a sample are read; 16 keeps the high byte.
    let (w, h) = (w as usize, h as usize);
    let samples = unpack(data, w, h, components, bpc, palette.is_none())?;
    if let Some((base, table)) = palette {
        let mut rgb = Vec::with_capacity(w * h * 3);
        for &index in &samples {
            let at = usize::from(index) * base;
            let entry = table.get(at..at + base)?;
            if base == 1 {
                rgb.extend([entry[0]; 3]);
            } else {
                rgb.extend_from_slice(entry);
            }
        }
        return image::RgbImage::from_raw(w as u32, h as u32, rgb).map(DynamicImage::ImageRgb8);
    }
    let mut samples = samples;
    if inverted {
        for s in &mut samples {
            *s = 255 - *s;
        }
    }
    match components {
        1 => image::GrayImage::from_raw(w as u32, h as u32, samples).map(DynamicImage::ImageLuma8),
        3 => image::RgbImage::from_raw(w as u32, h as u32, samples).map(DynamicImage::ImageRgb8),
        4 => {
            // CMYK to RGB.
            let rgb: Vec<u8> = samples
                .chunks_exact(4)
                .flat_map(|p| {
                    let k = 255 - u32::from(p[3]);
                    [0, 1, 2].map(|i| ((255 - u32::from(p[i])) * k / 255) as u8)
                })
                .collect();
            image::RgbImage::from_raw(w as u32, h as u32, rgb).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}

/// Samples of `bpc` bits, rows padded to whole bytes, as one byte each. With
/// `scale`, samples of fewer than 8 bits are spread over 0..=255 (a 1-bit
/// sample is 0 or 255); an indexed picture's samples stay indices.
fn unpack(data: &[u8], w: usize, h: usize, components: usize, bpc: i64, scale: bool) -> Option<Vec<u8>> {
    let per_row = w * components;
    let mut out = Vec::with_capacity(per_row * h);
    match bpc {
        8 => {
            if data.len() < per_row * h {
                return None;
            }
            out.extend_from_slice(&data[..per_row * h]);
        }
        16 => {
            if data.len() < per_row * h * 2 {
                return None;
            }
            out.extend(data[..per_row * h * 2].chunks_exact(2).map(|p| p[0]));
        }
        1 | 2 | 4 => {
            let bits = bpc as usize;
            let row_bytes = (per_row * bits).div_ceil(8);
            if data.len() < row_bytes * h {
                return None;
            }
            let max = (1u32 << bits) - 1;
            for row in 0..h {
                let line = &data[row * row_bytes..(row + 1) * row_bytes];
                for i in 0..per_row {
                    let at = i * bits;
                    let v = u32::from((line[at / 8] >> (8 - bits - at % 8)) & (max as u8));
                    out.push(if scale { (v * 255 / max) as u8 } else { v as u8 });
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

pub(crate) fn scan(path: &Path) -> Result<ImportBook> {
    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let doc = load(path)?;
    let pages: BTreeMap<u32, ObjectId> = doc.get_pages();
    let mut book = ImportBook::new(&label, Format::Pdf, BookSource::Pdf(path.to_path_buf()));
    let mut kept: Vec<u32> = Vec::new();
    let mut without = 0usize;
    for (&number, &id) in &pages {
        match page_picture_kind(&doc, id) {
            true => kept.push(number),
            false => without += 1,
        }
    }
    if kept.is_empty() {
        return Err(fail(format!(
            "{label} has no page pictures: its pages are drawn from text or vector art, which MangaPoint does not render. Only PDFs whose pages are pictures (scans, comics) can be imported."
        )));
    }
    if without > 0 {
        book.warnings.push(format!(
            "{without} page(s) have no picture (text or drawings), and are left out."
        ));
    }
    let title = info_field(&doc, b"Title");
    book.meta.title = if title.is_empty() { series_title(&label) } else { title };
    book.meta.author = info_field(&doc, b"Author");
    book.meta.description = info_field(&doc, b"Subject");
    book.meta.genres = info_field(&doc, b"Keywords")
        .split([',', ';'])
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();

    // Chapters from the outline: its top level, at the pages it points to.
    let mut starts: Vec<(usize, String)> = Vec::new();
    if let Ok(toc) = doc.get_toc() {
        let top = toc.toc.iter().map(|t| t.level).min().unwrap_or(1);
        for entry in toc.toc.iter().filter(|t| t.level == top && !t.title.trim().is_empty()) {
            if let Some(at) = kept.iter().position(|&n| n as usize >= entry.page) {
                starts.push((at, entry.title.trim().to_string()));
            }
        }
    }
    starts.sort_by_key(|(i, _)| *i);
    starts.dedup_by_key(|(i, _)| *i);
    let page_ref = |n: &u32| PageRef { source: 0, key: n.to_string() };
    if starts.len() > 1 {
        if starts[0].0 > 0 {
            starts.insert(0, (0, String::new()));
        }
        for (n, (from, title)) in starts.iter().enumerate() {
            let end = starts.get(n + 1).map_or(kept.len(), |(i, _)| *i);
            let number = chapter_number(title);
            book.chapters.push(ImportChapter::new(
                number,
                if title.is_empty() { chapter_name("", number) } else { chapter_name(title, number) },
                kept[*from..end].iter().map(page_ref).collect(),
            ));
        }
    } else {
        let number = chapter_number(&label);
        book.chapters.push(ImportChapter::new(
            number,
            chapter_name(&clean_name(&label), number),
            kept.iter().map(page_ref).collect(),
        ));
    }
    book.cover = pages.get(&kept[0]).and_then(|&id| page_picture(&doc, id).ok());
    Ok(book)
}

/// Whether a page has a picture to take out (without decoding it).
fn page_picture_kind(doc: &Document, page: ObjectId) -> bool {
    doc.get_page_images(page).is_ok_and(|images| !images.is_empty())
}

pub(crate) fn visit(
    path: &Path,
    keys: &[String],
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let doc = load(path)?;
    let pages = doc.get_pages();
    for key in keys {
        let picture = key
            .parse::<u32>()
            .ok()
            .and_then(|n| pages.get(&n))
            .and_then(|&id| page_picture(&doc, id).ok());
        each(key, picture.unwrap_or_default())?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    /// A PDF whose pages are the given pictures: (width, height, filter, samples).
    pub(crate) fn make_pdf(path: &Path, pictures: &[(u32, u32, Option<&str>, Vec<u8>)], title: &str) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();
        for (w, h, filter, content) in pictures {
            let mut dict = dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => *w as i64, "Height" => *h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            };
            if let Some(f) = filter {
                dict.set("Filter", *f);
            }
            let image = doc.add_object(Stream::new(dict, content.clone()));
            let contents = doc.add_object(Stream::new(dictionary! {}, b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Contents" => contents,
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            });
            kids.push(page.into());
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        if !title.is_empty() {
            let info = doc.add_object(dictionary! {
                "Title" => Object::string_literal(title), "Author" => Object::string_literal("A. Writer"),
            });
            doc.trailer.set("Info", info);
        }
        doc.save(path).unwrap();
    }

    fn jpeg(shade: u8) -> Vec<u8> {
        let img = image::GrayImage::from_pixel(16, 16, image::Luma([shade]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_pdf_of_pictures_gives_a_page_for_each_with_its_details() {
        let dir = std::env::temp_dir().join(format!("mp-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Scans - c5.pdf");
        make_pdf(
            &path,
            &[(16, 16, Some("DCTDecode"), jpeg(40)), (8, 4, None, vec![200; 32])],
            "My Scans",
        );
        let book = scan(&path).unwrap();
        assert_eq!(book.meta.title, "My Scans");
        assert_eq!(book.meta.author, "A. Writer");
        assert_eq!(book.page_count(), 2);
        assert_eq!(book.chapters[0].number, Some(5.0));
        assert!(book.cover.is_some());
        // The JPEG goes through as it is; raw samples become a PNG.
        let mut got = Vec::new();
        visit(&path, &["1".into(), "2".into()], &mut |k, bytes| {
            got.push((k.to_string(), bytes));
            Ok(())
        })
        .unwrap();
        assert_eq!(image::guess_format(&got[0].1).unwrap(), ImageFormat::Jpeg);
        assert_eq!(image::guess_format(&got[1].1).unwrap(), ImageFormat::Png);
        let raw = image::load_from_memory(&got[1].1).unwrap().to_luma8();
        assert_eq!((raw.width(), raw.height(), raw.get_pixel(3, 2).0[0]), (8, 4, 200));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pdf_with_no_pictures_is_refused_and_says_what_it_can_do() {
        let dir = std::env::temp_dir().join(format!("mp-pdf2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("text.pdf");
        make_pdf(&path, &[], "");
        let message = scan(&path).unwrap_err().to_string();
        assert!(message.contains("no page pictures"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn samples_of_one_bit_are_scaled_and_rows_padded_to_bytes() {
        // 3 pixels wide: each row is one byte, 101 and 010.
        let out = unpack(&[0b1010_0000, 0b0100_0000], 3, 2, 1, 1, true).unwrap();
        assert_eq!(out, [255, 0, 255, 0, 255, 0]);
        assert_eq!(unpack(&[0b1010_0000], 3, 1, 1, 1, false).unwrap(), [1, 0, 1]);
        assert_eq!(unpack(&[1, 2, 3, 4], 2, 1, 1, 16, true).unwrap(), [1, 3]);
        assert!(unpack(&[1], 2, 2, 1, 8, true).is_none());
    }
}
