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
mod comicinfo;
mod epub_in;
mod folder;
mod names;
mod pdf_in;
mod rar_in;
mod sevenz_in;
mod tiff_in;
mod zip_in;

pub use comicinfo::ComicInfo;
pub use names::{chapter_name, chapter_number, clean_name, natural_cmp, plain, series_title};

use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use library::{chapter_key, DownloadedChapter, LibraryFiles, IMPORT_SOURCE};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// The most a single page may be once unpacked, and the most entries an
/// archive may hold: a defence against archives made to fill the disk.
pub(crate) const MAX_PAGE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 100_000;

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
fn scan_file(path: &Path) -> Result<ImportBook> {
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
        Some(Format::Zip) => zip_in::scan(path),
        Some(Format::Rar) => rar_in::scan(path),
        Some(Format::SevenZ) => sevenz_in::scan(path),
        Some(Format::Pdf) => pdf_in::scan(path),
        _ => Err(fail(format!(
            "{name} is not a file type MangaPoint can import. It reads CBZ, ZIP, CBR, RAR, CB7, 7Z, EPUB and PDF books, and JPG, PNG, WEBP, GIF and TIFF pictures."
        ))),
    }
}

/// Read what was chosen: folders, archives, books and loose pictures. Loose
/// pictures from one folder are one book. A file that cannot be read does not
/// stop the others; it is listed in `failures`.
pub fn scan(paths: &[PathBuf]) -> ScanResult {
    let mut result = ScanResult::default();
    let mut loose: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let book = if path.is_dir() {
            folder::scan_dir(path)
        } else if PAGE_EXTENSIONS.contains(&extension(path).as_str()) {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            match loose.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, files)) => files.push(path.clone()),
                None => loose.push((parent, vec![path.clone()])),
            }
            continue;
        } else if path.is_file() {
            scan_file(path)
        } else {
            Err(fail("it is not there"))
        };
        match book {
            Ok(book) => result.books.push(book),
            Err(e) => result.failures.push(ScanFailure {
                file: name,
                message: e.to_string().trim_start_matches("import: ").to_string(),
            }),
        }
    }
    for (parent, files) in loose {
        match folder::scan_files(&parent, files) {
            Ok(book) => result.books.push(book),
            Err(e) => result.failures.push(ScanFailure {
                file: parent.display().to_string(),
                message: e.to_string().trim_start_matches("import: ").to_string(),
            }),
        }
    }
    result
}

// ---- Writing -----------------------------------------------------------------

/// What [`commit`] needs to know about where the book goes.
pub struct CommitTarget<'a> {
    pub series_id: &'a str,
    /// The chapters the series already has, so a new one never takes a folder
    /// that is in use.
    pub existing: &'a [DownloadedChapter],
    /// The id for the first new chapter (see `LibraryDb::next_import_chapter_id`);
    /// the next ones count down from it.
    pub first_id: i64,
    pub now: u64,
}

#[derive(Debug, Default)]
pub struct CommitOutcome {
    /// The chapters that were written (or added to), ready for the library.
    pub chapters: Vec<DownloadedChapter>,
    /// Pages that were not pictures MangaPoint can read.
    pub skipped_pages: usize,
}

/// A page, if it is a picture the converter reads: its bytes are kept as they
/// are. TIFFs have been turned into PNGs by the time they get here.
fn accept(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let format = image::guess_format(&bytes).ok()?;
    if !matches!(
        format,
        image::ImageFormat::Jpeg
            | image::ImageFormat::Png
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
    ) {
        return None;
    }
    let (w, h) = image::ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .ok()?;
    (w > 0 && h > 0 && u64::from(w) * u64::from(h) <= 400_000_000).then_some(bytes)
}

/// Read the pages named by `keys` from one source, handing each to `each`
/// with its key (in any order: a solid archive is read once, front to back).
fn visit(
    source: &BookSource,
    keys: &[String],
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    match source {
        BookSource::Dir(root) => folder::visit(root, keys, each),
        BookSource::Zip(path) => zip_in::visit(path, keys, each),
        BookSource::SevenZ(path) => sevenz_in::visit(path, keys, each),
        BookSource::Rar(path) => rar_in::visit(path, keys, each),
        BookSource::Pdf(path) => pdf_in::visit(path, keys, each),
    }
}

struct Planned {
    key: String,
    into: Placement,
    number: Option<f64>,
    name: String,
    id: i64,
    /// Pages the chapter already has when pages are added to it.
    base: usize,
    pages: Vec<PageRef>,
}

