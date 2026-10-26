//! PDF: books whose pages are pictures (scanned or comic PDFs). The picture
//! of each page is taken out as it is: a JPEG as it stands, other pictures
//! turned into PNGs. Pages drawn from text or vector art have no picture and
//! are not imported; nothing is rendered.

use super::names::{chapter_name, chapter_number, clean_name, series_title};
use super::{fail, BookSource, Format, ImportBook, ImportChapter, PageRef};
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use image::{DynamicImage, ImageFormat};
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

/// The largest PDF read (the parser holds the whole file in memory, and the
/// objects it makes from it besides), and the most a single picture's samples
/// may take once unpacked.
const MAX_PDF_BYTES: u64 = 768 * 1024 * 1024;
const MAX_PICTURE_BYTES: usize = 512 * 1024 * 1024;

fn load(path: &Path) -> Result<Document> {
    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let file = std::fs::File::open(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    // Refused by its size before any of it is read.
    let size = file.metadata().map_err(|e| PipelineError::Io(path.to_path_buf(), e))?.len();
    if size > MAX_PDF_BYTES {
        return Err(fail(format!(
            "{label} is {} MB, which is more than the {} MB MangaPoint reads as one PDF. Split it into smaller PDFs first.",
            size / (1024 * 1024),
            MAX_PDF_BYTES / (1024 * 1024)
        )));
    }
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
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|p| u16::from_be_bytes(*p)).collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
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
    // The size the picture says it has is not trusted: a PDF can say anything.
    let (w, h) = match (u32::try_from(image.width), u32::try_from(image.height)) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 && u64::from(w) * u64::from(h) <= super::MAX_PAGE_PIXELS => (w, h),
        _ => {
            return Err(fail(format!(
                "the page picture is {} x {}, which is not a size MangaPoint can use",
                image.width, image.height
            )))
        }
    };
    let stream = doc
        .get_object(image.id)
        .and_then(Object::as_stream)
        .map_err(|e| fail(e.to_string()))?;
    let data = stream
        .decompressed_content_with_limit(MAX_PICTURE_BYTES)
        .map_err(|e| fail(format!("the page picture cannot be decoded ({e})")))?;
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
            // The profile says how many samples a pixel has; only 1, 3 and 4 are
            // pictures (a negative or huge number is a damaged or hostile file).
            let n = usize::try_from(profile.dict.get(b"N").ok()?.as_i64().ok()?).ok()?;
            if !matches!(n, 1 | 3 | 4) {
                return None;
            }
            n
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
        let mut rgb = Vec::with_capacity(w.checked_mul(h)?.checked_mul(3).filter(|n| *n <= MAX_PICTURE_BYTES)?);
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
                .as_chunks::<4>()
                .0
                .iter()
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
    // The sizes come from the file, so none of this arithmetic can be trusted
    // not to overflow, and nothing is allocated before it is known to fit.
    let per_row = w.checked_mul(components)?;
    let count = per_row.checked_mul(h).filter(|n| *n <= MAX_PICTURE_BYTES)?;
    if components == 0 || w == 0 || h == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    match bpc {
        8 => {
            if data.len() < count {
                return None;
            }
            out.extend_from_slice(&data[..count]);
        }
        16 => {
            let bytes = count.checked_mul(2)?;
            if data.len() < bytes {
                return None;
            }
            out.extend(data[..bytes].as_chunks::<2>().0.iter().map(|p| p[0]));
        }
        1 | 2 | 4 => {
            let bits = bpc as usize;
            let row_bytes = per_row.checked_mul(bits)?.div_ceil(8);
            if data.len() < row_bytes.checked_mul(h)? {
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

pub(crate) fn scan(path: &Path, cancel: &CancelFlag) -> Result<ImportBook> {
    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let doc = load(path)?;
    let pages: BTreeMap<u32, ObjectId> = doc.get_pages();
    let mut book = ImportBook::new(&label, Format::Pdf, BookSource::Pdf(path.to_path_buf()));
    let mut kept: Vec<u32> = Vec::new();
    let mut without = 0usize;
    for (&number, &id) in &pages {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
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
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let doc = load(path)?;
    let pages = doc.get_pages();
    for key in keys {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
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
        let book = scan(&path, &CancelFlag::new()).unwrap();
        assert_eq!(book.meta.title, "My Scans");
        assert_eq!(book.meta.author, "A. Writer");
        assert_eq!(book.page_count(), 2);
        assert_eq!(book.chapters[0].number, Some(5.0));
        assert!(book.cover.is_some());
        // The JPEG goes through as it is; raw samples become a PNG.
        let mut got = Vec::new();
        visit(&path, &["1".into(), "2".into()], &CancelFlag::new(), &mut |k, bytes| {
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
        let message = scan(&path, &CancelFlag::new()).unwrap_err().to_string();
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

    /// A one-page PDF whose picture has an ICC colour space of `n` samples a pixel.
    fn make_icc_pdf(path: &Path, n: i64, w: u32, h: u32) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let profile = doc.add_object(Stream::new(dictionary! { "N" => n }, vec![0u8; 8]));
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => i64::from(w), "Height" => i64::from(h),
                "ColorSpace" => vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(profile)],
                "BitsPerComponent" => 8,
            },
            vec![7u8; 64],
        ));
        let contents = doc.add_object(Stream::new(dictionary! {}, b"q 100 0 0 100 0 0 cm /Im0 Do Q".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => contents,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(path).unwrap();
    }

    fn page_one(path: &Path) -> Vec<u8> {
        let mut got = None;
        visit(path, &["1".into()], &CancelFlag::new(), &mut |_, bytes| {
            got = Some(bytes);
            Ok(())
        })
        .unwrap();
        got.unwrap()
    }

    #[test]
    fn absurd_picture_sizes_are_refused_with_an_error_not_allocated() {
        let dir = std::env::temp_dir().join(format!("mp-pdf-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, (w, h)) in [(100_000u32, 100_000u32), (u32::MAX, u32::MAX), (20_000, 20_000)].into_iter().enumerate() {
            let path = dir.join(format!("huge{i}.pdf"));
            make_pdf(&path, &[(w, h, None, vec![0; 16])], "");
            // The book is read (the page has a picture), but the picture is not made.
            let book = scan(&path, &CancelFlag::new()).unwrap();
            assert!(book.cover.is_none(), "{w}x{h}");
            assert!(page_one(&path).is_empty(), "{w}x{h}");
        }
        // The same through the one function that does the arithmetic.
        assert!(unpack(&[], usize::MAX, 2, 3, 8, true).is_none());
        assert!(unpack(&[], 1 << 40, 1 << 40, 4, 16, true).is_none());
        assert!(unpack(&[], 1 << 20, 1 << 20, 1, 1, true).is_none());
        assert!(unpack(&[], 4, 4, 0, 8, true).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_colour_space_with_a_negative_or_impossible_sample_count_is_refused() {
        let dir = std::env::temp_dir().join(format!("mp-pdf-icc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in [-5i64, 0, 2, 5, 1 << 40, i64::MIN] {
            let path = dir.join(format!("icc{}.pdf", n.unsigned_abs()));
            make_icc_pdf(&path, n, 4, 4);
            assert!(page_one(&path).is_empty(), "N = {n}");
        }
        // A sane profile still works.
        let path = dir.join("icc3.pdf");
        make_icc_pdf(&path, 3, 4, 4);
        assert_eq!(image::guess_format(&page_one(&path)).unwrap(), ImageFormat::Png);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pdf_too_large_to_read_is_refused_by_its_size_before_it_is_read() {
        let dir = std::env::temp_dir().join(format!("mp-pdf-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("giant.pdf");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_PDF_BYTES + 1).unwrap();
        drop(file);
        let message = scan(&path, &CancelFlag::new()).unwrap_err().to_string();
        assert!(message.contains("MB") && message.contains("giant.pdf"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_a_pdf_stops_when_cancelled() {
        let dir = std::env::temp_dir().join(format!("mp-pdf-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.pdf");
        make_pdf(&path, &[(8, 4, None, vec![1; 32]), (8, 4, None, vec![2; 32])], "");
        let cancel = CancelFlag::new();
        cancel.cancel();
        assert!(matches!(scan(&path, &cancel), Err(PipelineError::Cancelled)));
        let r = visit(&path, &["1".into()], &cancel, &mut |_, _| Ok(()));
        assert!(matches!(r, Err(PipelineError::Cancelled)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
