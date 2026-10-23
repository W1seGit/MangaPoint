//! The library as the page sees it: the books kept in this browser, and where
//! the visitor is in each (the engine's reading-state records, the same ones
//! that go into a `.mpstate` file). Plain data and plain functions, tested
//! natively; the screens only call these.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use web_engine::state::{self, BookState, Position, Settings, State};

// ---- books ------------------------------------------------------------------------

/// One chapter of a book: where it starts in the book's pages and how long it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub name: String,
    pub number: Option<f64>,
    pub first: usize,
    pub count: usize,
}

impl Chapter {
    /// "Ch. 12" for a numbered chapter, its name otherwise (the desktop's rule).
    pub fn label(&self, index: usize) -> String {
        match self.number {
            Some(n) if n.fract().abs() < 0.0005 => format!("Ch. {}", n.round() as i64),
            Some(n) => format!(
                "Ch. {}",
                format!("{n:.3}").trim_end_matches('0').trim_end_matches('.')
            ),
            None if !self.name.trim().is_empty() => self.name.trim().to_string(),
            None => format!("Chapter {}", index + 1),
        }
    }
}

/// A book kept in this browser (what the library shows).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    /// The file's fingerprint (64 hex digits): the same on every device.
    pub id: String,
    pub title: String,
    pub file_name: String,
    /// `zip`, `epub` or `images`.
    pub kind: String,
    pub size: u64,
    /// Unix milliseconds.
    pub added_at: u64,
    /// Where the bytes are: `opfs`, `idb`, or `none` (the browser would not keep them).
    pub stored: String,
    pub page_count: usize,
    pub has_cover: bool,
    pub chapters: Vec<Chapter>,
    /// Set by the page after a reload: the book is only a name, its file has to
    /// be imported again.
    pub needs_file: bool,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            file_name: String::new(),
            kind: "zip".into(),
            size: 0,
            added_at: 0,
            stored: "none".into(),
            page_count: 0,
            has_cover: false,
            chapters: Vec::new(),
            needs_file: false,
        }
    }
}

impl Book {
    /// The pages of every chapter, in reading order.
    pub fn counts(&self) -> Vec<usize> {
        self.chapters.iter().map(|c| c.count).collect()
    }
}

// ---- what the library offers -------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryView {
    #[default]
    All,
    Reading,
    Unread,
    Finished,
}

impl LibraryView {
    pub const ALL: [LibraryView; 4] = [
        LibraryView::All,
        LibraryView::Reading,
        LibraryView::Unread,
        LibraryView::Finished,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryView::All => "All",
            LibraryView::Reading => "Reading",
            LibraryView::Unread => "Not started",
            LibraryView::Finished => "Finished",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibrarySort {
    /// The ones read last first; the ones never read follow, newest first.
    #[default]
    LastRead,
    Added,
    Title,
    MostChapters,
    Size,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 5] = [
        LibrarySort::LastRead,
        LibrarySort::Added,
        LibrarySort::Title,
        LibrarySort::MostChapters,
        LibrarySort::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::LastRead => "Recently read",
            LibrarySort::Added => "Recently added",
            LibrarySort::Title => "Title A to Z",
            LibrarySort::MostChapters => "Most chapters",
            LibrarySort::Size => "Biggest",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LibrarySort::LastRead => "read",
            LibrarySort::Added => "added",
            LibrarySort::Title => "title",
            LibrarySort::MostChapters => "chapters",
            LibrarySort::Size => "size",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|s| s.key() == key).unwrap_or_default()
    }
}

/// What the Library card offers: "Read" or "Continue · Ch. 4".
#[derive(Clone, Debug, PartialEq)]
pub struct ReadButton {
    pub label: String,
    /// How much of the book is read, 0..1; `None` when nothing was read.
    pub fraction: Option<f32>,
}

/// Where reading starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub chapter: usize,
    pub page: usize,
    pub scroll: f32,
}

// ---- the library ----------------------------------------------------------------------------

/// The books and the reading state of every book ever opened here or loaded
/// from a state file (including books whose file is not here: those wait).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub books: Vec<Book>,
    pub states: BTreeMap<String, BookState>,
}

