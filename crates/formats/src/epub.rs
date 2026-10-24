//! Making an EPUB of page pictures.
//!
//! The book is a fixed-layout EPUB 3: one page of the manga per spine item, each
//! a picture exactly the size of the reader's screen, so a reader shows it
//! as it is and never scales or reflows it. It is built to be small and quick
//! for the reader to open:
//!
//! * the pictures are stored in the zip as they are (they are already
//!   compressed), and only the little XML files are deflated;
//! * file names and ids are as short as they can be, and one style sheet
//!   serves every page, so the package files of a book with hundreds of pages
//!   stay small;
//! * a reader showing four grays gets a 2-bit palette PNG, which it can show
//!   without converting and which is a quarter of the size of a 1-bit-per-gray
//!   XTCH page; a reader showing more gets a plain grayscale JPEG;
//! * the cover, the chapter list (as the table of contents) and the reading
//!   direction are all there.
//!
//! Pages are written to the file as they arrive, so a book does not have to fit
//! in memory.

use crate::{FormatError, Result};
use converter::{ColorImage, Depth, LevelImage};
use sha2::{Digest, Sha256};
use std::io::{Seek, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// JPEG quality for readers with many grays: small, and clean on e-paper.
const JPEG_QUALITY: u8 = 62;

/// JPEG quality for pages kept in colour. Higher than the gray pages' because
/// the reader's firmware does the rest (it tunes and dithers the picture for
/// its panel), and a visible loss in the source shows after that.
const COLOR_JPEG_QUALITY: u8 = 82;

/// How many pages one EPUB is cut at. A reader loads the whole list of pages
/// when it opens the book, and the small chips in some readers cannot hold a
/// long one, so a long series is made as several parts.
pub const MAX_PAGES: usize = 800;

/// A rough size of one page, for guessing how big a book will be.
pub fn typical_page_bytes(depth: Depth) -> u64 {
    match depth {
        Depth::One | Depth::Two => 22_000,
        Depth::Eight => 27_000,
    }
}

/// A rough size of one page kept in colour, for the same guess. Manga is mostly
/// gray even when it is in colour, which a JPEG stores cheaply.
pub fn typical_color_page_bytes() -> u64 {
    120_000
}

fn fail(message: impl std::fmt::Display) -> FormatError {
    FormatError::bad("EPUB", message)
}

/// One page as a picture: a PNG for pages of up to four grays, a JPEG for full grays.
pub fn encode_page(img: &LevelImage) -> Result<Vec<u8>> {
    match img.depth {
        Depth::Eight => encode_jpeg(img),
        depth => encode_png(img, depth),
    }
}

/// One page kept in colour, as a JPEG at quality 82: whatever the converter drew
/// (resized to the screen, no tone work, no dithering, no sharpening) is saved
/// as it is, for the reader to finish.
///
/// The encoder writes baseline JPEG without chroma subsampling (4:4:4), so
/// thin coloured lines keep their colour; the chroma of a mostly gray page
/// costs very little. A page with no colour at all (every pixel gray) is
/// saved as a grayscale JPEG, which is smaller still and looks the same.
pub fn encode_color_page(img: &ColorImage) -> Result<Vec<u8>> {
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

fn encode_jpeg(img: &LevelImage) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(img.levels.len() / 8);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(&img.levels, img.width, img.height, image::ExtendedColorType::L8)
        .map_err(fail)?;
    Ok(out)
}

/// An indexed PNG with the fewest bits the grays need.
fn encode_png(img: &LevelImage, depth: Depth) -> Result<Vec<u8>> {
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

/// An EPUB being written, page by page, to `W` (a file, or memory).
pub struct EpubBuilder<W: Write + Seek> {
    zip: ZipWriter<W>,
    meta: EpubMeta,
    /// The picture type of each page, by page: `png` or `jpg`.
    kinds: Vec<&'static str>,
    chapters: Vec<(String, usize)>,
}

impl<W: Write + Seek> EpubBuilder<W> {
    /// Start a book written to `out`.
    pub fn new(out: W, meta: EpubMeta) -> Result<Self> {
        let mut zip = ZipWriter::new(out);
        // The type of the book comes first and is not compressed, which is how
        // readers tell what a zip is without opening it.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored).map_err(fail)?;
        zip.write_all(b"application/epub+zip")?;
        let mut book = Self {
            zip,
            meta,
            kinds: Vec::new(),
            chapters: Vec::new(),
        };
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

    fn text(&mut self, name: &str, body: &str) -> Result<()> {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        self.zip.start_file(name, options).map_err(fail)?;
        Ok(self.zip.write_all(body.as_bytes())?)
    }

    /// Add a page that [`encode_page`] made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<()> {
        let kind = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else if picture.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            return Err(fail("a page is neither a PNG nor a JPEG picture"));
        };
        let n = self.kinds.len() + 1;
        // Pictures are compressed already: storing them is as small as it gets
        // and costs the reader nothing to unpack.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        self.zip
            .start_file(format!("OEBPS/{n}.{kind}"), stored)
            .map_err(fail)?;
        self.zip.write_all(picture)?;
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

    /// Write the contents and the package, and give back what the book was written to.
    pub fn finish(mut self) -> Result<W> {
        if self.kinds.is_empty() {
            return Err(fail("the book has no pages"));
        }
        let nav = self.nav();
        self.text("OEBPS/n.xhtml", &nav)?;
        let opf = self.package();
        self.text("OEBPS/c.opf", &opf)?;
        let mut writer = self.zip.finish().map_err(fail)?;
        writer.flush()?;
        Ok(writer)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn colour(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> ColorImage {
        let mut rgb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgb.extend(f(x, y));
            }
        }
        ColorImage { width: w, height: h, rgb }
    }

    #[test]
    fn a_colour_page_is_a_baseline_jpeg_that_keeps_its_colours_and_a_gray_one_is_a_gray_jpeg() {
        let page = colour(64, 48, |x, _| if x < 32 { [230, 30, 30] } else { [30, 40, 220] });
        let jpeg = encode_color_page(&page).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8, 0xFF]));
        let back = image::load_from_memory(&jpeg).unwrap();
        assert!(matches!(back, image::DynamicImage::ImageRgb8(_)));
        let rgb = back.to_rgb8();
        let [r, g, b] = rgb.get_pixel(8, 24).0;
        assert!(r > 200 && g < 60 && b < 60, "{:?}", [r, g, b]);
        let [r, g, b] = rgb.get_pixel(56, 24).0;
        assert!(b > 190 && r < 60 && g < 70, "{:?}", [r, g, b]);
        // No chroma subsampling: the sampling factors are all 1x1.
        let sof = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).expect("a baseline frame");
        assert_eq!(&jpeg[sof + 11..sof + 12], &[0x11], "luma sampling");
        assert_eq!(&jpeg[sof + 14..sof + 15], &[0x11], "Cb sampling");
        assert_eq!(&jpeg[sof + 17..sof + 18], &[0x11], "Cr sampling");

        let gray = colour(64, 48, |x, _| [x as u8 * 3; 3]);
        let jpeg = encode_color_page(&gray).unwrap();
        assert!(matches!(image::load_from_memory(&jpeg).unwrap(), image::DynamicImage::ImageLuma8(_)));
        assert!(jpeg.len() < encode_color_page(&colour(64, 48, |x, _| [x as u8 * 3, 0, 9])).unwrap().len());
        // EPUB and CBZ writers take it as a jpg.
        assert!(typical_color_page_bytes() > typical_page_bytes(Depth::Eight));
    }

    fn levels(depth: Depth, w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> LevelImage {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.push(f(x, y));
            }
        }
        LevelImage {
            width: w,
            height: h,
            depth,
            levels: v,
        }
    }

    fn in_memory() -> std::io::Cursor<Vec<u8>> {
        std::io::Cursor::new(Vec::new())
    }

    #[test]
    fn dates_are_written_the_way_epub_wants_them() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(1_781_000_000), "2026-06-09T10:13:20Z");
    }

    #[test]
    fn text_is_made_safe_for_xml() {
        assert_eq!(escape("Tom & <Jerry> \"x\""), "Tom &amp; &lt;Jerry&gt; &quot;x&quot;");
        assert_eq!(escape("a\u{1}b"), "ab");
    }

    #[test]
    fn four_grays_become_a_two_bit_png_that_reads_back_exactly() {
        let img = levels(Depth::Two, 13, 7, |x, y| ((x + y * 3) % 4) as u8);
        let png_bytes = encode_page(&img).unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
        decoder.set_transformations(png::Transformations::IDENTITY);
        let mut reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().bit_depth, png::BitDepth::Two);
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        let mut buf = vec![0; reader.output_buffer_size().expect("the buffer size fits in memory")];
        let frame = reader.next_frame(&mut buf).unwrap();
        let row = frame.line_size;
        for y in 0..7usize {
            for x in 0..13usize {
                let got = (buf[y * row + x / 4] >> (6 - (x % 4) * 2)) & 3;
                assert_eq!(got, img.levels[y * 13 + x], "({x},{y})");
            }
        }
    }

    #[test]
    fn full_grays_become_a_small_jpeg() {
        let img = levels(Depth::Eight, 120, 200, |x, y| ((x * 2 + y) % 256) as u8);
        let jpeg = encode_page(&img).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let back = image::load_from_memory(&jpeg).unwrap().to_luma8();
        assert_eq!((back.width(), back.height()), (120, 200));
        assert!(jpeg.len() < 120 * 200 / 2);
    }

    #[test]
    fn a_book_is_a_valid_fixed_layout_epub_with_its_chapters() {
        let mut book = EpubBuilder::new(
            in_memory(),
            EpubMeta {
                title: "My <Series>".into(),
                language: "ja".into(),
                rtl: true,
                width: 480,
                height: 800,
                created: 1_781_000_000,
            },
        )
        .unwrap();
        let page = encode_page(&levels(Depth::Two, 480, 800, |x, _| (x / 120) as u8)).unwrap();
        for _ in 0..3 {
            book.add_page(&page).unwrap();
        }
        book.add_page(&encode_page(&levels(Depth::Eight, 480, 800, |x, _| (x / 2) as u8)).unwrap()).unwrap();
        book.push_chapter("Chapter 1", 1);
        book.push_chapter("Chapter 2 & more", 3);
        assert_eq!(book.page_count(), 4);
        let file = book.finish().unwrap();

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(file.into_inner())).unwrap();
        // The type of the book is the first entry, stored as it is.
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);
        let mut read = |name: &str| {
            let mut s = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let opf = read("OEBPS/c.opf");
        assert!(opf.contains("pre-paginated") && opf.contains("page-progression-direction=\"rtl\""));
        assert!(opf.contains("<dc:title>My &lt;Series&gt;</dc:title>"));
        assert!(opf.contains("<dc:language>ja</dc:language>"));
        assert!(opf.contains("href=\"1.png\" media-type=\"image/png\" properties=\"cover-image\""));
        assert!(opf.contains("href=\"4.jpg\" media-type=\"image/jpeg\""));
        assert_eq!(opf.matches("<itemref").count(), 4);
        assert!(opf.contains("2026-06-09T10:13:20Z"));
        let nav = read("OEBPS/n.xhtml");
        assert!(nav.contains("<a href=\"2.xhtml\">Chapter 1</a>"));
        assert!(nav.contains("<a href=\"4.xhtml\">Chapter 2 &amp; more</a>"));
        let page = read("OEBPS/2.xhtml");
        assert!(page.contains("width=480,height=800") && page.contains("src=\"2.png\""));
        // Pictures are stored, text is deflated.
        assert_eq!(zip.by_name("OEBPS/1.png").unwrap().compression(), CompressionMethod::Stored);
        assert_eq!(zip.by_name("OEBPS/c.opf").unwrap().compression(), CompressionMethod::Deflated);
    }

    #[test]
    fn a_book_without_pages_is_refused_and_odd_pictures_are_too() {
        let meta = EpubMeta {
            title: "x".into(),
            language: String::new(),
            rtl: false,
            width: 10,
            height: 10,
            created: 0,
        };
        let mut book = EpubBuilder::new(in_memory(), meta).unwrap();
        assert!(book.add_page(b"GIF89a").is_err());
        assert!(book.finish().is_err());
    }

    #[test]
    fn a_page_of_four_grays_is_far_smaller_than_an_xtch_page() {
        // Flat areas and lines, like a manga page.
        let img = levels(Depth::Two, 480, 800, |x, y| {
            if (x / 40 + y / 40) % 5 == 0 { 0 } else if y % 97 < 3 { 1 } else { 3 }
        });
        let png = encode_page(&img).unwrap();
        assert!(png.len() < 480 * 800 / 4 / 3, "{} bytes", png.len());
    }
}
