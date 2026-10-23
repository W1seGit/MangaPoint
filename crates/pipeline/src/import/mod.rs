//! Importing files into the library: comic archives (CBZ, ZIP, 7Z, RAR), EPUBs
//! and PDFs whose pages are pictures, folders and loose pictures, and TIFFs.
//!
//! An import has two steps, so the person can look before anything is written:
//!
//! 1. [`scan`] reads names and details, and reads each book's cover: it says what
//!    the books are, which chapters they hold and how many pages, and
//!    writes nothing.
//! 2. [`commit`] (after the person has corrected the titles, numbers and names)
//!    writes the pages into the library, in the same layout a download has, so
//!    previewing and exporting need nothing new.
//!
//! Pages are named by us (`00001.img`); names inside an archive are only ever
//! looked up, never used as a path on disk.

mod archive;
mod commit;
mod epub_in;
mod folder;
mod pdf_in;
mod rar_in;
mod sevenz_in;
mod tiff_in;
mod zip_in;

// Reading ComicInfo.xml and the names inside a book is shared with the website (`formats`).
use formats::{comicinfo, names};
pub use comicinfo::ComicInfo;
pub use commit::{commit, sweep_leftovers, CommitOutcome, CommitTarget};
pub use names::{chapter_name, chapter_number, clean_name, natural_cmp, plain, series_title};

use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use library::DownloadedChapter;
use std::path::{Path, PathBuf};

/// The most a single page may be once unpacked, and the most entries an
/// archive may hold: a defence against archives made to fill the disk.
pub(crate) const MAX_PAGE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 100_000;
/// The most one import may unpack in all.
pub(crate) const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024 * 1024;
/// The most an entry read only for the book's details or cover may be.
pub(crate) const MAX_SCAN_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
/// The most such entries may add up to, together, while a book is scanned.
pub(crate) const MAX_SCAN_TOTAL_BYTES: u64 = 96 * 1024 * 1024;
/// The most pixels a page may have: the same limit the converter applies, so a
/// page that would fail at every export is refused when it is imported.
pub const MAX_PAGE_PIXELS: u64 = converter::decode::MAX_MEGAPIXELS as u64 * 1_000_000;

/// Picture files a book's pages can be.
pub const PAGE_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "gif", "tif", "tiff"];

/// Everything the file picker offers.
pub const IMPORT_EXTENSIONS: [&str; 15] = [
    "cbz", "zip", "cbr", "rar", "cb7", "7z", "epub", "pdf", "jpg", "jpeg", "png", "webp", "gif",
    "tif", "tiff",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Images,
    Folder,
    Cbz,
    Zip,
    Epub,
    Pdf,
    SevenZ,
    Rar,
    Tiff,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Images => "Images",
            Format::Folder => "Folder",
            Format::Cbz => "CBZ",
            Format::Zip => "ZIP",
            Format::Epub => "EPUB",
            Format::Pdf => "PDF",
            Format::SevenZ => "7Z",
            Format::Rar => "RAR",
            Format::Tiff => "TIFF",
        }
    }
}

/// What a book says about itself, for the person to correct.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookMeta {
    pub title: String,
    pub description: String,
    pub author: String,
    pub artist: String,
    pub status: String,
    pub genres: Vec<String>,
    pub language: String,
    /// Read right to left, when the book says so.
    pub right_to_left: Option<bool>,
}

/// One page of a book: which of its sources holds it, and its name there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRef {
    pub source: usize,
    pub key: String,
}

/// Where a chapter goes in the series it is imported into.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// A chapter of its own.
    #[default]
    New,
    /// After the pages an existing chapter (its key) already has.
    Append(String),
    /// In place of an existing chapter.
    Replace(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportChapter {
    pub number: Option<f64>,
    pub name: String,
    pub pages: Vec<PageRef>,
    /// Leave the chapter out of the import.
    pub skip: bool,
    pub into: Placement,
    /// The key of an existing chapter with the same number, if there is one
    /// (set by [`ImportBook::meet`]).
    pub meets: Option<String>,
}

impl ImportChapter {
    pub fn new(number: Option<f64>, name: impl Into<String>, pages: Vec<PageRef>) -> Self {
        Self {
            number,
            name: name.into(),
            pages,
            skip: false,
            into: Placement::New,
            meets: None,
        }
    }
}

/// Where a book's pages are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookSource {
    /// A folder; keys are paths inside it (a TIFF page as `path#index`).
    Dir(PathBuf),
    /// A ZIP, CBZ or EPUB; keys are entry names.
    Zip(PathBuf),
    SevenZ(PathBuf),
    Rar(PathBuf),
    /// A PDF; keys are page numbers.
    Pdf(PathBuf),
}

