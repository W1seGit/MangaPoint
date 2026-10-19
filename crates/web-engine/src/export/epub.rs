//! An EPUB of page pictures, built in memory.
//!
//! A copy of `crates/pipeline/src/epub.rs` that writes into a `Vec<u8>` instead of
//! a file: same encoders, same names, same package files, so the bytes are the
//! same (the tests in `tests/parity.rs` compare them with the desktop's).

use converter::{ColorImage, Depth, LevelImage};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// JPEG quality for readers with many grays: small, and clean on e-paper.
const JPEG_QUALITY: u8 = 62;

/// JPEG quality for pages kept in colour.
const COLOR_JPEG_QUALITY: u8 = 82;

/// How many pages one EPUB is cut at (readers cannot open longer lists).
pub const MAX_PAGES: usize = 800;

fn fail(message: impl std::fmt::Display) -> String {
    format!("EPUB: {message}")
}

/// One page as a picture: a PNG for pages of up to four grays, a JPEG for full grays.
pub fn encode_page(img: &LevelImage) -> Result<Vec<u8>, String> {
    match img.depth {
        Depth::Eight => encode_jpeg(img),
        depth => encode_png(img, depth),
    }
}

/// One page kept in colour, as a JPEG at quality 82 without chroma
/// subsampling; a page with no colour at all is saved as a grayscale JPEG.
pub fn encode_color_page(img: &ColorImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(img.rgb.len() / 8);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, COLOR_JPEG_QUALITY);
    if img.rgb.as_chunks::<3>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]) {
        let gray: Vec<u8> = img.rgb.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        encoder
            .encode(&gray, img.width, img.height, image::ExtendedColorType::L8)
            .map_err(fail)?;
    } else {
        encoder
            .encode(&img.rgb, img.width, img.height, image::ExtendedColorType::Rgb8)
            .map_err(fail)?;
    }
    Ok(out)
}

fn encode_jpeg(img: &LevelImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(img.levels.len() / 8);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(&img.levels, img.width, img.height, image::ExtendedColorType::L8)
        .map_err(fail)?;
    Ok(out)
}

