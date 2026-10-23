//! Importing: reading what the person chose into drafts they can correct, then
//! writing the books into the library. The reading and writing are the
//! pipeline's; this connects them to the dialog.
//!
//! Reading and writing both run in the background and can outlive the dialog
//! that started them (it can be closed, and another opened). Every read and
//! every run is given a number from `RUNS`, which also moves on when the
//! dialog is closed; what an older one finds out is dropped, never written into
//! the dialog (or the lists behind it) of a newer one.

use super::*;
use crate::logic::{ImportRegistry, Resolved, RunCounter};
use crate::state::{
    BookDraft, ChapterDraft, ChapterPlace, ImportDone, ImportSession, ImportStage, InfoDraft,
};
use pipeline::import::{self, CommitTarget, ImportBook, Placement};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The books that were read, behind the drafts the dialog shows (in the same
/// order). They hold the pages' places, which the drafts do not.
static IMPORT_BOOKS: Mutex<Vec<ImportBook>> = Mutex::new(Vec::new());
/// Moves on with every read, every run and every close of the dialog.
static RUNS: RunCounter = RunCounter::new();
/// How to stop the read that is going on.
static SCAN_CANCEL: Mutex<Option<CancelFlag>> = Mutex::new(None);
/// The import that is writing, for deleting and for closing the window to see.
static RUNNING: ImportRegistry = ImportRegistry::new();

fn next_run() -> u64 {
    RUNS.next()
}

/// Is run `mine` still the latest thing the dialog is doing?
fn current(mine: u64) -> bool {
    RUNS.is_current(mine)
}

/// Is an import writing into the library now?
pub fn import_running() -> bool {
    RUNNING.running()
}

/// The series a running import writes into.
pub fn importing_series() -> Vec<String> {
    RUNNING.series()
}

/// Lets go of the record of a run when it ends, however it ends (also when the
/// task is dropped).
struct RunGuard(u64);

impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.end(self.0);
    }
}

/// Change the dialog's state, if it is open.
fn edit_session(state: AppState, change: impl FnOnce(&mut ImportSession)) {
    let mut signal = state.import;
    let mut guard = signal.write();
    if let Some(session) = guard.as_mut() {
        change(session);
    }
}

/// Change the dialog's state, if it is open and run `run` is still the latest.
fn edit_session_for(state: AppState, run: u64, change: impl FnOnce(&mut ImportSession)) {
    if current(run) {
        edit_session(state, change);
    }
}

/// The address to show a cover from its bytes, as a small JPEG.
fn cover_url(bytes: &[u8]) -> Option<String> {
    let small = logic::small_cover(bytes)?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(&small)))
}

/// A picture to keep as a cover, as a small JPEG, with the address to show it.
pub fn cover_from_bytes(bytes: &[u8]) -> Option<(Arc<Vec<u8>>, String)> {
    let small = logic::small_cover(bytes)?;
    let url = format!("data:image/jpeg;base64,{}", STANDARD.encode(&small));
    Some((Arc::new(small), url))
}

/// The picture in a file, as a cover. `None` if it is not a picture. The file
/// is read and shrunk on a worker thread, not on the one the window runs on.
pub async fn cover_from_file(path: std::path::PathBuf) -> Option<(Arc<Vec<u8>>, String)> {
    const MAX: u64 = 40 * 1024 * 1024;
    tokio::task::spawn_blocking(move || {
        if std::fs::metadata(&path).ok()?.len() > MAX {
            return None;
        }
        cover_from_bytes(&std::fs::read(&path).ok()?)
    })
    .await
    .ok()
    .flatten()
}

fn status_word(raw: &str) -> String {
    let word = library::status_label(raw);
    if word.is_empty() {
        raw.trim().to_string()
    } else {
        word
    }
}

