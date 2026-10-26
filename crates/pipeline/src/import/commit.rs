//! Writing an import into the library.
//!
//! Nothing in the library is touched until every page has been read and
//! written: the pages go to a staging folder of their own inside the series'
//! folder, and only when all of them are there are the chapters moved into
//! place (a rename each). Whatever goes wrong before that, or while moving,
//! leaves the library as it was: the staging folder is removed, chapters that
//! were already moved are moved back out, and a chapter that was being replaced
//! is put back.

use super::{
    fail, folder, pdf_in, rar_in, sevenz_in, zip_in, BookSource, ImportBook, Placement,
    MAX_PAGE_PIXELS, MAX_TOTAL_BYTES,
};
use crate::download::write_atomically;
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use library::{chapter_key, DownloadedChapter, LibraryDb, LibraryFiles, IMPORT_SOURCE};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The start of the name of a staging folder (`_import-<pid>-<n>`): it is a
/// folder in the series' folder, which no chapter key can be named like.
const STAGE_PREFIX: &str = "_import-";
/// Written first in a staging folder, so a sweep only touches ours.
const STAGE_MARKER: &str = "import.partial";
/// A chapter that is being replaced waits in the staging folder under this
/// name (followed by its own folder name) until the new one is in place.
const BACKUP_PREFIX: &str = "old-";

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

/// Does this page pass for a picture the converter can read? Its bytes are kept
/// as they are. TIFFs have been turned into PNGs by the time they get here.
pub(crate) fn accept(bytes: &[u8]) -> bool {
    let Ok(format) = image::guess_format(bytes) else {
        return false;
    };
    if !matches!(
        format,
        image::ImageFormat::Jpeg
            | image::ImageFormat::Png
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
    ) {
        return false;
    }
    let Ok((w, h)) = image::ImageReader::with_format(Cursor::new(bytes), format).into_dimensions()
    else {
        return false;
    };
    w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PAGE_PIXELS
}

/// Read the pages named by `keys` from one source, handing each to `each`
/// with its key (in any order: a solid archive is read once, front to back).
fn visit(
    source: &BookSource,
    keys: &[String],
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    match source {
        BookSource::Dir(root) => folder::visit(root, keys, cancel, each),
        BookSource::Zip(path) => zip_in::visit(path, keys, cancel, each),
        BookSource::SevenZ(path) => sevenz_in::visit(path, keys, cancel, each),
        BookSource::Rar(path) => rar_in::visit(path, keys, cancel, each),
        BookSource::Pdf(path) => pdf_in::visit(path, keys, cancel, each),
    }
}

struct Planned {
    key: String,
    into: Placement,
    number: Option<f64>,
    name: String,
    id: i64,
    pages: Vec<super::PageRef>,
}

/// A change made to the library, to be undone if a later one fails.
enum Undo {
    /// A chapter folder this import made.
    NewDir(String),
    /// A chapter folder this import put in place of another, which waits at
    /// `backup`.
    Swapped { key: String, backup: PathBuf },
    /// Pages this import added to a chapter that was already there.
    Added(Vec<PathBuf>),
}

/// The staging folder, and what to undo. Dropping it removes the staging
/// folder, and (unless the work succeeded and it was disarmed) undoes
/// everything that was moved into the library: every way out of [`commit`]
/// cleans up exactly what the import made.
struct Work<'a> {
    files: &'a LibraryFiles,
    series_id: &'a str,
    stage: String,
    series_existed: bool,
    undo: Vec<Undo>,
    armed: bool,
}

impl<'a> Work<'a> {
    fn new(files: &'a LibraryFiles, series_id: &'a str) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let series_existed = files.series_dir(series_id).exists();
        let series = files.series_dir(series_id);
        std::fs::create_dir_all(&series).map_err(|e| PipelineError::Io(series.clone(), e))?;
        let stage = format!(
            "{STAGE_PREFIX}{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        // Built before the marker is written, so that a failure here still
        // cleans up what exists.
        let work = Self {
            files,
            series_id,
            stage,
            series_existed,
            undo: Vec::new(),
            armed: true,
        };
        let dir = work.stage_dir();
        std::fs::create_dir_all(&dir).map_err(|e| PipelineError::Io(dir.clone(), e))?;
        let marker = dir.join(STAGE_MARKER);
        std::fs::write(&marker, std::process::id().to_string())
            .map_err(|e| PipelineError::Io(marker, e))?;
        Ok(work)
    }

