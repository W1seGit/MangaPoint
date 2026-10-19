//! Making a PDF of page pictures, written as the pages arrive.
//!
//! A small writer made for one job: one picture per page, the page exactly the
//! size of the picture (one pixel is one point), a title and author, and an
//! outline with an entry for each chapter. A JPEG goes in as it is
//! (`DCTDecode`, never decoded or encoded again); other pictures go in as raw
//! pixels, compressed without loss (`FlateDecode`).
//!
//! Every page is written to the file at once, so a book never sits in memory:
//! only the offsets of the objects are kept, for the cross-reference table at
//! the end.

use crate::error::{PipelineError, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// What the document says about itself.
#[derive(Clone, Debug, Default)]
pub struct PdfMeta {
    pub title: String,
    pub author: String,
    /// Pages turn right to left (a hint for viewers that care).
    pub rtl: bool,
    /// Seconds since the Unix epoch, for the creation date; 0 leaves it out.
    pub created: u64,
}

/// The colors of a picture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfColor {
    Gray,
    Rgb,
}

impl PdfColor {
    fn name(self) -> &'static str {
        match self {
            PdfColor::Gray => "/DeviceGray",
            PdfColor::Rgb => "/DeviceRGB",
        }
    }

    pub fn channels(self) -> usize {
        match self {
            PdfColor::Gray => 1,
            PdfColor::Rgb => 3,
        }
    }
}

/// A picture ready to be a page.
pub enum PdfImage<'a> {
    /// A baseline or progressive JPEG with 8 bits per channel, exactly as stored.
    Jpeg {
        data: &'a [u8],
        width: u32,
        height: u32,
        color: PdfColor,
    },
    /// 8-bit pixels, row after row, with no padding.
    Raw {
        pixels: &'a [u8],
        width: u32,
        height: u32,
        color: PdfColor,
    },
}

/// What a JPEG says about its picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegInfo {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    pub precision: u8,
}

/// Read a JPEG's size and channels from its header, without decoding it.
/// `None` for anything that is not a JPEG with a readable frame header.
pub fn jpeg_info(data: &[u8]) -> Option<JpegInfo> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            return None;
        }
        // Fill bytes before a marker.
        while i < data.len() && data[i] == 0xff {
            i += 1;
        }
        let marker = *data.get(i)?;
        i += 1;
        match marker {
            // Markers without a length.
            0x01 | 0xd0..=0xd8 => continue,
            0xd9 | 0xda => return None,
            _ => {}
        }
        let len = u16::from_be_bytes([*data.get(i)?, *data.get(i + 1)?]) as usize;
        if len < 2 {
            return None;
        }
        // SOF0..SOF15 except DHT (c4), JPG (c8) and DAC (cc).
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let f = data.get(i + 2..i + 2 + 6)?;
            return Some(JpegInfo {
                precision: f[0],
                height: u32::from(u16::from_be_bytes([f[1], f[2]])),
                width: u32::from(u16::from_be_bytes([f[3], f[4]])),
                components: f[5],
            });
        }
        i += len;
    }
    None
}

/// A PDF being written.
pub struct PdfBuilder {
    out: Counting<BufWriter<File>>,
    path: PathBuf,
    meta: PdfMeta,
    /// Where each object starts; index 0 is object 1.
    offsets: Vec<u64>,
    /// The object number of each page.
    pages: Vec<u32>,
    chapters: Vec<(String, usize)>,
}