/// What loading a state file did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// Titles of the books here whose place was restored.
    pub restored: Vec<String>,
    /// Books in the file that are not here yet: they resume when imported.
    pub waiting: usize,
}

impl Library {
    pub fn find(&self, id: &str) -> Option<&Book> {
        self.books.iter().find(|b| b.id == id)
    }

    pub fn state_of(&self, id: &str) -> Option<&BookState> {
        self.states.get(id)
    }

    /// Add a book (or replace the one with the same fingerprint). The reading
    /// state of the same fingerprint, from an earlier visit or a loaded file,
    /// is kept: this is what makes importing the file again resume the book.
    /// Returns the state that was waiting for it, if there was one.
    pub fn add_book(&mut self, book: Book) -> Option<BookState> {
        let waiting = self.states.get(&book.id).cloned();
        match self.books.iter_mut().find(|b| b.id == book.id) {
            Some(slot) => *slot = book,
            None => self.books.push(book),
        }
        waiting
    }

    /// Take a book out. Its reading state stays (the book then waits for its
    /// file) unless `forget` is set.
    pub fn remove_book(&mut self, id: &str, forget: bool) {
        self.books.retain(|b| b.id != id);
        if forget {
            self.states.remove(id);
        }
    }

    pub fn forget_state(&mut self, id: &str) {
        self.states.remove(id);
    }

    /// Reading state for books that are not in the library: `(state)` of each,
    /// the ones read last first.
    pub fn waiting(&self) -> Vec<&BookState> {
        let mut list: Vec<&BookState> = self
            .states
            .values()
            .filter(|s| self.find(&s.fingerprint).is_none())
            .collect();
        list.sort_by(|a, b| {
            b.last_read_at
                .cmp(&a.last_read_at)
                .then_with(|| a.title.cmp(&b.title))
        });
        list
    }