/// One thing to import: a file, a folder, or a set of loose pictures.
#[derive(Clone, Debug)]
pub struct ImportBook {
    /// What it was called as a file, to show.
    pub label: String,
    pub format: Format,
    pub meta: BookMeta,
    pub chapters: Vec<ImportChapter>,
    /// The cover, as the bytes of a picture.
    pub cover: Option<Vec<u8>>,
    /// Things worth telling the person (pages that could not be read).
    pub warnings: Vec<String>,
    pub(crate) sources: Vec<BookSource>,
}

impl ImportBook {
    pub(crate) fn new(label: impl Into<String>, format: Format, source: BookSource) -> Self {
        Self {
            label: label.into(),
            format,
            meta: BookMeta::default(),
            chapters: Vec::new(),
            cover: None,
            warnings: Vec::new(),
            sources: vec![source],
        }
    }

    pub fn page_count(&self) -> usize {
        self.chapters.iter().map(|c| c.pages.len()).sum()
    }

    /// The pages that will be imported.
    pub fn included_pages(&self) -> usize {
        self.chapters
            .iter()
            .filter(|c| !c.skip)
            .map(|c| c.pages.len())
            .sum()
    }

    /// The files and folders it came from, by name.
    pub fn source_names(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter_map(|s| match s {
                BookSource::Dir(p)
                | BookSource::Zip(p)
                | BookSource::SevenZ(p)
                | BookSource::Rar(p)
                | BookSource::Pdf(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
            })
            .collect()
    }

    /// Several books as one: their chapters in order, the first details that
    /// say anything, and the first cover.
    pub fn merge(books: Vec<ImportBook>) -> Option<ImportBook> {
        let mut books = books.into_iter();
        let mut merged = books.next()?;
        for book in books {
            let offset = merged.sources.len();
            merged.sources.extend(book.sources);
            for mut chapter in book.chapters {
                for page in &mut chapter.pages {
                    page.source += offset;
                }
                merged.chapters.push(chapter);
            }
            let (m, b) = (&mut merged.meta, book.meta);
            for (have, new) in [
                (&mut m.title, b.title),
                (&mut m.description, b.description),
                (&mut m.author, b.author),
                (&mut m.artist, b.artist),
                (&mut m.status, b.status),
                (&mut m.language, b.language),
            ] {
                if have.is_empty() {
                    *have = new;
                }
            }
            if m.genres.is_empty() {
                m.genres = b.genres;
            }
            m.right_to_left = m.right_to_left.or(b.right_to_left);
            if merged.cover.is_none() {
                merged.cover = book.cover;
            }
            merged.warnings.extend(book.warnings);
            merged.label = format!("{} + {}", merged.label, book.label);
        }
        merged.renumber_repeats();
        Some(merged)
    }

    /// Books put together can each have a "Chapter 1". A chapter whose number
    /// an earlier one has is given a fresh number after the highest, so they
    /// stay separate chapters in the order they were put together.
    fn renumber_repeats(&mut self) {
        let mut seen: Vec<f64> = Vec::new();
        for chapter in &mut self.chapters {
            match chapter.number {
                Some(n) if seen.iter().any(|s| (s - n).abs() < 0.0005) => {
                    if chapter.name == format!("Chapter {}", plain(n)) {
                        chapter.name.clear();
                    }
                    chapter.number = None;
                }
                Some(n) => seen.push(n),
                None => {}
            }
        }
        self.number_chapters(&[]);
    }

    /// Give the chapters that have no number one, after the numbers that are
    /// taken (`taken`: those of the series it goes into, none for a new one).
    /// A book whose chapters are all unnumbered is numbered from 1 in order
    /// (from after the last taken number when it goes into a series).
    pub fn number_chapters(&mut self, taken: &[f64]) {
        let mut next = taken
            .iter()
            .copied()
            .chain(self.chapters.iter().filter_map(|c| c.number))
            .fold(0.0_f64, f64::max)
            .floor()
            + 1.0;
        for chapter in self.chapters.iter_mut().filter(|c| c.number.is_none()) {
            chapter.number = Some(next);
            if chapter.name.is_empty() || chapter.name == "Chapter" {
                chapter.name = format!("Chapter {}", plain(next));
            }
            next += 1.0;
        }
    }

    /// Note which chapters meet one the series already has (the same number).
    /// Those are left out unless the person decides otherwise, so importing
    /// the same file twice does nothing twice.
    pub fn meet(&mut self, existing: &[DownloadedChapter]) {
        for chapter in &mut self.chapters {
            chapter.meets = chapter.number.and_then(|n| {
                existing
                    .iter()
                    .find(|e| e.number.is_some_and(|m| (m - n).abs() < 0.0005))
                    .map(|e| e.key.clone())
            });
            if chapter.meets.is_some() && chapter.into == Placement::New {
                chapter.skip = true;
            }
        }
    }

    /// Like [`ImportBook::meet`], for the chapters that have not been looked at
    /// yet (no decision recorded): a chapter the person already decided about
    /// keeps what they chose.
    pub fn meet_undecided(&mut self, existing: &[DownloadedChapter]) {
        for chapter in self.chapters.iter_mut().filter(|c| c.meets.is_none()) {
            let key = chapter.number.and_then(|n| {
                existing
                    .iter()
                    .find(|e| e.number.is_some_and(|m| (m - n).abs() < 0.0005))
                    .map(|e| e.key.clone())
            });
            if key.is_some() {
                chapter.meets = key;
                chapter.skip = true;
            }
        }
    }
}

/// Save a series' cover where the library keeps it: a temporary file renamed
/// into place, so a crash never leaves half a picture. The folder is made if
/// it is not there.
pub fn save_cover(dest: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| PipelineError::Io(dir.to_path_buf(), e))?;
    }
    crate::download::write_atomically(dest, bytes)
}

