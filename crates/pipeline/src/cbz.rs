//! Making a CBZ: a comic book archive.
//!
//! A CBZ is a zip of page pictures named so that they sort in reading order,
//! plus a `ComicInfo.xml` that says what the book is. The pictures are the same
//! ones an EPUB holds (see [`crate::epub::encode_page`]), exactly the size of
//! the reader's screen, and are stored in the zip as they are because they are
//! already compressed.
//!
//! `ComicInfo.xml` carries what readers and comic libraries (Kobo, PocketBook,
//! KOReader, Kavita, Komga and others) understand:
//!
//! * the series, the title of this file, the chapter number when it holds one
//!   chapter, a summary, the writer and the artist, the genres, the language,
//!   whether it is a manga and so whether it is read right to left;
//! * a `<Page>` for every picture, with its size, the cover marked as such, and
//!   a `Bookmark` on the first page of each chapter, which is how a comic
//!   archive lists its chapters.
//!
//! Pages are written to the file as they arrive, so a book does not have to fit
//! in memory.

use crate::error::{PipelineError, Result};
use std::fs::File;
use std::io::{BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fail(message: impl std::fmt::Display) -> PipelineError {
    PipelineError::Epub(format!("CBZ: {message}"))
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
    zip: ZipWriter<BufWriter<File>>,
    path: PathBuf,
    meta: CbzMeta,
    pages: Vec<Page>,
    chapters: Vec<Chapter>,
}

impl CbzBuilder {
    /// Start a book at `path`.
    pub fn create(path: &Path, meta: CbzMeta) -> Result<Self> {
        let file = File::create(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        Ok(Self {
            zip: ZipWriter::new(BufWriter::new(file)),
            path: path.to_path_buf(),
            meta,
            pages: Vec::new(),
            chapters: Vec::new(),
        })
    }

    /// How many pages the book has so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Add a page that [`crate::epub::encode_page`] made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<()> {
        let kind = if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else if picture.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            return Err(fail("a page is neither a PNG nor a JPEG picture"));
        };
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
        let path = self.path.clone();
        self.zip
            .write_all(picture)
            .map_err(|e| PipelineError::Io(path, e))?;
        self.pages.push(Page {
            size: picture.len(),
            width,
            height,
        });
        Ok(())
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize, number: Option<f64>) {
        self.chapters.push(Chapter {
            name: name.to_string(),
            start,
            number,
        });
    }

    /// Write `ComicInfo.xml` and close the file.
    pub fn finish(mut self) -> Result<()> {
        if self.pages.is_empty() {
            return Err(fail("the book has no pages"));
        }
        let xml = self.comic_info();
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        self.zip.start_file("ComicInfo.xml", options).map_err(fail)?;
        let path = self.path.clone();
        self.zip
            .write_all(xml.as_bytes())
            .map_err(|e| PipelineError::Io(path.clone(), e))?;
        let mut writer = self.zip.finish().map_err(fail)?;
        writer.flush().map_err(|e| PipelineError::Io(path, e))
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
        tag("Notes", &if m.status.is_empty() {
            notes
        } else {
            format!("{notes} Status: {}.", m.status)
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use converter::{Depth, LevelImage};
    use std::io::Read;

    fn page(depth: Depth, w: u32, h: u32) -> Vec<u8> {
        let levels = (0..w * h).map(|i| (i % 4) as u8).collect();
        crate::epub::encode_page(&LevelImage { width: w, height: h, depth, levels }).unwrap()
    }

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mangapoint-cbz-{tag}-{}.cbz", std::process::id()))
    }

    fn meta() -> CbzMeta {
        CbzMeta {
            series: "Moonlit & Cafe".into(),
            language: "en".into(),
            rtl: true,
            summary: "A <tiny> story.".into(),
            author: "A. Writer".into(),
            artist: "B. Artist".into(),
            genres: vec!["Drama".into(), "Slice of Life".into()],
            status: "Ongoing".into(),
            sources: vec!["Source A".into()],
            created: 1_792_108_800, // 2026-10-16
        }
    }

    fn open(path: &Path) -> zip::ZipArchive<File> {
        zip::ZipArchive::new(File::open(path).unwrap()).unwrap()
    }

    #[test]
    fn a_cbz_is_ordered_pictures_and_a_comic_info() {
        let path = temp("book");
        let mut b = CbzBuilder::create(&path, meta()).unwrap();
        for _ in 0..3 {
            b.add_page(&page(Depth::Two, 48, 80)).unwrap();
        }
        b.add_page(&page(Depth::Eight, 48, 80)).unwrap();
        b.push_chapter("Ch. 1 - Start", 0, Some(1.0));
        b.push_chapter("Ch. 2", 2, Some(2.0));
        assert_eq!(b.page_count(), 4);
        b.finish().unwrap();

        let mut z = open(&path);
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        assert_eq!(names, ["00001.png", "00002.png", "00003.png", "00004.jpg", "ComicInfo.xml"]);
        // Pictures are stored, not deflated again.
        assert_eq!(z.by_name("00001.png").unwrap().compression(), CompressionMethod::Stored);
        let mut xml = String::new();
        z.by_name("ComicInfo.xml").unwrap().read_to_string(&mut xml).unwrap();
        for want in [
            "<Series>Moonlit &amp; Cafe</Series>",
            "<Title>Moonlit &amp; Cafe</Title>",
            "<Summary>A &lt;tiny&gt; story.</Summary>",
            "<Year>2026</Year>",
            "<Writer>A. Writer</Writer>",
            "<Penciller>B. Artist</Penciller>",
            "<Genre>Drama, Slice of Life</Genre>",
            "<PageCount>4</PageCount>",
            "<LanguageISO>en</LanguageISO>",
            "<Manga>YesAndRightToLeft</Manga>",
            "<Page Image=\"0\" Type=\"FrontCover\" Bookmark=\"Ch. 1 - Start\"",
            "<Page Image=\"2\" Bookmark=\"Ch. 2\"",
            "ImageWidth=\"48\" ImageHeight=\"80\"",
        ] {
            assert!(xml.contains(want), "missing {want} in\n{xml}");
        }
        assert!(!xml.contains("<Number>"), "several chapters are not one issue");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_file_of_one_chapter_is_that_issue() {
        let path = temp("one");
        let mut b = CbzBuilder::create(&path, CbzMeta { series: "S".into(), ..Default::default() }).unwrap();
        b.add_page(&page(Depth::Two, 16, 16)).unwrap();
        b.push_chapter("Ch. 12.5 - Half", 0, Some(12.5));
        b.finish().unwrap();
        let mut xml = String::new();
        open(&path).by_name("ComicInfo.xml").unwrap().read_to_string(&mut xml).unwrap();
        assert!(xml.contains("<Title>Ch. 12.5 - Half</Title>") && xml.contains("<Number>12.5</Number>"), "{xml}");
        assert!(xml.contains("<Manga>No</Manga>"), "left to right is not a manga");
        assert!(!xml.contains("<Summary>"), "empty details are left out");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_book_with_no_pages_is_an_error_and_a_wrong_picture_is_refused() {
        let path = temp("empty");
        let b = CbzBuilder::create(&path, CbzMeta::default()).unwrap();
        assert!(b.finish().is_err());
        let mut b = CbzBuilder::create(&path, CbzMeta::default()).unwrap();
        assert!(b.add_page(b"not a picture").is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn years_come_out_right() {
        assert_eq!(year_of(0), None);
        assert_eq!(year_of(1_792_108_800), Some(2026));
        assert_eq!(year_of(951_782_400), Some(2000)); // 2000-02-29
        assert_eq!(year_of(1_735_689_599), Some(2024)); // 2024-12-31 23:59:59
        assert_eq!(year_of(1_735_689_600), Some(2025));
    }
}