    /// The state to keep for a book, made when it is first read.
    fn state_for<'a>(&'a mut self, book: &Book) -> &'a mut BookState {
        let s = self.states.entry(book.id.clone()).or_insert_with(|| BookState {
            fingerprint: book.id.clone(),
            ..BookState::default()
        });
        s.size = book.size;
        s.file_name = book.file_name.clone();
        s.title = book.title.clone();
        s.chapters = book.chapters.len() as u32;
        s
    }

    /// Make sure the book has a reading state, so it stays in the library (waiting for its
    /// file) once its file is removed.
    pub fn keep_place(&mut self, book: &Book) {
        self.state_for(book);
    }

    /// Remember where the reader is. `page` counts in the chapter.
    pub fn record_position(&mut self, book: &Book, chapter: usize, page: usize, scroll: f32, now_ms: u64) {
        let name = book
            .chapters
            .get(chapter)
            .map(|c| c.label(chapter))
            .unwrap_or_default();
        let s = self.state_for(book);
        s.position = Position {
            chapter: chapter as u32,
            chapter_name: name,
            page: page as u32,
            scroll: if scroll.is_finite() { scroll.clamp(0.0, 1.0) } else { 0.0 },
        };
        s.last_read_at = now_ms;
    }

    /// Note that a chapter was read to its end.
    pub fn mark_read(&mut self, book: &Book, chapter: usize, now_ms: u64) {
        let s = self.state_for(book);
        let c = chapter as u32;
        if let Err(at) = s.chapters_read.binary_search(&c) {
            s.chapters_read.insert(at, c);
        }
        s.last_read_at = now_ms;
    }

    /// The chapter is not read after all.
    pub fn mark_unread(&mut self, book: &Book, chapter: usize) {
        if let Some(s) = self.states.get_mut(&book.id) {
            s.chapters_read.retain(|c| *c != chapter as u32);
        }
    }

    pub fn is_read(&self, book: &Book, chapter: usize) -> bool {
        self.states
            .get(&book.id)
            .is_some_and(|s| s.chapters_read.contains(&(chapter as u32)))
    }

    /// Change a book's saved reading preferences.
    pub fn set_prefs(&mut self, book: &Book, f: impl FnOnce(&mut state::Prefs)) {
        let s = self.state_for(book);
        f(&mut s.prefs);
    }

    // ---- reading progress ----

    /// How much of the book is read: whole chapters count one each, the one in
    /// progress by how far into it the reader is.
    pub fn fraction(&self, book: &Book) -> f32 {
        let Some(s) = self.states.get(&book.id) else { return 0.0 };
        if book.chapters.is_empty() {
            return 0.0;
        }
        let mut done = 0.0f32;
        for (i, c) in book.chapters.iter().enumerate() {
            if s.chapters_read.contains(&(i as u32)) {
                done += 1.0;
            } else if s.position.chapter as usize == i && c.count > 0 && s.last_read_at > 0 {
                done += ((s.position.page as usize + 1) as f32 / c.count as f32).min(1.0) * 0.99;
            }
        }
        (done / book.chapters.len() as f32).clamp(0.0, 1.0)
    }

    pub fn started(&self, id: &str) -> bool {
        self.states
            .get(id)
            .is_some_and(|s| s.last_read_at > 0 || !s.chapters_read.is_empty())
    }

    pub fn finished(&self, book: &Book) -> bool {
        !book.chapters.is_empty()
            && self.states.get(&book.id).is_some_and(|s| {
                (0..book.chapters.len() as u32).all(|c| s.chapters_read.contains(&c))
            })
    }

    /// Where to start: where they left off (moving on when that chapter is
    /// done), or the first chapter not read, or the beginning. `wanted` is a
    /// chapter the visitor picked.
    pub fn start_place(&self, book: &Book, wanted: Option<usize>) -> Option<Start> {
        let counts = book.counts();
        if counts.is_empty() {
            return None;
        }
        let state = self.states.get(&book.id);
        let begin = |chapter: usize| Start { chapter, page: 0, scroll: 0.0 };
        let resume = |chapter: usize, s: &BookState| Start {
            chapter,
            page: (s.position.page as usize).min(counts[chapter].saturating_sub(1)),
            scroll: if s.position.scroll.is_finite() {
                s.position.scroll.clamp(0.0, 1.0)
            } else {
                0.0
            },
        };
        if let Some(c) = wanted {
            let c = c.min(counts.len() - 1);
            return Some(match state {
                Some(s) if s.last_read_at > 0 && s.position.chapter as usize == c => resume(c, s),
                _ => begin(c),
            });
        }
        let Some(s) = state.filter(|s| s.last_read_at > 0 || !s.chapters_read.is_empty()) else {
            return Some(begin(0));
        };
        let at = s.position.chapter as usize;
        if s.last_read_at > 0 && at < counts.len() {
            let last = counts[at].saturating_sub(1);
            let finished = s.chapters_read.contains(&(at as u32)) && s.position.page as usize >= last;
            if !finished {
                return Some(resume(at, s));
            }
            return Some(if at + 1 < counts.len() { begin(at + 1) } else { resume(at, s) });
        }
        let first_unread = (0..counts.len())
            .find(|c| !s.chapters_read.contains(&(*c as u32)))
            .unwrap_or(0);
        Some(begin(first_unread))
    }

    pub fn read_button(&self, book: &Book) -> Option<ReadButton> {
        if book.chapters.is_empty() || book.needs_file {
            return None;
        }
        if !self.started(&book.id) {
            return Some(ReadButton { label: "Read".into(), fraction: None });
        }
        let start = self.start_place(book, None)?;
        Some(ReadButton {
            label: format!("Continue \u{b7} {}", book.chapters[start.chapter].label(start.chapter)),
            fraction: Some(self.fraction(book)),
        })
    }

    // ---- sorting and filtering ----

    /// Indices into `books`, in the order the Library shows them.
    pub fn order(&self, view: LibraryView, sort: LibrarySort, filter: &str) -> Vec<usize> {
        let needle = filter.trim().to_lowercase();
        let last_read = |b: &Book| self.states.get(&b.id).map_or(0, |s| s.last_read_at);
        let mut idx: Vec<usize> = (0..self.books.len())
            .filter(|&i| {
                let b = &self.books[i];
                let in_view = match view {
                    LibraryView::All => true,
                    LibraryView::Reading => self.started(&b.id) && !self.finished(b),
                    LibraryView::Unread => !self.started(&b.id),
                    LibraryView::Finished => self.finished(b),
                };
                in_view
                    && (needle.is_empty()
                        || b.title.to_lowercase().contains(&needle)
                        || b.file_name.to_lowercase().contains(&needle))
            })
            .collect();
        idx.sort_by(|&a, &b| {
            let (x, y) = (&self.books[a], &self.books[b]);
            match sort {
                LibrarySort::LastRead => last_read(y)
                    .cmp(&last_read(x))
                    .then(y.added_at.cmp(&x.added_at)),
                LibrarySort::Added => y.added_at.cmp(&x.added_at),
                LibrarySort::Title => x.title.to_lowercase().cmp(&y.title.to_lowercase()),
                LibrarySort::MostChapters => y.chapters.len().cmp(&x.chapters.len()),
                LibrarySort::Size => y.size.cmp(&x.size),
            }
            .then(x.title.to_lowercase().cmp(&y.title.to_lowercase()))
            .then(x.id.cmp(&y.id))
        });
        idx
    }

    // ---- the state file ----

    /// Everything to write into a `.mpstate` file.
    pub fn to_state(&self, settings: Settings, saved_at: &str) -> State {
        // Keep titles and sizes fresh for books that are here.
        let books: Vec<BookState> = self
            .states
            .values()
            .map(|s| {
                let mut s = s.clone();
                if let Some(b) = self.find(&s.fingerprint) {
                    s.size = b.size;
                    s.title = b.title.clone();
                    s.file_name = b.file_name.clone();
                    s.chapters = b.chapters.len() as u32;
                }
                s
            })
            .collect();
        state::from_records(books, settings, saved_at)
    }

    /// Take a loaded state file in: merge it with what is here (the newer
    /// record of each book wins, chapters read are united). Books that are here
    /// resume at once; the others wait for their file.
    pub fn apply_loaded(&mut self, loaded: &State, settings: Settings, now_iso: &str) -> (Loaded, Settings) {
        let here = self.to_state(settings, now_iso);
        let merged = state::merge(&here, loaded);
        let mut out = Loaded::default();
        for incoming in &loaded.books {
            match self.find(&incoming.fingerprint) {
                Some(book) => out.restored.push(book.title.clone()),
                None => out.waiting += 1,
            }
        }
        self.states = merged
            .books
            .into_iter()
            .map(|b| (b.fingerprint.clone(), b))
            .collect();
        (out, merged.settings)
    }
}