/// Counts what is written, so offsets are known without asking the file.
struct Counting<W> {
    inner: W,
    count: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// The object numbers fixed before the pages: the catalog and the page tree.
const CATALOG: u32 = 1;
const PAGE_TREE: u32 = 2;

impl PdfBuilder {
    /// Start a document at `path`.
    pub fn create(path: &Path, meta: PdfMeta) -> Result<Self> {
        let file = File::create(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        let mut out = Counting {
            inner: BufWriter::new(file),
            count: 0,
        };
        // The second line holds bytes above 127, which tells tools this is binary.
        out.write_all(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
            .map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        Ok(Self {
            out,
            path: path.to_path_buf(),
            meta,
            // The catalog and page tree are written last; their places are kept.
            offsets: vec![0, 0],
            pages: Vec::new(),
            chapters: Vec::new(),
        })
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize) {
        self.chapters.push((name.to_string(), start));
    }

    fn io(&self) -> impl Fn(std::io::Error) -> PipelineError + '_ {
        |e| PipelineError::Io(self.path.clone(), e)
    }

    /// Start an object and return its number.
    fn begin(&mut self) -> Result<u32> {
        let number = self.offsets.len() as u32 + 1;
        self.offsets.push(self.out.count);
        writeln!(self.out, "{number} 0 obj").map_err(self.io())?;
        Ok(number)
    }

    fn end(&mut self) -> Result<()> {
        self.out.write_all(b"\nendobj\n").map_err(self.io())
    }

    /// Add a page that is exactly the size of the picture.
    pub fn add_page(&mut self, image: PdfImage<'_>) -> Result<()> {
        let (width, height, color) = match &image {
            PdfImage::Jpeg { width, height, color, .. } | PdfImage::Raw { width, height, color, .. } => {
                (*width, *height, *color)
            }
        };
        if width == 0 || height == 0 {
            return Err(PipelineError::Epub("PDF: a picture has no size".into()));
        }
        // Object numbers of this page: its picture, its content, the page itself.
        let picture = self.begin()?;
        let (filter, extra, data): (&str, String, std::borrow::Cow<'_, [u8]>) = match image {
            PdfImage::Jpeg { data, .. } => ("/DCTDecode", String::new(), data.into()),
            PdfImage::Raw { pixels, .. } => {
                let packed = deflate_rows(pixels, width as usize * color.channels(), height as usize)
                    .map_err(self.io())?;
                (
                    "/FlateDecode",
                    format!(
                        " /DecodeParms << /Predictor 15 /Colors {} /BitsPerComponent 8 /Columns {width} >>",
                        color.channels()
                    ),
                    packed.into(),
                )
            }
        };
        write!(
            self.out,
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {} /BitsPerComponent 8 /Filter {filter}{extra} /Length {} >>\nstream\n",
            color.name(),
            data.len()
        )
        .map_err(self.io())?;
        self.out.write_all(&data).map_err(self.io())?;
        self.out.write_all(b"\nendstream").map_err(self.io())?;
        self.end()?;

        let content = self.begin()?;
        let draw = format!("q {width} 0 0 {height} 0 0 cm /Im0 Do Q");
        write!(self.out, "<< /Length {} >>\nstream\n{draw}\nendstream", draw.len()).map_err(self.io())?;
        self.end()?;

        let page = self.begin()?;
        write!(
            self.out,
            "<< /Type /Page /Parent {PAGE_TREE} 0 R /MediaBox [0 0 {width} {height}] /Resources << /XObject << /Im0 {picture} 0 R >> >> /Contents {content} 0 R >>"
        )
        .map_err(self.io())?;
        self.end()?;
        self.pages.push(page);
        Ok(())
    }

    /// Write the outline, the page tree, the catalog and the cross-reference
    /// table, and close the file.
    pub fn finish(mut self) -> Result<()> {
        if self.pages.is_empty() {
            return Err(PipelineError::Epub("PDF: the book has no pages".into()));
        }
        // The outline: one entry for each chapter, in order, ending where the next begins.
        let chapters: Vec<(String, u32)> = self
            .chapters
            .iter()
            .filter_map(|(name, start)| Some((name.clone(), *self.pages.get(*start)?)))
            .collect();
        let first_item = self.offsets.len() as u32 + 1;
        let outline_root = first_item + chapters.len() as u32;
        for (i, (name, page)) in chapters.iter().enumerate() {
            let n = self.begin()?;
            debug_assert_eq!(n, first_item + i as u32);
            let mut links = String::new();
            if i > 0 {
                links.push_str(&format!(" /Prev {} 0 R", n - 1));
            }
            if i + 1 < chapters.len() {
                links.push_str(&format!(" /Next {} 0 R", n + 1));
            }
            write!(
                self.out,
                "<< /Title {} /Parent {outline_root} 0 R{links} /Dest [{page} 0 R /Fit] >>",
                text_string(name)
            )
            .map_err(self.io())?;
            self.end()?;
        }
        let outlines = if chapters.is_empty() {
            None
        } else {
            let n = self.begin()?;
            debug_assert_eq!(n, outline_root);
            write!(
                self.out,
                "<< /Type /Outlines /First {first_item} 0 R /Last {} 0 R /Count {} >>",
                outline_root - 1,
                chapters.len()
            )
            .map_err(self.io())?;
            self.end()?;
            Some(outline_root)
        };

        // The page tree (object 2).
        self.offsets[PAGE_TREE as usize - 1] = self.out.count;
        let kids: Vec<String> = self.pages.iter().map(|p| format!("{p} 0 R")).collect();
        write!(
            self.out,
            "{PAGE_TREE} 0 obj\n<< /Type /Pages /Count {} /Kids [{}] >>\nendobj\n",
            self.pages.len(),
            kids.join(" ")
        )
        .map_err(self.io())?;

        // The catalog (object 1).
        self.offsets[CATALOG as usize - 1] = self.out.count;
        let mut catalog = format!("<< /Type /Catalog /Pages {PAGE_TREE} 0 R");
        if let Some(o) = outlines {
            catalog.push_str(&format!(" /Outlines {o} 0 R /PageMode /UseOutlines"));
        }
        if self.meta.rtl {
            catalog.push_str(" /ViewerPreferences << /Direction /R2L >>");
        }
        catalog.push_str(" >>");
        write!(self.out, "{CATALOG} 0 obj\n{catalog}\nendobj\n").map_err(self.io())?;

        // Who made it.
        let info = self.begin()?;
        let mut dict = format!("<< /Title {} /Creator (MangaPoint) /Producer (MangaPoint)", text_string(&self.meta.title));
        if !self.meta.author.trim().is_empty() {
            dict.push_str(&format!(" /Author {}", text_string(&self.meta.author)));
        }
        if self.meta.created > 0 {
            dict.push_str(&format!(" /CreationDate ({})", pdf_date(self.meta.created)));
        }
        dict.push_str(" >>");
        write!(self.out, "{dict}").map_err(self.io())?;
        self.end()?;

        // The cross-reference table: every entry is exactly 20 bytes.
        let xref = self.out.count;
        let size = self.offsets.len() + 1;
        write!(self.out, "xref\n0 {size}\n0000000000 65535 f \n").map_err(self.io())?;
        for offset in &self.offsets {
            writeln!(self.out, "{offset:010} 00000 n ").map_err(self.io())?;
        }
        write!(
            self.out,
            "trailer\n<< /Size {size} /Root {CATALOG} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .map_err(self.io())?;
        self.out.flush().map_err(self.io())
    }
}

/// Compress pixel rows with the PNG "Up" filter in front of each (so lines that
/// repeat the row above shrink to almost nothing), as `/Predictor 15` expects.
fn deflate_rows(pixels: &[u8], row: usize, rows: usize) -> std::io::Result<Vec<u8>> {
    if pixels.len() != row * rows {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "pixel data does not match the picture size"));
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
    let mut line = vec![0u8; row + 1];
    line[0] = 2;
    for y in 0..rows {
        let current = &pixels[y * row..(y + 1) * row];
        if y == 0 {
            line[1..].copy_from_slice(current);
        } else {
            let above = &pixels[(y - 1) * row..y * row];
            for ((o, c), a) in line[1..].iter_mut().zip(current).zip(above) {
                *o = c.wrapping_sub(*a);
            }
        }
        z.write_all(&line)?;
    }
    z.finish()
}

/// A text string for a title or a name: plain ASCII in parentheses, anything
/// else as UTF-16 in hexadecimal, which every viewer reads.
fn text_string(s: &str) -> String {
    let s = s.trim();
    if s.is_ascii() && !s.chars().any(char::is_control) {
        let mut out = String::from("(");
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for unit in s.encode_utf16() {
            out.push_str(&format!("{unit:04X}"));
        }
        out.push('>');
        out
    }
}

/// `D:20250131120000Z` for a time in seconds since the Unix epoch.
fn pdf_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
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
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_escaped_or_sent_as_utf16() {
        assert_eq!(text_string("Solo (Leveling)"), "(Solo \\(Leveling\\))");
        assert_eq!(text_string("進撃"), "<FEFF90326483>");
        assert_eq!(text_string("a\\b"), "(a\\\\b)");
    }

    #[test]
    fn dates_are_written_in_pdf_form() {
        assert_eq!(pdf_date(0), "D:19700101000000Z");
        assert_eq!(pdf_date(1_700_000_000), "D:20231114221320Z");
    }

    #[test]
    fn jpeg_headers_are_read_without_decoding() {
        // SOI, an APP0 segment, then SOF0: 8 bits, 5 high, 7 wide, 3 channels.
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 8, 0, 5, 0, 7, 3]);
        assert_eq!(
            jpeg_info(&jpeg),
            Some(JpegInfo { width: 7, height: 5, components: 3, precision: 8 })
        );
        assert_eq!(jpeg_info(b"\x89PNG"), None);
        assert_eq!(jpeg_info(&[0xff, 0xd8, 0xff]), None);
        assert_eq!(jpeg_info(&[]), None);
    }
}