    fn stage_dir(&self) -> PathBuf {
        self.files.chapter_dir(self.series_id, &self.stage)
    }

    fn rollback(&mut self) {
        for undo in std::mem::take(&mut self.undo).into_iter().rev() {
            match undo {
                Undo::NewDir(key) => {
                    let _ = self.files.remove_chapter(self.series_id, &key);
                }
                Undo::Swapped { key, backup } => {
                    let _ = self.files.remove_chapter(self.series_id, &key);
                    let _ = std::fs::rename(&backup, self.files.chapter_dir(self.series_id, &key));
                }
                Undo::Added(paths) => {
                    for path in paths {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }
}

impl Drop for Work<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.rollback();
        }
        let _ = self.files.remove_chapter(self.series_id, &self.stage);
        // A series folder this import made, and nothing came of it.
        if !self.series_existed {
            let _ = std::fs::remove_dir(self.files.series_dir(self.series_id));
        }
    }
}

/// The page files of a folder (`NNNNN.img`), in reading order, with the number
/// each is named by.
fn images_in(dir: &Path, include_empty: bool) -> Vec<(u64, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, PathBuf)> = read
        .flatten()
        .filter(|e| {
            let path = e.path();
            path.extension().is_some_and(|x| x == "img")
                && (include_empty || e.metadata().is_ok_and(|m| m.len() > 0))
        })
        .filter_map(|e| {
            let path = e.path();
            let number = path.file_stem()?.to_str()?.parse::<u64>().ok()?;
            Some((number, path))
        })
        .collect();
    found.sort();
    found
}

fn page_name(number: u64) -> String {
    format!("{number:05}.img")
}

/// About how much room the pages will take: what the sources weigh.
fn estimate_bytes(book: &ImportBook, planned: &[Planned]) -> u64 {
    let mut total = 0u64;
    let mut sources: HashSet<usize> = HashSet::new();
    for page in planned.iter().flat_map(|p| &p.pages) {
        if let Some(source) = book.sources.get(page.source) {
            match source {
                BookSource::Dir(root) => {
                    let rel = page.key.rsplit_once('#').map_or(page.key.as_str(), |(r, _)| r);
                    total += std::fs::metadata(root.join(rel)).map_or(0, |m| m.len());
                }
                BookSource::Zip(p) | BookSource::SevenZ(p) | BookSource::Rar(p) | BookSource::Pdf(p) => {
                    if sources.insert(page.source) {
                        total += std::fs::metadata(p).map_or(0, |m| m.len());
                    }
                }
            }
        }
    }
    total
}