pub use web_engine::state::iso_of_ms;

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(n: u8) -> String {
        state::sample_fingerprint(n)
    }

    fn book(n: u8, title: &str, chapters: &[usize]) -> Book {
        let mut first = 0;
        let chapters = chapters
            .iter()
            .enumerate()
            .map(|(i, &count)| {
                let c = Chapter { name: format!("Chapter {}", i + 1), number: Some((i + 1) as f64), first, count };
                first += count;
                c
            })
            .collect::<Vec<_>>();
        Book {
            id: fp(n),
            title: title.into(),
            file_name: format!("{title}.cbz"),
            size: 1000 * n as u64,
            added_at: n as u64,
            stored: "opfs".into(),
            page_count: first,
            chapters,
            ..Book::default()
        }
    }

    #[test]
    fn labels_follow_the_desktop() {
        let c = |name: &str, number| Chapter { name: name.into(), number, first: 0, count: 1 };
        assert_eq!(c("x", Some(12.0)).label(0), "Ch. 12");
        assert_eq!(c("x", Some(12.5)).label(0), "Ch. 12.5");
        assert_eq!(c("Prologue", None).label(0), "Prologue");
        assert_eq!(c(" ", None).label(3), "Chapter 4");
    }

    #[test]
    fn a_book_never_read_offers_read_and_a_started_one_continues() {
        let mut lib = Library::default();
        let b = book(1, "Alpha", &[10, 10, 10]);
        lib.add_book(b.clone());
        let btn = lib.read_button(&b).unwrap();
        assert_eq!(btn.label, "Read");
        assert_eq!(btn.fraction, None);
        lib.record_position(&b, 1, 4, 0.0, 5_000);
        let btn = lib.read_button(&b).unwrap();
        assert_eq!(btn.label, "Continue \u{b7} Ch. 2");
        assert!(btn.fraction.unwrap() > 0.1 && btn.fraction.unwrap() < 0.34);
        let start = lib.start_place(&b, None).unwrap();
        assert_eq!((start.chapter, start.page), (1, 4));
    }

    #[test]
    fn a_finished_chapter_moves_on_and_the_last_one_stays() {
        let mut lib = Library::default();
        let b = book(1, "Alpha", &[3, 3]);
        lib.record_position(&b, 0, 2, 0.0, 10);
        lib.mark_read(&b, 0, 11);
        let s = lib.start_place(&b, None).unwrap();
        assert_eq!((s.chapter, s.page), (1, 0));
        lib.record_position(&b, 1, 2, 0.0, 12);
        lib.mark_read(&b, 1, 13);
        let s = lib.start_place(&b, None).unwrap();
        assert_eq!((s.chapter, s.page), (1, 2), "the last chapter is resumed, not skipped");
        assert!(lib.finished(&b));
        assert_eq!(lib.fraction(&b), 1.0);
    }

    #[test]
    fn a_picked_chapter_resumes_only_if_it_was_the_one_being_read() {
        let mut lib = Library::default();
        let b = book(1, "Alpha", &[5, 5]);
        lib.record_position(&b, 1, 3, 0.0, 10);
        assert_eq!(lib.start_place(&b, Some(1)).unwrap().page, 3);
        assert_eq!(lib.start_place(&b, Some(0)).unwrap().page, 0);
        assert_eq!(lib.start_place(&b, Some(99)).unwrap().chapter, 1, "held to what exists");
    }

    #[test]
    fn pages_past_the_end_of_a_shrunk_chapter_are_held_in() {
        let mut lib = Library::default();
        let b = book(1, "Alpha", &[10]);
        lib.record_position(&b, 0, 9, 0.5, 10);
        let smaller = book(1, "Alpha", &[4]);
        assert_eq!(lib.start_place(&smaller, None).unwrap().page, 3);
    }

    #[test]
    fn marking_read_keeps_the_list_sorted_without_repeats() {
        let mut lib = Library::default();
        let b = book(1, "A", &[1, 1, 1, 1]);
        for c in [3, 1, 3, 0] {
            lib.mark_read(&b, c, 1);
        }
        assert_eq!(lib.states[&b.id].chapters_read, vec![0, 1, 3]);
    }

    #[test]
    fn filtering_sorting_and_views() {
        let mut lib = Library::default();
        for (n, t) in [(1, "Charlie"), (2, "alpha"), (3, "Bravo")] {
            lib.add_book(book(n, t, &[2, 2]));
        }
        let titles = |o: Vec<usize>, l: &Library| o.iter().map(|&i| l.books[i].title.clone()).collect::<Vec<_>>();
        assert_eq!(
            titles(lib.order(LibraryView::All, LibrarySort::Title, ""), &lib),
            ["alpha", "Bravo", "Charlie"]
        );
        assert_eq!(
            titles(lib.order(LibraryView::All, LibrarySort::Added, ""), &lib),
            ["Bravo", "alpha", "Charlie"]
        );
        assert_eq!(titles(lib.order(LibraryView::All, LibrarySort::Title, " BRA "), &lib), ["Bravo"]);
        let b = lib.books[0].clone();
        lib.record_position(&b, 0, 1, 0.0, 99);
        assert_eq!(titles(lib.order(LibraryView::Reading, LibrarySort::Title, ""), &lib), ["Charlie"]);
        assert_eq!(titles(lib.order(LibraryView::Unread, LibrarySort::Title, ""), &lib), ["alpha", "Bravo"]);
        assert!(lib.order(LibraryView::Finished, LibrarySort::Title, "").is_empty());
        // Recently read puts the one read first.
        assert_eq!(titles(lib.order(LibraryView::All, LibrarySort::LastRead, ""), &lib)[0], "Charlie");
        assert_eq!(LibrarySort::from_key("size"), LibrarySort::Size);
        assert_eq!(LibrarySort::from_key("nonsense"), LibrarySort::LastRead);
    }

    #[test]
    fn a_removed_book_keeps_its_place_and_waits_for_its_file() {
        let mut lib = Library::default();
        let b = book(1, "Alpha", &[5, 5]);
        lib.add_book(b.clone());
        lib.record_position(&b, 1, 2, 0.0, 77);
        lib.remove_book(&b.id, false);
        assert!(lib.books.is_empty());
        assert_eq!(lib.waiting().len(), 1);
        assert_eq!(lib.waiting()[0].title, "Alpha");
        // Importing the same file again finds the place.
        let waiting = lib.add_book(b.clone()).unwrap();
        assert_eq!((waiting.position.chapter, waiting.position.page), (1, 2));
        assert!(lib.waiting().is_empty());
        assert_eq!(lib.start_place(&b, None).unwrap().page, 2);
        lib.remove_book(&b.id, true);
        assert!(lib.waiting().is_empty());
    }

    #[test]
    fn a_state_file_round_trip_restores_places_and_lists_missing_books() {
        let mut phone = Library::default();
        let a = book(1, "Alpha", &[5, 5, 5]);
        let z = book(2, "Zulu", &[4]);
        phone.add_book(a.clone());
        phone.add_book(z.clone());
        phone.record_position(&a, 2, 3, 0.25, 2_000);
        phone.mark_read(&a, 0, 1_900);
        phone.mark_read(&a, 1, 1_950);
        phone.record_position(&z, 0, 1, 0.0, 1_000);
        let file = phone.to_state(Settings::default(), "2026-10-19T12:00:00.000Z");
        let text = state::to_json_pretty(&file);
        assert!(text.contains("\"format\": \"mangapoint-state\""));

        // A new browser has only Alpha.
        let mut laptop = Library::default();
        laptop.add_book(a.clone());
        let parsed = state::parse_str(&text).unwrap();
        let (loaded, _) = laptop.apply_loaded(&parsed, Settings::default(), "2026-10-19T13:00:00.000Z");
        assert_eq!(loaded.restored, ["Alpha"]);
        assert_eq!(loaded.waiting, 1);
        let s = laptop.start_place(&a, None).unwrap();
        assert_eq!((s.chapter, s.page), (2, 3));
        assert_eq!(laptop.states[&a.id].chapters_read, vec![0, 1]);
        let w = laptop.waiting();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].title, "Zulu");
        assert_eq!(w[0].position.page, 1);
        // Importing Zulu resumes it.
        let waiting = laptop.add_book(z.clone()).unwrap();
        assert_eq!(waiting.position.page, 1);
        assert_eq!(laptop.start_place(&z, None).unwrap().page, 1);
    }

    #[test]
    fn loading_an_older_file_does_not_undo_newer_reading_here() {
        let mut lib = Library::default();
        let a = book(1, "Alpha", &[5, 5]);
        lib.add_book(a.clone());
        lib.record_position(&a, 0, 1, 0.0, 1_000);
        let old = lib.to_state(Settings::default(), "2026-01-01T00:00:00.000Z");
        lib.record_position(&a, 1, 4, 0.0, 9_000);
        lib.apply_loaded(&old, Settings::default(), "2026-10-19T00:00:00.000Z");
        let s = lib.start_place(&a, None).unwrap();
        assert_eq!((s.chapter, s.page), (1, 4));
    }

    #[test]
    fn a_book_that_needs_its_file_has_no_read_button() {
        let mut b = book(1, "A", &[2]);
        b.needs_file = true;
        assert!(Library::default().read_button(&b).is_none());
    }

    #[test]
    fn iso_dates_match_the_engines_reader() {
        for ms in [0u64, 1_781_000_123_456, 951_782_400_000, 4_000_000_000_000] {
            assert_eq!(state::iso_to_ms(&iso_of_ms(ms)), ms, "{ms}");
        }
        assert_eq!(iso_of_ms(0), "1970-01-01T00:00:00.000Z");
    }
}
