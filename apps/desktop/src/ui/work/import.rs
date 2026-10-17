//! Importing: reading what the person chose into drafts they can correct, then
//! writing the books into the library. The reading and writing are the
//! pipeline's; this connects them to the dialog.

use super::*;
use crate::state::{
    BookDraft, ChapterDraft, ChapterPlace, ImportDone, ImportSession, ImportStage, InfoDraft,
};
use pipeline::import::{self, CommitTarget, ImportBook, Placement};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The books that were read, behind the drafts the dialog shows (in the same
/// order). They hold the pages' places, which the drafts do not.
static IMPORT_BOOKS: Mutex<Vec<ImportBook>> = Mutex::new(Vec::new());
static IMPORT_CANCEL: Mutex<Option<CancelFlag>> = Mutex::new(None);

/// Change the dialog's state, if it is open.
fn edit_session(state: AppState, change: impl FnOnce(&mut ImportSession)) {
    let mut signal = state.import;
    let mut guard = signal.write();
    if let Some(session) = guard.as_mut() {
        change(session);
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

/// The picture in a file, as a cover. `None` if it is not a picture.
pub fn cover_from_file(path: &std::path::Path) -> Option<(Arc<Vec<u8>>, String)> {
    const MAX: u64 = 40 * 1024 * 1024;
    if std::fs::metadata(path).ok()?.len() > MAX {
        return None;
    }
    cover_from_bytes(&std::fs::read(path).ok()?)
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
        show_details: false,
        show_chapters: book.chapters.len() <= 6,
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
    let mut session = state.import;
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
    // What a series being added to already has: its numbers are taken.
    let existing: Vec<library::DownloadedChapter> = target
        .as_deref()
        .and_then(|id| state.library.peek().find(id).map(|s| s.chapters.clone()))
        .unwrap_or_default();
    let adding = target.is_some();
    spawn_forever(async move {
        let read = tokio::task::spawn_blocking(move || {
            let mut result = import::scan(&paths);
            let taken: Vec<f64> = existing.iter().filter_map(|c| c.number).collect();
            for book in &mut result.books {
                book.number_chapters(&taken);
                if adding {
                    book.meet(&existing);
                }
            }
            let drafts: Vec<BookDraft> = result.books.iter().map(draft_of).collect();
            (result, drafts)
        })
        .await;
        match read {
            Ok((result, drafts)) => {
                if let Ok(mut books) = IMPORT_BOOKS.lock() {
                    *books = result.books;
                }
                let mut failures: Vec<(String, String)> = result
                    .failures
                    .into_iter()
                    .map(|f| (f.file, f.message))
                    .collect();
                if drafts.is_empty() && failures.is_empty() {
                    failures.push((String::new(), "There is nothing to import there.".into()));
                }
                edit_session(state, move |s| {
                    s.books = drafts;
                    s.failures = failures;
                    s.stage = ImportStage::Review;
                });
            }
            Err(e) => {
                edit_session(state, move |s| {
                    s.failures = vec![(String::new(), format!("Reading the files failed: {e}"))];
                    s.stage = ImportStage::Review;
                });
            }
        }
    });
}

/// Close the dialog. Importing in progress is stopped first.
pub fn close_import(state: AppState) {
    cancel_import();
    let mut session = state.import;
    session.set(None);
    if let Ok(mut books) = IMPORT_BOOKS.lock() {
        books.clear();
    }
}

/// Stop a running import at the next page.
pub fn cancel_import() {
    if let Ok(cancel) = IMPORT_CANCEL.lock() {
        if let Some(flag) = cancel.as_ref() {
            flag.cancel();
        }
    }
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
    let books: Vec<ImportBook> = IMPORT_BOOKS.lock().map(|b| b.clone()).unwrap_or_default();
    let mut units: Vec<Unit> = Vec::new();
    let mut edited: Vec<(ImportBook, InfoDraft)> = Vec::new();
    for (book, draft) in books.iter().zip(&session.books) {
        if !draft.include {
            continue;
        }
        let mut book = book.clone();
        apply_edits(&mut book, draft);
        edited.push((book, draft.info.clone()));
    }
    if session.combine && edited.len() > 1 && session.target.is_none() {
        let info = edited[0].1.clone();
        let merged = ImportBook::merge(edited.into_iter().map(|(b, _)| b).collect());
        units.extend(merged.map(|book| Unit { book, info }));
    } else {
        units.extend(edited.into_iter().map(|(book, info)| Unit { book, info }));
    }
    let total: usize = units.iter().map(|u| u.book.included_pages()).sum();
    if units.is_empty() || total == 0 {
        edit_session(state, |s| s.errors.push("Nothing is chosen to import.".into()));
        return;
    }
    let cancel = CancelFlag::new();
    if let Ok(mut slot) = IMPORT_CANCEL.lock() {
        *slot = Some(cancel.clone());
    }
    edit_session(state, |s| {
        s.stage = ImportStage::Importing;
        s.done_pages = 0;
        s.total_pages = total;
        s.finished.clear();
        s.errors.clear();
    });
    let target = session.target.clone();
    spawn_forever(async move {
        let mut base = 0usize;
        for unit in units {
            let pages = unit.book.included_pages();
            match import_unit(state, unit, target.clone(), base, &cancel).await {
                Ok(Some(done)) => edit_session(state, |s| s.finished.push(done)),
                Ok(None) => {}
                Err(PipelineError::Cancelled) => {
                    edit_session(state, |s| {
                        s.errors.push("Cancelled. What was started was taken away again.".into());
                    });
                    break;
                }
                Err(message) => edit_session(state, |s| s.errors.push(message.to_string())),
            }
            base += pages;
        }
        let summary = {
            let session = state.import.peek();
            session.as_ref().map(|s| {
                let chapters: usize = s.finished.iter().map(|d| d.chapters).sum();
                (chapters, s.finished.first().map(|d| d.title.clone()))
            })
        };
        edit_session(state, |s| {
            s.stage = ImportStage::Finished;
            s.done_pages = s.total_pages;
        });
        if let Some((chapters, title)) = summary.filter(|(c, _)| *c > 0) {
            state.notify(format!(
                "Imported {} into {}.",
                logic::plural(chapters, "chapter"),
                title.unwrap_or_else(|| "your library".into())
            ));
        }
        if let Ok(mut slot) = IMPORT_CANCEL.lock() {
            *slot = None;
        }
    });
}

/// Make or add to one series from one unit. `Ok(None)` when there was nothing
/// new to add.
async fn import_unit(
    state: AppState,
    unit: Unit,
    target: Option<String>,
    base: usize,
    cancel: &CancelFlag,
) -> Result<Option<ImportDone>, PipelineError> {
    let Unit { mut book, info } = unit;
    let files = library_files();
    // Which series: the one being added to, one imported before with this title,
    // or a new one (with a free title if another series has the name).
    let (series_id, title, new_series) = {
        let library = state.library.peek();
        match target.as_deref().and_then(|id| library.find(id)) {
            Some(series) => (series.id.clone(), series.title.clone(), false),
            None => {
                let wanted = if book.meta.title.trim().is_empty() {
                    import::series_title(&book.label)
                } else {
                    book.meta.title.trim().to_string()
                };
                match library.find(&library::series_id(&wanted)) {
                    Some(s) if s.is_imported() => (s.id.clone(), s.title.clone(), false),
                    Some(_) => {
                        let title = library.free_title(&wanted);
                        (library::series_id(&title), title, true)
                    }
                    None => (library::series_id(&wanted), wanted, true),
                }
            }
        }
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
    let had_decision: Vec<bool> = book.chapters.iter().map(|c| c.meets.is_some()).collect();
    let mut probe = book.clone();
    probe.meet(&existing);
    for ((chapter, probed), decided) in book.chapters.iter_mut().zip(&probe.chapters).zip(had_decision) {
        if !decided && probed.meets.is_some() {
            chapter.meets = probed.meets.clone();
            chapter.skip = true;
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
    let (task_files, task_book, task_series, task_cancel) =
        (files.clone(), book.clone(), series_id.clone(), cancel.clone());
    let (task_existing, task_progress) = (existing.clone(), progress.clone());
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
        edit_session(state, |s| s.done_pages = done);
    }
    let outcome = handle
        .await
        .map_err(|e| PipelineError::Import(e.to_string()))??;
    if outcome.chapters.is_empty() {
        return Ok(None);
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
        let source = info.cover.as_ref().map(|c| c.to_vec()).or_else(|| book.cover.clone());
        let small = match source {
            Some(bytes) => tokio::task::spawn_blocking(move || logic::small_cover(&bytes))
                .await
                .ok()
                .flatten(),
            None => None,
        };
        if let Some(small) = small {
            let target = files.cover_path(&series_id);
            if let Some(dir) = target.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            has_cover = std::fs::write(&target, &small).is_ok();
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
    let pages: usize = outcome.chapters.iter().map(|c| c.pages).sum();
    let chapters = outcome.chapters.len();
    {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_imported(
            &title,
            &defaults,
            Some(details),
            book.source_names(),
            outcome.chapters,
            now,
        );
        if new_series {
            db.adopt_default_preset(&id, &defaults, &preset);
        }
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
        let target = files.cover_path(series_id);
        if let Some(dir) = target.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        has_cover = std::fs::write(&target, cover.as_slice()).is_ok() || has_cover;
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
/// cover of its own.
pub fn first_page_cover(series_id: &str) -> Option<(Arc<Vec<u8>>, String)> {
    let files = library_files();
    let series = library_files_series_first_chapter(series_id)?;
    let page = files.page_files(series_id, &series).into_iter().next()?;
    cover_from_file(&page)
}

fn library_files_series_first_chapter(series_id: &str) -> Option<String> {
    let files = library_files();
    let dir = files.series_dir(series_id);
    let mut chapters: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    chapters.sort_by(|a, b| import::natural_cmp(a, b));
    chapters.into_iter().find(|c| !files.page_files(series_id, c).is_empty())
}