/// Write a book's chapters into the library. `progress(done, total)` is called
/// as pages are written; the work stops at the next page when `cancel` is set.
/// Whether it stops, fails or finishes, the library holds either all of what
/// was written or none of it, and a chapter being replaced is only given up
/// once its replacement is complete.
pub fn commit(
    book: &ImportBook,
    files: &LibraryFiles,
    target: &CommitTarget,
    progress: &mut dyn FnMut(usize, usize),
    cancel: &CancelFlag,
) -> Result<CommitOutcome> {
    let io = |path: &Path, e: std::io::Error| PipelineError::Io(path.to_path_buf(), e);
    let has = |key: &str| target.existing.iter().any(|c| c.key == key);
    let mut used: HashSet<String> = target.existing.iter().map(|c| c.key.clone()).collect();
    let mut next_id = target.first_id;
    let mut planned: Vec<Planned> = Vec::new();
    for chapter in book.chapters.iter().filter(|c| !c.skip && !c.pages.is_empty()) {
        // Pages for a chapter that is gone (deleted meanwhile) make a chapter of
        // their own, with their own number and name.
        let into = match &chapter.into {
            Placement::Append(key) | Placement::Replace(key) if !has(key) => Placement::New,
            other => other.clone(),
        };
        let (key, id) = match &into {
            Placement::New => {
                let id = next_id;
                next_id -= 1;
                let mut key = chapter_key(chapter.number, id);
                if used.contains(&key) || files.chapter_dir(target.series_id, &key).exists() {
                    key = chapter_key(None, id);
                }
                (key, id)
            }
            Placement::Replace(key) | Placement::Append(key) => {
                let id = target
                    .existing
                    .iter()
                    .find(|c| c.key == *key)
                    .map_or(next_id, |c| c.chapter_id);
                (key.clone(), id)
            }
        };
        used.insert(key.clone());
        planned.push(Planned {
            key,
            into,
            number: chapter.number,
            name: chapter.name.clone(),
            id,
            pages: chapter.pages.clone(),
        });
    }
    let total: usize = planned.iter().map(|p| p.pages.len()).sum();
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    if total == 0 {
        progress(0, 0);
        return Ok(CommitOutcome::default());
    }

    let mut work = Work::new(files, target.series_id)?;
    let stage = work.stage_dir();
    // Room for the pages (and, for a replacement, for both copies).
    crate::export::check_space(&stage, estimate_bytes(book, &planned))?;

    // Pages by source and key, to the chapter and place they go to.
    let mut wanted: Vec<HashMap<String, Vec<(usize, usize)>>> =
        vec![HashMap::new(); book.sources.len()];
    let mut order: Vec<Vec<String>> = vec![Vec::new(); book.sources.len()];
    for (c, plan) in planned.iter().enumerate() {
        let dir = stage.join(c.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        for (p, page) in plan.pages.iter().enumerate() {
            if let Some(map) = wanted.get_mut(page.source) {
                let places = map.entry(page.key.clone()).or_default();
                if places.is_empty() {
                    order[page.source].push(page.key.clone());
                }
                places.push((c, p));
            }
        }
    }
    progress(0, total);
    let mut done = 0usize;
    let mut skipped = 0usize;
    let mut written = 0u64;
    for (s, source) in book.sources.iter().enumerate() {
        let map = &wanted[s];
        if map.is_empty() {
            continue;
        }
        visit(source, &order[s], cancel, &mut |key, bytes| {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let Some(places) = map.get(key) else {
                return Ok(());
            };
            let good = accept(&bytes);
            for &(c, p) in places {
                if good {
                    written += bytes.len() as u64;
                    if written > MAX_TOTAL_BYTES {
                        return Err(fail(format!(
                            "{} holds more than {} GB of pictures, which is more than one import takes.",
                            book.label,
                            MAX_TOTAL_BYTES / (1024 * 1024 * 1024)
                        )));
                    }
                    let path = stage.join(c.to_string()).join(page_name(p as u64 + 1));
                    write_atomically(&path, &bytes)?;
                } else {
                    skipped += 1;
                }
                done += 1;
            }
            progress(done, total);
            Ok(())
        })?;
    }

    // Everything is written. Close the gaps left by pages that could not be
    // read (inside the staging folder, where nothing else looks), then move
    // each chapter into place.
    let mut chapters = Vec::new();
    for (c, plan) in planned.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let staged = stage.join(c.to_string());
        let pages = images_in(&staged, false);
        for (i, (_, from)) in pages.iter().enumerate() {
            let to = staged.join(page_name(i as u64 + 1));
            if *from != to {
                std::fs::rename(from, &to).map_err(|e| io(&to, e))?;
            }
        }
        if pages.is_empty() {
            // Nothing readable: what the chapter already has is left alone.
            continue;
        }
        let dest = files.chapter_dir(target.series_id, &plan.key);
        match &plan.into {
            Placement::Append(_) if dest.exists() => {
                // After the highest page number the chapter has, so a gap in its
                // numbering never makes a new page take an old one's name.
                let base = images_in(&dest, true).last().map_or(0, |(n, _)| *n);
                work.undo.push(Undo::Added(Vec::new()));
                for (i, _) in pages.iter().enumerate() {
                    let from = staged.join(page_name(i as u64 + 1));
                    let to = dest.join(page_name(base + i as u64 + 1));
                    std::fs::rename(&from, &to).map_err(|e| io(&to, e))?;
                    if let Some(Undo::Added(moved)) = work.undo.last_mut() {
                        moved.push(to);
                    }
                }
            }
            Placement::Replace(_) if dest.exists() => {
                let name = dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let backup = stage.join(format!("{BACKUP_PREFIX}{name}"));
                std::fs::rename(&dest, &backup).map_err(|e| io(&dest, e))?;
                if let Err(e) = std::fs::rename(&staged, &dest) {
                    let _ = std::fs::rename(&backup, &dest);
                    return Err(io(&dest, e));
                }
                work.undo.push(Undo::Swapped { key: plan.key.clone(), backup });
            }
            _ => {
                if dest.exists() {
                    return Err(fail(format!("the folder for chapter {} is already there", plan.name)));
                }
                std::fs::rename(&staged, &dest).map_err(|e| io(&dest, e))?;
                work.undo.push(Undo::NewDir(plan.key.clone()));
            }
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
            pages: files.page_files(target.series_id, &plan.key).len(),
            bytes: files.chapter_bytes(target.series_id, &plan.key),
            downloaded_at: target.now,
            // Imported pages are kept exactly as they came.
            quality: Some(library::PageQuality::Full),
        });
    }
    // Done: what was moved stays; the staging folder (and the chapters it held
    // that were replaced) goes when `work` is dropped.
    work.armed = false;
    progress(total, total);
    Ok(CommitOutcome {
        chapters,
        skipped_pages: skipped,
    })
}