/// An indexed PNG with the fewest bits the grays need.
fn encode_png(img: &LevelImage, depth: Depth) -> Result<Vec<u8>, String> {
    let (bits, per_byte, palette): (png::BitDepth, usize, Vec<u8>) = match depth {
        Depth::One => (png::BitDepth::One, 8, vec![0, 0, 0, 255, 255, 255]),
        _ => (
            png::BitDepth::Two,
            4,
            vec![0, 0, 0, 85, 85, 85, 170, 170, 170, 255, 255, 255],
        ),
    };
    let shift = 8 / per_byte;
    let (w, h) = (img.width as usize, img.height as usize);
    let row_bytes = w.div_ceil(per_byte);
    let mut data = vec![0u8; row_bytes * h];
    for y in 0..h {
        for x in 0..w {
            let level = img.levels[y * w + x];
            data[y * row_bytes + x / per_byte] |= level << (8 - shift - (x % per_byte) * shift);
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, img.width, img.height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(bits);
        encoder.set_palette(palette);
        encoder.set_compression(png::Compression::High);
        encoder.set_filter(png::Filter::Adaptive);
        let mut writer = encoder.write_header().map_err(fail)?;
        writer.write_image_data(&data).map_err(fail)?;
        writer.finish().map_err(fail)?;
    }
    Ok(out)
}

/// What the book says about itself.
#[derive(Clone, Debug)]
pub struct EpubMeta {
    pub title: String,
    /// A language tag such as "en".
    pub language: String,
    /// Read right to left (manga).
    pub rtl: bool,
    /// The size of every page, in pixels.
    pub width: u32,
    pub height: u32,
    /// Seconds since the Unix epoch.
    pub created: u64,
}

/// An EPUB being written, page by page.
pub struct EpubBuilder {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    meta: EpubMeta,
    /// The picture type of each page, by page: `png` or `jpg`.
    kinds: Vec<&'static str>,
    chapters: Vec<(String, usize)>,
}

impl EpubBuilder {
    /// Start a book.
    pub fn new(meta: EpubMeta) -> Result<Self, String> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        // The type of the book comes first and is not compressed, which is how
        // readers tell what a zip is without opening it.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored).map_err(fail)?;
        zip.write_all(b"application/epub+zip").map_err(fail)?;
        let mut book = Self { zip, meta, kinds: Vec::new(), chapters: Vec::new() };
        book.text(
            "META-INF/container.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )?;
        book.text(
            "OEBPS/s.css",
            "html,body{margin:0;padding:0;width:100%;height:100%;background:#fff}img{display:block;width:100%;height:100%;object-fit:contain}",
        )?;
        Ok(book)
    }

    /// How many pages the book has so far.
    pub fn page_count(&self) -> usize {
        self.kinds.len()
    }

    fn text(&mut self, name: &str, body: &str) -> Result<(), String> {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        self.zip.start_file(name, options).map_err(fail)?;
        self.zip.write_all(body.as_bytes()).map_err(fail)
    }

    /// Add a page that [`encode_page`] made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<(), String> {
        let kind = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else if picture.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            return Err(fail("a page is neither a PNG nor a JPEG picture"));
        };
        let n = self.kinds.len() + 1;
        // Pictures are compressed already: storing them is as small as it gets.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        self.zip
            .start_file(format!("OEBPS/{n}.{kind}"), stored)
            .map_err(fail)?;
        self.zip.write_all(picture).map_err(fail)?;
        let (w, h) = (self.meta.width, self.meta.height);
        let page = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width={w},height={h}\"/><title>{n}</title><link rel=\"stylesheet\" href=\"s.css\"/></head><body><img src=\"{n}.{kind}\" alt=\"\"/></body></html>"
        );
        self.text(&format!("OEBPS/{n}.xhtml"), &page)?;
        self.kinds.push(kind);
        Ok(())
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize) {
        self.chapters.push((name.to_string(), start));
    }

    /// Write the contents and the package, and give the finished file.
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        if self.kinds.is_empty() {
            return Err(fail("the book has no pages"));
        }
        let nav = self.nav();
        self.text("OEBPS/n.xhtml", &nav)?;
        let opf = self.package();
        self.text("OEBPS/c.opf", &opf)?;
        Ok(self.zip.finish().map_err(fail)?.into_inner())
    }

    fn nav(&self) -> String {
        let mut items = String::new();
        let first = (self.meta.title.clone(), 0);
        let chapters: &[(String, usize)] = if self.chapters.is_empty() {
            std::slice::from_ref(&first)
        } else {
            &self.chapters
        };
        for (name, start) in chapters {
            items.push_str(&format!(
                "<li><a href=\"{}.xhtml\">{}</a></li>",
                start + 1,
                escape(name)
            ));
        }
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><meta charset=\"utf-8\"/><title>{}</title></head><body><nav epub:type=\"toc\"><ol>{items}</ol></nav><nav epub:type=\"landmarks\" hidden=\"hidden\"><ol><li><a epub:type=\"cover\" href=\"1.xhtml\">Cover</a></li><li><a epub:type=\"bodymatter\" href=\"1.xhtml\">Start</a></li></ol></nav></body></html>",
            escape(&self.meta.title)
        )
    }

    fn package(&self) -> String {
        let pages = self.kinds.len();
        let mut manifest = String::with_capacity(pages * 110);
        let mut spine = String::with_capacity(pages * 28);
        for (i, kind) in self.kinds.iter().enumerate() {
            let n = i + 1;
            let mime = if *kind == "png" { "image/png" } else { "image/jpeg" };
            let cover = if i == 0 { " properties=\"cover-image\"" } else { "" };
            manifest.push_str(&format!(
                "<item id=\"p{n}\" href=\"{n}.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"i{n}\" href=\"{n}.{kind}\" media-type=\"{mime}\"{cover}/>"
            ));
            spine.push_str(&format!("<itemref idref=\"p{n}\"/>"));
        }
        let direction = if self.meta.rtl { "rtl" } else { "ltr" };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"u\" prefix=\"rendition: http://www.idpf.org/vocab/rendition/#\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"u\">urn:uuid:{}</dc:identifier><dc:title>{}</dc:title><dc:language>{}</dc:language><meta property=\"dcterms:modified\">{}</meta><meta property=\"rendition:layout\">pre-paginated</meta><meta property=\"rendition:orientation\">portrait</meta><meta property=\"rendition:spread\">none</meta><meta name=\"cover\" content=\"i1\"/></metadata><manifest><item id=\"n\" href=\"n.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"s\" href=\"s.css\" media-type=\"text/css\"/>{manifest}</manifest><spine page-progression-direction=\"{direction}\">{spine}</spine></package>",
            self.identifier(),
            escape(&self.meta.title),
            escape(if self.meta.language.is_empty() { "en" } else { &self.meta.language }),
            iso8601(self.meta.created),
        )
    }

    /// A stable id for the book, made from what it is.
    fn identifier(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.meta.title.as_bytes());
        hash.update(self.meta.created.to_le_bytes());
        hash.update((self.kinds.len() as u64).to_le_bytes());
        let d = hash.finalize();
        let hex: String = d[..16].iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
}

/// Text safe to put in XML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters are not allowed in XML at all.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// `2026-10-14T16:21:08Z` from seconds since the Unix epoch.
fn iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}