/// What the dialog shows for a book that was read.
fn draft_of(book: &ImportBook) -> BookDraft {
    let cover_url = book.cover.as_deref().and_then(cover_url);
    let meta = &book.meta;
    BookDraft {
        label: book.label.clone(),
        format: book.format.label(),
        info: InfoDraft {
            title: meta.title.clone(),
            description: meta.description.clone(),
            author: meta.author.clone(),
            artist: meta.artist.clone(),
            status: status_word(&meta.status),
            genres: meta.genres.clone(),
            cover_url,
            cover: None,
        },
        chapters: book
            .chapters
            .iter()
            .map(|c| ChapterDraft {
                number: c.number.map(import::plain).unwrap_or_default(),
                name: c.name.clone(),
                pages: c.pages.len(),
                include: !c.skip,
                meets: c.meets.clone(),
                place: ChapterPlace::Skip,
            })
            .collect(),
        warnings: book.warnings.clone(),
        include: true,
        open: true,
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

/// Put what the person typed into the book it was read as.
fn apply_edits(book: &mut ImportBook, draft: &BookDraft) {
    let info = &draft.info;
    if !info.title.trim().is_empty() {
        book.meta.title = info.title.trim().to_string();
    }
    book.meta.description = info.description.trim().to_string();
    book.meta.author = info.author.trim().to_string();
    book.meta.artist = info.artist.trim().to_string();
    book.meta.status = info.status.trim().to_string();
    book.meta.genres = info.genres.clone();
    if let Some(cover) = &info.cover {
        book.cover = Some(cover.to_vec());
    }
    for (chapter, edit) in book.chapters.iter_mut().zip(&draft.chapters) {
        chapter.number = parse_number(&edit.number);
        let name = edit.name.trim();
        chapter.name = if name.is_empty() {
            import::chapter_name("", chapter.number)
        } else {
            name.to_string()
        };
        chapter.meets = edit.meets.clone();
        match (&edit.meets, edit.place) {
            (Some(key), ChapterPlace::Add) => {
                chapter.into = Placement::Append(key.clone());
                chapter.skip = false;
            }
            (Some(key), ChapterPlace::Replace) => {
                chapter.into = Placement::Replace(key.clone());
                chapter.skip = false;
            }
            (Some(_), ChapterPlace::Both) => {
                chapter.into = Placement::New;
                chapter.skip = false;
            }
            (Some(_), ChapterPlace::Skip) => chapter.skip = true,
            (None, _) => chapter.skip = !edit.include,
        }
    }
}

/// Choose files to import with the system's dialog, then read them. `target`
/// is the series pages are being added to, if any.
pub fn start_import(state: AppState, target: Option<String>, folder: bool) {
    spawn_forever(async move {
        let paths = if folder {
            crate::ui::widgets::pick_folder("Choose a folder to import", None)
                .await
                .map(|p| vec![p])
        } else {
            crate::ui::widgets::pick_files(
                "Choose books and pictures to import",
                None,
                "Comics, books and pictures",
                &pipeline::import::IMPORT_EXTENSIONS,
            )
            .await
        };
        if let Some(paths) = paths.filter(|p| !p.is_empty()) {
            import_paths(state, target, paths);
        }
    });
}

/// Read these files and folders and open the dialog on what they hold.
pub fn import_paths(state: AppState, target: Option<String>, paths: Vec<std::path::PathBuf>) {
    read_paths(state, target, paths, false);
}

/// Choose more files with the system's dialog and add what they hold to what the
/// dialog already shows.
pub fn add_to_import(state: AppState, folder: bool) {
    let target = state.import.peek().as_ref().and_then(|s| s.target.clone());
    spawn_forever(async move {
        let paths = if folder {
            crate::ui::widgets::pick_folder("Choose a folder to import", None)
                .await
                .map(|p| vec![p])
        } else {
            crate::ui::widgets::pick_files(
                "Choose more books and pictures to import",
                None,
                "Comics, books and pictures",
                &pipeline::import::IMPORT_EXTENSIONS,
            )
            .await
        };
        if let Some(paths) = paths.filter(|p| !p.is_empty()) {
            read_paths(state, target, paths, true);
        }
    });
}

/// Read these files into the dialog that is open, after the books it has.
pub fn add_paths_to_import(state: AppState, paths: Vec<std::path::PathBuf>) {
    let target = state.import.peek().as_ref().and_then(|s| s.target.clone());
    read_paths(state, target, paths, true);
}

/// Take one book out of the dialog (and out of the list behind it, together,
/// so the two never differ).
pub fn remove_book(state: AppState, index: usize) {
    let Ok(mut books) = IMPORT_BOOKS.lock() else {
        return;
    };
    edit_session(state, |s| {
        if index < s.books.len() && index < books.len() {
            books.remove(index);
            s.books.remove(index);
        }
    });
}

/// Read these; `more` keeps the books the dialog already has and puts the
/// new ones after them.
fn read_paths(state: AppState, target: Option<String>, paths: Vec<std::path::PathBuf>, more: bool) {
    // This read is the one that counts; one still going is stopped.
    let run = next_run();
    let cancel = CancelFlag::new();
    if let Ok(mut slot) = SCAN_CANCEL.lock() {
        if let Some(older) = slot.replace(cancel.clone()) {
            older.cancel();
        }
    }
    let mut session = state.import;
    let kept = if more { session.peek().clone() } else { None };
    if let Some(mut kept) = kept.clone() {
        kept.stage = ImportStage::Reading;
        session.set(Some(kept));
    } else {
        session.set(Some(ImportSession {
            target: target.clone(),
            stage: ImportStage::Reading,
            books: Vec::new(),
            failures: Vec::new(),
            combine: false,
            done_pages: 0,
            total_pages: 0,
            finished: Vec::new(),
            errors: Vec::new(),
        }));
    }
    // What a series being added to already has: its numbers are taken.
    let existing: Vec<library::DownloadedChapter> = target
        .as_deref()
        .and_then(|id| state.library.peek().find(id).map(|s| s.chapters.clone()))
        .unwrap_or_default();
    let adding = target.is_some();
    // Which files were imported into which series before.
    let known: Vec<(String, Vec<String>)> = state
        .library
        .peek()
        .series
        .iter()
        .filter_map(|s| {
            s.imported
                .as_ref()
                .filter(|i| !i.books.is_empty())
                .map(|i| (s.id.clone(), i.books.clone()))
        })
        .collect();
    let scan_target = target.clone();
    spawn_forever(async move {
        let scan_cancel = cancel.clone();
        let read = tokio::task::spawn_blocking(move || {
            let mut result = import::scan(&paths, &scan_cancel);
            let taken: Vec<f64> = existing.iter().filter_map(|c| c.number).collect();
            for book in &mut result.books {
                if scan_cancel.is_cancelled() {
                    break;
                }
                book.number_chapters(&taken);
                if adding {
                    book.meet(&existing);
                }
                // The same file as before: its chapters are left out unless the person says so.
                let prints = import::book_fingerprints(book);
                let series = scan_target.clone().unwrap_or_else(|| {
                    library::series_id(&logic::wanted_title(&book.meta.title, &book.label))
                });
                if logic::already_imported(&known, &series, &prints) {
                    for chapter in &mut book.chapters {
                        chapter.skip = true;
                    }
                    book.warnings.push(
                        "Already imported: this same file was imported into the series before, so its chapters are left out. Tick the ones you want to import again."
                            .to_string(),
                    );
                }
            }
            let drafts: Vec<BookDraft> = if scan_cancel.is_cancelled() {
                Vec::new()
            } else {
                result.books.iter().map(draft_of).collect()
            };
            (result, drafts)
        })
        .await;
        // Closed, or read again, meanwhile: this read's answer is no longer wanted.
        if !current(run) || cancel.is_cancelled() {
            return;
        }
        match read {
            Ok((result, drafts)) => {
                let mut failures: Vec<(String, String)> = result
                    .failures
                    .into_iter()
                    .map(|f| (f.file, f.message))
                    .collect();
                if drafts.is_empty() && failures.is_empty() {
                    failures.push((String::new(), "There is nothing to import there.".into()));
                }
                // The books and their drafts change together, under one lock.
                let Ok(mut books) = IMPORT_BOOKS.lock() else {
                    return;
                };
                let new_books = result.books;
                edit_session_for(state, run, move |s| {
                    if more && books.len() == s.books.len() {
                        books.extend(new_books);
                        s.books.extend(drafts);
                    } else {
                        *books = new_books;
                        s.books = drafts;
                    }
                    s.failures = failures;
                    s.stage = ImportStage::Review;
                });
            }
            Err(e) => {
                edit_session_for(state, run, move |s| {
                    s.failures = vec![(String::new(), format!("Reading the files failed: {e}"))];
                    s.stage = ImportStage::Review;
                });
            }
        }
    });
}

/// Close the dialog. A read or an import in progress is stopped first, and
/// whatever it still finds out is dropped.
pub fn close_import(state: AppState) {
    next_run();
    if let Ok(mut slot) = SCAN_CANCEL.lock() {
        if let Some(read) = slot.take() {
            read.cancel();
        }
    }
    cancel_import();
    let mut session = state.import;
    session.set(None);
    if let Ok(mut books) = IMPORT_BOOKS.lock() {
        books.clear();
    }
}

/// Stop a running import at the next page.
pub fn cancel_import() {
    RUNNING.cancel();
}

/// One series to make or add to: a book (or books put together) and what the
/// person said about it.
struct Unit {
    book: ImportBook,
    info: InfoDraft,
}

/// Write the books of the dialog into the library.
pub fn run_import(state: AppState) {
    let Some(session) = state.import.peek().clone() else {
        return;
    };
    if session.stage != ImportStage::Review {
        return;
    }
    // Pages cannot be added to a series that has been deleted since.
    if let Some(id) = session.target.as_deref() {
        if let Err(message) = logic::resolve_series(&state.library.peek(), Some(id), "") {
            edit_session(state, |s| s.errors.push(message.to_string()));
            return;
        }
    }
    if RUNNING.running() {
        edit_session(state, |s| {
            s.errors.push("An import is still finishing. Try again in a moment.".into());
        });
        return;
    }
    // The books are moved out of the list, not copied (they can hold a lot).
    let taken: Vec<ImportBook> = IMPORT_BOOKS
        .lock()
        .map(|mut b| std::mem::take(&mut *b))
        .unwrap_or_default();
    if taken.len() != session.books.len() {
        // The list behind the dialog and the dialog differ: nothing is written from a
        // list that may not be what is shown.
        if let Ok(mut books) = IMPORT_BOOKS.lock() {
            *books = taken;
        }
        edit_session(state, |s| {
            s.errors.push("The list changed while it was being read. Add the files again.".into());
        });
        return;
    }
    let mut staged: Vec<(ImportBook, &BookDraft)> = taken.into_iter().zip(&session.books).collect();
    for (book, draft) in &mut staged {
        if draft.include {
            apply_edits(book, draft);
        }
    }
    let total: usize = staged
        .iter()
        .filter(|(_, d)| d.include)
        .map(|(b, _)| b.included_pages())
        .sum();
    if total == 0 {
        if let Ok(mut books) = IMPORT_BOOKS.lock() {
            *books = staged.into_iter().map(|(b, _)| b).collect();
        }
        edit_session(state, |s| s.errors.push("Nothing is chosen to import.".into()));
        return;
    }
    let edited: Vec<(ImportBook, InfoDraft)> = staged
        .into_iter()
        .filter(|(_, d)| d.include)
        .map(|(b, d)| (b, d.info.clone()))
        .collect();
    let units: Vec<Unit> = if session.combine && edited.len() > 1 && session.target.is_none() {
        let info = edited[0].1.clone();
        ImportBook::merge(edited.into_iter().map(|(b, _)| b).collect())
            .map(|book| Unit { book, info })
            .into_iter()
            .collect()
    } else {
        edited.into_iter().map(|(book, info)| Unit { book, info }).collect()
    };
    // The series the import will write into, known before it starts so that a
    // delete or the closing of the window can see them.
    let series: Vec<String> = {
        let library = state.library.peek();
        units
            .iter()
            .filter_map(|u| {
                let wanted = logic::wanted_title(&u.book.meta.title, &u.book.label);
                logic::resolve_series(&library, session.target.as_deref(), &wanted).ok()
            })
            .map(|r| r.id)
            .collect()
    };
    let run = next_run();
    let cancel = CancelFlag::new();
    if !RUNNING.begin(run, series, cancel.clone()) {
        return;
    }
    edit_session(state, |s| {
        s.stage = ImportStage::Importing;
        s.done_pages = 0;
        s.total_pages = total;
        s.finished.clear();
        s.errors.clear();
    });
    let target = session.target.clone();
    let guard = RunGuard(run);
    spawn_forever(async move {
        let _running = guard;
        let mut finished: Vec<ImportDone> = Vec::new();
        let mut base = 0usize;
        for unit in units {
            let pages = unit.book.included_pages();
            match import_unit(state, run, unit, target.clone(), base, &cancel).await {
                Ok(Some(done)) => {
                    finished.push(done.clone());
                    edit_session_for(state, run, |s| s.finished.push(done));
                }
                Ok(None) => {}
                Err(PipelineError::Cancelled) => {
                    edit_session_for(state, run, |s| {
                        s.errors.push("Cancelled. What was started was taken away again.".into());
                    });
                    break;
                }
                Err(message) => {
                    let text = message.to_string();
                    let text = text.trim_start_matches("import: ").to_string();
                    edit_session_for(state, run, |s| s.errors.push(text));
                }
            }
            base += pages;
        }
        edit_session_for(state, run, |s| {
            s.stage = ImportStage::Finished;
            s.done_pages = s.total_pages;
        });
        let chapters: usize = finished.iter().map(|d| d.chapters).sum();
        if chapters > 0 {
            state.notify(format!(
                "Imported {} into {}.",
                logic::plural(chapters, "chapter"),
                finished.first().map_or("your library", |d| d.title.as_str())
            ));
        }
    });
}

/// Is an export of this series running, or paused to go on later?
fn being_exported(state: AppState, series_id: &str) -> bool {
    state
        .job
        .peek()
        .as_ref()
        .is_some_and(|v| v.kind == JobKind::Export && v.result.is_none() && v.series_id == series_id)
}

/// Make or add to one series from one unit. `Ok(None)` when there was nothing
/// new to add.
async fn import_unit(
    state: AppState,
    run: u64,
    unit: Unit,
    target: Option<String>,
    base: usize,
    cancel: &CancelFlag,
) -> Result<Option<ImportDone>, PipelineError> {
    let Unit { mut book, info } = unit;
    let files = library_files();
    // Which series: the one being added to, one imported before with this title,
    // or a new one (with a free title if another series has the name). A series
    // that was renamed keeps its id; nothing is looked up by its title.
    let Resolved { id: series_id, title, new_series } = {
        let wanted = logic::wanted_title(&book.meta.title, &book.label);
        logic::resolve_series(&state.library.peek(), target.as_deref(), &wanted)
            .map_err(|m| PipelineError::Import(m.to_string()))?
    };
    let existing: Vec<library::DownloadedChapter> = state
        .library
        .peek()
        .find(&series_id)
        .map(|s| s.chapters.clone())
        .unwrap_or_default();
    // Numbers: those the series has are taken; a chapter that meets one is left
    // out unless the person chose otherwise.
    let taken: Vec<f64> = existing.iter().filter_map(|c| c.number).collect();
    book.number_chapters(&taken);
    book.meet_undecided(&existing);
    // Chapters of a series that is being exported are not added to or replaced
    // meanwhile: the export is reading them.
    if being_exported(state, &series_id) {
        let mut held = 0;
        for chapter in book
            .chapters
            .iter_mut()
            .filter(|c| !c.skip && matches!(c.into, Placement::Append(_) | Placement::Replace(_)))
        {
            chapter.skip = true;
            held += 1;
        }
        if held > 0 {
            edit_session_for(state, run, |s| {
                s.errors.push(format!(
                    "{} of {title} {} not changed, because the series is being exported. Import again when the export has finished.",
                    logic::plural(held, "chapter"),
                    if held == 1 { "was" } else { "were" }
                ));
            });
        }
    }
    let already = book
        .chapters
        .iter()
        .filter(|c| c.skip && c.meets.is_some())
        .count();
    if book.included_pages() == 0 {
        // Nothing new: say so, with what was left out.
        return Ok(Some(ImportDone {
            series_id,
            title,
            chapters: 0,
            pages: 0,
            skipped: 0,
            already,
        }));
    }
    let first_id = state.library.peek().next_import_chapter_id();
    let now = now_secs();
    let progress = Arc::new(AtomicUsize::new(0));
    let book = Arc::new(book);
    let (task_files, task_book, task_series, task_cancel) =
        (files.clone(), book.clone(), series_id.clone(), cancel.clone());
    let (task_existing, task_progress) = (existing, progress.clone());
    let handle = tokio::task::spawn_blocking(move || {
        let target = CommitTarget {
            series_id: &task_series,
            existing: &task_existing,
            first_id,
            now,
        };
        import::commit(
            &task_book,
            &task_files,
            &target,
            &mut |done, _| task_progress.store(done, Ordering::Relaxed),
            &task_cancel,
        )
    });
    while !handle.is_finished() {
        tokio::time::sleep(Duration::from_millis(120)).await;
        let done = base + progress.load(Ordering::Relaxed);
        edit_session_for(state, run, |s| s.done_pages = done);
    }
    let outcome = handle
        .await
        .map_err(|e| PipelineError::Import(e.to_string()))??;
    if outcome.chapters.is_empty() {
        // Pages were read but none was a picture MangaPoint can use: say so.
        return Ok((outcome.skipped_pages > 0).then_some(ImportDone {
            series_id,
            title,
            chapters: 0,
            pages: 0,
            skipped: outcome.skipped_pages,
            already,
        }));
    }

    // The cover: one the person chose, else the book's own when the series has none.
    let has_cover_already = state
        .library
        .peek()
        .find(&series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let mut has_cover = has_cover_already;
    if info.cover.is_some() || !has_cover_already {
        let (chosen, cover_book) = (info.cover.clone(), book.clone());
        let small = tokio::task::spawn_blocking(move || {
            let bytes = chosen.as_deref().map(Vec::as_slice).or(cover_book.cover.as_deref())?;
            logic::small_cover(bytes)
        })
        .await
        .ok()
        .flatten();
        if let Some(small) = small {
            let path = files.cover_path(&series_id);
            let saved = tokio::task::spawn_blocking(move || import::save_cover(&path, &small))
                .await
                .map_err(|e| PipelineError::Import(e.to_string()))
                .and_then(|r| r);
            match saved {
                Ok(()) => has_cover = true,
                Err(e) => {
                    let text = e.to_string();
                    edit_session_for(state, run, |s| {
                        s.errors.push(format!(
                            "The pages were imported, but the cover could not be saved: {}",
                            text.trim_start_matches("import: ")
                        ));
                    });
                }
            }
        }
    }
    let (mut defaults, preset) = state.new_series_defaults();
    if let Some(rtl) = book.meta.right_to_left {
        defaults.direction = if rtl {
            converter::xtc::ReadDirection::RightToLeft
        } else {
            converter::xtc::ReadDirection::LeftToRight
        };
    }
    let details = library::SeriesInfo {
        description: book.meta.description.clone(),
        author: book.meta.author.clone(),
        artist: book.meta.artist.clone(),
        status: book.meta.status.clone(),
        genres: book.meta.genres.clone(),
        source_names: Vec::new(),
        has_cover,
        saved_at: now,
    };
    let prints = {
        let book = book.clone();
        tokio::task::spawn_blocking(move || import::book_fingerprints(&book))
            .await
            .unwrap_or_default()
    };
    let content_prints = {
        let book = book.clone();
        tokio::task::spawn_blocking(move || import::content_prints(&book)).await.unwrap_or_default()
    };
    let pages: usize = outcome.chapters.iter().map(|c| c.pages).sum();
    let chapters = outcome.chapters.len();
    let keys: Vec<String> = outcome.chapters.iter().map(|c| c.key.clone()).collect();
    // Recorded only if the series is still there (or this import is making it):
    // one deleted while the import ran is not brought back.
    let recorded = {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_imported_to(
            &series_id,
            &title,
            &defaults,
            Some(details),
            book.source_names(),
            outcome.chapters,
            now,
            new_series,
        );
        if let Some(id) = &id {
            db.note_imported_books(id, prints);
            db.note_content_prints(id, content_prints);
            if new_series {
                db.adopt_default_preset(id, &defaults, &preset);
            }
        }
        id.is_some()
    };
    if !recorded {
        let (gone_files, gone_series) = (files.clone(), series_id.clone());
        let _ = tokio::task::spawn_blocking(move || {
            for key in keys {
                let _ = gone_files.remove_chapter(&gone_series, &key);
            }
        })
        .await;
        return Err(PipelineError::Import(format!(
            "{title} was deleted while its pages were being imported, so they were not kept."
        )));
    }
    state.save_library();
    load_saved_cover(state, &series_id);
    Ok(Some(ImportDone {
        series_id,
        title,
        chapters,
        pages,
        skipped: outcome.skipped_pages,
        already,
    }))
}

/// Save what the person edited about a series (its title, details and cover).
pub fn save_series_details(state: AppState, series_id: &str, draft: &InfoDraft) {
    let files = library_files();
    let mut has_cover = state
        .library
        .peek()
        .find(series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    if let Some(cover) = &draft.cover {
        match import::save_cover(&files.cover_path(series_id), cover.as_slice()) {
            Ok(()) => has_cover = true,
            Err(e) => state.error(format!(
                "Could not save the cover: {}",
                e.to_string().trim_start_matches("import: ")
            )),
        }
    }
    {
        let mut library = state.library;
        let mut db = library.write();
        if !draft.title.trim().is_empty() {
            db.set_title(series_id, &draft.title);
        }
        let mut info = db
            .find(series_id)
            .and_then(|s| s.info.clone())
            .unwrap_or_default();
        info.description = draft.description.trim().to_string();
        info.author = draft.author.trim().to_string();
        info.artist = draft.artist.trim().to_string();
        info.status = draft.status.trim().to_string();
        info.genres = draft.genres.clone();
        info.has_cover = has_cover;
        info.saved_at = now_secs();
        db.set_info(series_id, info);
    }
    state.save_library();
    load_saved_cover(state, series_id);
    // The page's title follows the new name.
    let mut workspace = state.workspace;
    let mut guard = workspace.write();
    if let Some(w) = guard.as_mut() {
        if w.series_id == series_id && !draft.title.trim().is_empty() {
            w.title = draft.title.trim().to_string();
        }
    }
}

/// The first page of a series as a cover (a small JPEG), for when a book has no
/// cover of its own. Read and shrunk on a worker thread.
pub async fn first_page_cover(series_id: String) -> Option<(Arc<Vec<u8>>, String)> {
    tokio::task::spawn_blocking(move || {
        let files = library_files();
        let chapter = first_chapter_with_pages(&series_id)?;
        let page = files.page_files(&series_id, &chapter).into_iter().next()?;
        const MAX: u64 = 40 * 1024 * 1024;
        if std::fs::metadata(&page).ok()?.len() > MAX {
            return None;
        }
        cover_from_bytes(&std::fs::read(&page).ok()?)
    })
    .await
    .ok()
    .flatten()
}

fn first_chapter_with_pages(series_id: &str) -> Option<String> {
    let files = library_files();
    let dir = files.series_dir(series_id);
    let mut chapters: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        // The folder an import is writing into is not a chapter yet.
        .filter(|name| !name.starts_with("_import-"))
        .collect();
    chapters.sort_by(|a, b| import::natural_cmp(a, b));
    chapters.into_iter().find(|c| !files.page_files(series_id, c).is_empty())
}