/// What identifies the files a book was read from (path, size and time of
/// each file; a folder by its path and page count), to notice the same book
/// being imported twice. Loose pictures have none.
pub fn book_fingerprints(book: &ImportBook) -> Vec<String> {
    book.sources
        .iter()
        .filter_map(|source| match source {
            BookSource::Zip(p) | BookSource::SevenZ(p) | BookSource::Rar(p) | BookSource::Pdf(p) => {
                let meta = std::fs::metadata(p).ok()?;
                let time = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                Some(format!("{}|{}|{}", p.display(), meta.len(), time))
            }
            BookSource::Dir(p) if book.format == Format::Folder => {
                Some(format!("{}|folder|{}", p.display(), book.page_count()))
            }
            BookSource::Dir(_) => None,
        })
        .collect()
}

/// The identity of each file a book was read from (see `formats::fingerprint`): the same one the
/// website gives the same file, so a reading-state file made by either finds the book in the other.
/// Folders and loose pictures have none.
pub fn content_prints(book: &ImportBook) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let one = |p: &std::path::Path| -> Option<String> {
        let mut file = std::fs::File::open(p).ok()?;
        let size = file.metadata().ok()?.len();
        let n = formats::fingerprint::HEAD_TAIL_BYTES as u64;
        let mut head = vec![0u8; size.min(n) as usize];
        file.read_exact(&mut head).ok()?;
        let mut tail = vec![0u8; size.min(n) as usize];
        file.seek(SeekFrom::Start(size.saturating_sub(n))).ok()?;
        file.read_exact(&mut tail).ok()?;
        Some(formats::fingerprint::book_fingerprint_head_tail(size, &head, &tail))
    };
    book.sources
        .iter()
        .filter_map(|source| match source {
            BookSource::Zip(p) | BookSource::SevenZ(p) | BookSource::Rar(p) | BookSource::Pdf(p) => one(p),
            BookSource::Dir(_) => None,
        })
        .collect()
}