// ---- Leftovers ---------------------------------------------------------------

/// The process id a staging folder's marker names.
fn marker_pid(stage: &Path) -> Option<u32> {
    std::fs::read_to_string(stage.join(STAGE_MARKER)).ok()?.trim().parse().ok()
}

/// Remove the staging folders an import left behind when the app was closed or
/// killed part-way. Only a folder that is named as a staging folder is, carries
/// the marker an import writes, names a process that is no longer running, and
/// that no chapter of the library is called is touched. A chapter that was
/// being replaced when the app stopped is put back first. Call it once when
/// the app starts. Returns how many folders it removed.
pub fn sweep_leftovers(files: &LibraryFiles, library: &LibraryDb) -> usize {
    sweep_with(files, library, &crate::compose::process_is_running)
}

pub(super) fn sweep_with(files: &LibraryFiles, library: &LibraryDb, running: &dyn Fn(u32) -> bool) -> usize {
    let mut removed = 0;
    for root in files.roots() {
        let Ok(series_dirs) = std::fs::read_dir(root) else {
            continue;
        };
        for series in series_dirs.flatten() {
            let series_path = series.path();
            if !series.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(inside) = std::fs::read_dir(&series_path) else {
                continue;
            };
            for entry in inside.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let stage = entry.path();
                if !name.starts_with(STAGE_PREFIX)
                    || !entry.file_type().is_ok_and(|t| t.is_dir())
                    || library
                        .series
                        .iter()
                        .any(|s| s.chapters.iter().any(|c| c.key == name))
                {
                    continue;
                }
                // Ours only: a marker written by an import, of a process that is gone.
                let Some(pid) = marker_pid(&stage) else {
                    continue;
                };
                if pid == std::process::id() || running(pid) {
                    continue;
                }
                // A chapter that was being replaced goes back where it was, when
                // nothing has taken its place.
                if let Ok(waiting) = std::fs::read_dir(&stage) {
                    for backup in waiting.flatten() {
                        let backup_name = backup.file_name().to_string_lossy().into_owned();
                        if let Some(original) = backup_name.strip_prefix(BACKUP_PREFIX) {
                            let back = series_path.join(original);
                            if !back.exists() {
                                let _ = std::fs::rename(backup.path(), back);
                            }
                        }
                    }
                }
                if library::remove_inside(root, &stage).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_failed_move_did_is_undone_new_folders_appended_pages_and_replaced_chapters() {
        let dir = std::env::temp_dir().join(format!("mp-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = LibraryFiles::new(dir.join("library"));
        // A chapter that is swapped out, and one that gets pages added.
        for key in ["swapped", "added"] {
            std::fs::create_dir_all(files.chapter_dir("s", key)).unwrap();
            std::fs::write(files.chapter_dir("s", key).join("00001.img"), b"old").unwrap();
        }
        {
            let mut work = Work::new(&files, "s").unwrap();
            let stage = work.stage_dir();
            // Swapped.
            let backup = stage.join("old-swapped");
            std::fs::rename(files.chapter_dir("s", "swapped"), &backup).unwrap();
            std::fs::create_dir_all(files.chapter_dir("s", "swapped")).unwrap();
            std::fs::write(files.chapter_dir("s", "swapped").join("00001.img"), b"new").unwrap();
            work.undo.push(Undo::Swapped { key: "swapped".into(), backup });
            // Added.
            let page = files.chapter_dir("s", "added").join("00002.img");
            std::fs::write(&page, b"extra").unwrap();
            work.undo.push(Undo::Added(vec![page]));
            // New.
            std::fs::create_dir_all(files.chapter_dir("s", "fresh")).unwrap();
            work.undo.push(Undo::NewDir("fresh".into()));
            // Dropped while armed: an exit by error.
        }
        assert_eq!(std::fs::read(files.chapter_dir("s", "swapped").join("00001.img")).unwrap(), b"old");
        assert!(!files.chapter_dir("s", "added").join("00002.img").exists());
        assert_eq!(std::fs::read(files.chapter_dir("s", "added").join("00001.img")).unwrap(), b"old");
        assert!(!files.chapter_dir("s", "fresh").exists());
        let names: Vec<_> = std::fs::read_dir(files.series_dir("s")).unwrap().flatten().collect();
        assert_eq!(names.len(), 2, "no staging folder left: {names:?}");
        // Disarmed (the work succeeded): what was moved stays, the staging folder goes.
        {
            let mut work = Work::new(&files, "s").unwrap();
            std::fs::create_dir_all(files.chapter_dir("s", "kept")).unwrap();
            work.undo.push(Undo::NewDir("kept".into()));
            work.armed = false;
        }
        assert!(files.chapter_dir("s", "kept").exists());
        assert_eq!(std::fs::read_dir(files.series_dir("s")).unwrap().flatten().count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_room_a_book_needs_is_what_its_sources_weigh_and_too_little_room_is_refused() {
        let dir = std::env::temp_dir().join(format!("mp-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.cbz"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("p.png"), vec![0u8; 300]).unwrap();
        let mut book = ImportBook::new("x", super::super::Format::Cbz, BookSource::Zip(dir.join("a.cbz")));
        book.sources.push(BookSource::Dir(dir.clone()));
        let page = |source: usize, key: &str| super::super::PageRef { source, key: key.into() };
        let plan = Planned {
            key: "c1".into(),
            into: Placement::New,
            number: Some(1.0),
            name: "one".into(),
            id: -1,
            // The archive counts once however many pages come from it.
            pages: vec![page(0, "1.jpg"), page(0, "2.jpg"), page(1, "p.png")],
        };
        assert_eq!(estimate_bytes(&book, &[plan]), 1300);
        // More room than any disk has: refused before anything is written, with the usual message.
        let err = crate::export::check_space(&dir, u64::MAX / 4).unwrap_err();
        assert!(matches!(err, PipelineError::NoSpace { .. }), "{err:?}");
        assert!(crate::export::check_space(&dir, 1000).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
