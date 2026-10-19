//! A CBZ (comic book archive), built in memory.
//!
//! A copy of `crates/pipeline/src/cbz.rs` that writes into a `Vec<u8>`: same page
//! names, same `ComicInfo.xml`, same bookmarks, so the bytes are the same.

use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fail(message: impl std::fmt::Display) -> String {
    format!("CBZ: {message}")
}

/// What the book says about itself.
#[derive(Clone, Debug, Default)]
pub struct CbzMeta {
    /// The series.
    pub series: String,
    /// A language tag such as "en".
    pub language: String,
    /// Read right to left (manga).
    pub rtl: bool,
    pub summary: String,
    pub author: String,
    pub artist: String,
    pub genres: Vec<String>,
    /// "Ongoing", "Completed" and so on; empty when not known.
    pub status: String,
    /// Where the series came from, for the `Notes` line.
    pub sources: Vec<String>,
    /// Seconds since the Unix epoch.
    pub created: u64,
}

/// One chapter: its name, the 0-based page it starts at, and its number.
#[derive(Clone, Debug)]
struct Chapter {
    name: String,
    start: usize,
    number: Option<f64>,
}

/// One picture already in the archive.
struct Page {
    size: usize,
    width: u32,
    height: u32,
}

/// A CBZ being written, page by page.
pub struct CbzBuilder {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    meta: CbzMeta,
    pages: Vec<Page>,
    chapters: Vec<Chapter>,
}

impl CbzBuilder {
    pub fn new(meta: CbzMeta) -> Self {
        Self {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
            meta,
            pages: Vec::new(),
            chapters: Vec::new(),
        }
    }

    /// How many pages the book has so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Add a page that `epub::encode_page` made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<(), String> {
        let kind = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else if picture.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            return Err(fail("a page is neither a PNG nor a JPEG picture"));
        };
        self.add_named(picture, kind)
    }

    /// Add a page picture as it was saved: a JPEG, a PNG or a WebP, kept
    /// byte for byte.
    pub fn add_original(&mut self, picture: &[u8]) -> Result<(), String> {
        let kind = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else if picture.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else if picture.len() > 12 && &picture[..4] == b"RIFF" && &picture[8..12] == b"WEBP" {
            "webp"
        } else {
            return Err(fail("a page is not a JPEG, PNG or WebP picture"));
        };
        self.add_named(picture, kind)
    }

    fn add_named(&mut self, picture: &[u8], kind: &str) -> Result<(), String> {
        let (width, height) = image::ImageReader::new(Cursor::new(picture))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
            .unwrap_or((0, 0));
        // Five digits keep any number of pages in order, even for a reader that
        // sorts the names as plain text.
        let name = format!("{:05}.{kind}", self.pages.len() + 1);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        self.zip.start_file(name, stored).map_err(fail)?;
        self.zip.write_all(picture).map_err(fail)?;
        self.pages.push(Page { size: picture.len(), width, height });
        Ok(())
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize, number: Option<f64>) {
        self.chapters.push(Chapter { name: name.to_string(), start, number });
    }

    /// Write `ComicInfo.xml` and give the finished file.
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        if self.pages.is_empty() {
            return Err(fail("the book has no pages"));
        }
        let xml = self.comic_info();
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        self.zip.start_file("ComicInfo.xml", options).map_err(fail)?;
        self.zip.write_all(xml.as_bytes()).map_err(fail)?;
        Ok(self.zip.finish().map_err(fail)?.into_inner())
    }

    fn comic_info(&self) -> String {
        let m = &self.meta;
        let mut x = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\n",
        );
        let mut tag = |name: &str, value: &str| {
            if !value.trim().is_empty() {
                x.push_str(&format!("  <{name}>{}</{name}>\n", escape(value.trim())));
            }
        };
        // With a single chapter in the file, it is the issue: its name is the
        // title and its number the number. With several, the file is the series.
        match self.chapters.as_slice() {
            [one] => {
                tag("Title", &one.name);
                if let Some(n) = one.number {
                    tag("Number", &trim_number(n));
                }
            }
            _ => tag("Title", &m.series),
        }
        tag("Series", &m.series);
        tag("Summary", &m.summary);
        if let Some(year) = year_of(m.created) {
            tag("Year", &year.to_string());
        }
        tag("Writer", &m.author);
        tag("Penciller", &m.artist);
        tag("Genre", &m.genres.join(", "));
        tag("PageCount", &self.pages.len().to_string());
        tag("LanguageISO", &m.language);
        tag("Format", "Web");
        tag("Manga", if m.rtl { "YesAndRightToLeft" } else { "No" });
        let notes = if m.sources.is_empty() {
            "Made with MangaPoint".to_string()
        } else {
            format!("Made with MangaPoint. From {}.", m.sources.join(", "))
        };
        tag(
            "Notes",
            &if m.status.is_empty() {
                notes
            } else {
                format!("{notes} Status: {}.", m.status)
            },
        );
        x.push_str("  <Pages>\n");
        for (i, p) in self.pages.iter().enumerate() {
            let mut attrs = format!("Image=\"{i}\"");
            if i == 0 {
                attrs.push_str(" Type=\"FrontCover\"");
            }
            if let Some(c) = self.chapters.iter().find(|c| c.start == i) {
                attrs.push_str(&format!(" Bookmark=\"{}\"", escape(&c.name)));
            }
            attrs.push_str(&format!(
                " ImageSize=\"{}\" ImageWidth=\"{}\" ImageHeight=\"{}\"",
                p.size, p.width, p.height
            ));
            x.push_str(&format!("    <Page {attrs} />\n"));
        }
        x.push_str("  </Pages>\n</ComicInfo>\n");
        x
    }
}

/// 12 for 12.0, 12.5 for 12.5.
fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The calendar year of a time in seconds since the Unix epoch.
fn year_of(secs: u64) -> Option<u32> {
    if secs == 0 {
        return None;
    }
    // Days since 1970-01-01, then the civil year (Howard Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    u32::try_from(year).ok()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