/// A file or folder that could not be read, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanFailure {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub books: Vec<ImportBook>,
    pub failures: Vec<ScanFailure>,
}

pub(crate) fn fail(message: impl std::fmt::Display) -> PipelineError {
    PipelineError::Import(message.to_string())
}

pub(crate) fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub(crate) fn is_page_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    // Files an archiver adds that are not pages.
    if base.starts_with("._") || base == ".ds_store" || base == "thumbs.db" {
        return false;
    }
    if lower.split(['/', '\\']).any(|part| part == "__macosx") {
        return false;
    }
    let ext = base.rsplit('.').next().unwrap_or("");
    base.contains('.') && PAGE_EXTENSIONS.contains(&ext)
}

/// Read what is at the front of a file, to tell what it really is.
fn sniff(path: &Path) -> Option<Format> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Some(Format::Zip)
    } else if head.starts_with(b"Rar!\x1a\x07") {
        Some(Format::Rar)
    } else if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some(Format::SevenZ)
    } else if head.starts_with(b"%PDF") {
        Some(Format::Pdf)
    } else {
        None
    }
}

const UNSUPPORTED: [&str; 7] = ["mobi", "azw", "azw3", "azw4", "kfx", "lit", "djvu"];

/// Read one file as a book.
fn scan_file(path: &Path, cancel: &CancelFlag) -> Result<ImportBook> {
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let ext = extension(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if UNSUPPORTED.contains(&ext.as_str()) {
        return Err(fail(format!(
            "{name} is a .{ext} file, which cannot be imported: it is a locked or proprietary format. Convert it to CBZ or EPUB first (Calibre can do it)."
        )));
    }
    let format = sniff(path).or(match ext.as_str() {
        "cbz" | "zip" | "epub" => Some(Format::Zip),
        "cbr" | "rar" => Some(Format::Rar),
        "cb7" | "7z" => Some(Format::SevenZ),
        "pdf" => Some(Format::Pdf),
        _ => None,
    });
    match format {
        Some(Format::Zip) => zip_in::scan(path, cancel),
        Some(Format::Rar) => rar_in::scan(path, cancel),
        Some(Format::SevenZ) => sevenz_in::scan(path, cancel),
        Some(Format::Pdf) => pdf_in::scan(path, cancel),
        _ => Err(fail(format!(
            "{name} is not a file type MangaPoint can import. It reads CBZ, ZIP, CBR, RAR, CB7, 7Z, EPUB and PDF books, and JPG, PNG, WEBP, GIF and TIFF pictures."
        ))),
    }
}

/// Read what was chosen: folders, archives, books and loose pictures. Loose
/// pictures from one folder are one book. A file that cannot be read does not
/// stop the others; it is listed in `failures`. When `cancel` is set the
/// reading stops at the next file, archive entry or page, and what has been
/// read so far is returned (a caller that cancelled drops it).
pub fn scan(paths: &[PathBuf], cancel: &CancelFlag) -> ScanResult {
    let mut result = ScanResult::default();
    let mut loose: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for path in paths {
        if cancel.is_cancelled() {
            return result;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let book = if path.is_dir() {
            folder::scan_dir(path, cancel)
        } else if PAGE_EXTENSIONS.contains(&extension(path).as_str()) {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            match loose.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, files)) => files.push(path.clone()),
                None => loose.push((parent, vec![path.clone()])),
            }
            continue;
        } else if path.is_file() {
            scan_file(path, cancel)
        } else {
            Err(fail("it is not there"))
        };
        match book {
            Ok(book) => result.books.push(book),
            Err(PipelineError::Cancelled) => return result,
            Err(e) => result.failures.push(ScanFailure {
                file: name,
                message: e.to_string().trim_start_matches("import: ").to_string(),
            }),
        }
    }
    for (parent, files) in loose {
        if cancel.is_cancelled() {
            return result;
        }
        match folder::scan_files(&parent, files, cancel) {
            Ok(book) => result.books.push(book),
            Err(PipelineError::Cancelled) => return result,
            Err(e) => result.failures.push(ScanFailure {
                file: parent.display().to_string(),
                message: e.to_string().trim_start_matches("import: ").to_string(),
            }),
        }
    }
    result
}

#[cfg(test)]
mod tests;