/// Write a book's chapters into the library. `progress(done, total)` is called
/// as pages are written; the work stops at the next page when `cancel` is set,
/// and the chapters it had started are taken away again.
pub fn commit(
    book: &ImportBook,
    files: &LibraryFiles,
    target: &CommitTarget,
    progress: &mut dyn FnMut(usize, usize),
    cancel: &CancelFlag,
) -> Result<CommitOutcome> {
    let io = |path: &Path, e: std::io::Error| PipelineError::Io(path.to_path_buf(), e);
    let mut used: std::collections::HashSet<String> =
        target.existing.iter().map(|c| c.key.clone()).collect();
    let mut next_id = target.first_id;
    let mut planned: Vec<Planned> = Vec::new();
    for chapter in book.chapters.iter().filter(|c| !c.skip && !c.pages.is_empty()) {
        let (key, id, base) = match &chapter.into {
            Placement::New => {
                let id = next_id;
                next_id -= 1;
                let mut key = chapter_key(chapter.number, id);
                if used.contains(&key) || files.chapter_dir(target.series_id, &key).exists() {
                    key = chapter_key(None, id);
                }
                (key, id, 0)
            }
            Placement::Replace(key) => {
                let id = existing_id(target.existing, key).unwrap_or(next_id);
                (key.clone(), id, 0)
            }
            Placement::Append(key) => {
                let id = existing_id(target.existing, key).unwrap_or(next_id);
                let base = files.page_files(target.series_id, key).len();
                (key.clone(), id, base)
            }
        };
        used.insert(key.clone());
        planned.push(Planned {
            key,
            into: chapter.into.clone(),
            number: chapter.number,
            name: chapter.name.clone(),
            id,
            base,
            pages: chapter.pages.clone(),
        });
    }
    let total: usize = planned.iter().map(|p| p.pages.len()).sum();
    let mut done = 0usize;
    let mut skipped = 0usize;
    let mut created: Vec<String> = Vec::new();
    // Pages by source and key, to the chapter and place they go to.
    let mut wanted: Vec<HashMap<String, Vec<(usize, usize)>>> =
        vec![HashMap::new(); book.sources.len()];
    for (c, plan) in planned.iter().enumerate() {
        if plan.into == Placement::New || matches!(plan.into, Placement::Replace(_)) {
            if matches!(plan.into, Placement::Replace(_)) {
                files
                    .remove_chapter(target.series_id, &plan.key)
                    .map_err(|e| io(&files.chapter_dir(target.series_id, &plan.key), e))?;
            }
            created.push(plan.key.clone());
        }
        let dir = files.chapter_dir(target.series_id, &plan.key);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        for (p, page) in plan.pages.iter().enumerate() {
            if let Some(map) = wanted.get_mut(page.source) {
                map.entry(page.key.clone()).or_default().push((c, p));
            }
        }
    }
    let cleanup = |created: &[String]| {
        for key in created {
            let _ = files.remove_chapter(target.series_id, key);
        }
    };
    progress(0, total);
    for (s, source) in book.sources.iter().enumerate() {
        let map = &wanted[s];
        if map.is_empty() {
            continue;
        }
        let keys: Vec<String> = book
            .chapters
            .iter()
            .filter(|c| !c.skip)
            .flat_map(|c| c.pages.iter())
            .filter(|p| p.source == s)
            .map(|p| p.key.clone())
            .collect();
        let run = visit(source, &keys, &mut |key, bytes| {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let Some(places) = map.get(key) else {
                return Ok(());
            };
            let mut bytes = bytes;
            let mut left = places.len();
            for &(c, p) in places {
                left -= 1;
                // The last place takes the bytes; a page used twice is copied.
                let own = if left == 0 { std::mem::take(&mut bytes) } else { bytes.clone() };
                let plan = &planned[c];
                match accept(own) {
                    Some(good) => {
                        let path = files.page_path(target.series_id, &plan.key, plan.base + p);
                        std::fs::write(&path, good).map_err(|e| io(&path, e))?;
                    }
                    None => skipped += 1,
                }
                done += 1;
            }
            progress(done, total);
            Ok(())
        });
        if let Err(e) = run {
            cleanup(&created);
            return Err(e);
        }
    }
    // Close the gaps left by pages that could not be read, and count.
    let mut chapters = Vec::new();
    for plan in &planned {
        let dir = files.chapter_dir(target.series_id, &plan.key);
        let mut pages = files.page_files(target.series_id, &plan.key);
        pages.sort();
        for (i, from) in pages.iter().enumerate() {
            let to = files.page_path(target.series_id, &plan.key, i);
            if *from != to {
                std::fs::rename(from, &to).map_err(|e| io(&to, e))?;
            }
        }
        if pages.is_empty() {
            let _ = std::fs::remove_dir(&dir);
            continue;
        }
        let existing = target.existing.iter().find(|c| c.key == plan.key);
        let keep_details = matches!(plan.into, Placement::Append(_));
        chapters.push(DownloadedChapter {
            key: plan.key.clone(),
            number: if keep_details { existing.and_then(|c| c.number) } else { plan.number },
            name: if keep_details {
                existing.map_or_else(|| plan.name.clone(), |c| c.name.clone())
            } else {
                plan.name.clone()
            },
            source_id: IMPORT_SOURCE.to_string(),
            chapter_id: plan.id,
            pages: pages.len(),
            bytes: files.chapter_bytes(target.series_id, &plan.key),
            downloaded_at: target.now,
        });
    }
    progress(total, total);
    Ok(CommitOutcome {
        chapters,
        skipped_pages: skipped,
    })
}

fn existing_id(existing: &[DownloadedChapter], key: &str) -> Option<i64> {
    existing.iter().find(|c| c.key == key).map(|c| c.chapter_id)
}

#[cfg(test)]
mod tests;
