//! The reader: which series and chapter are open and where "back" goes.

use super::{AppState, Page};
use dioxus::prelude::*;
use std::sync::Mutex;

/// A reading position that has not been put in the library yet.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingReading {
    pub series_id: String,
    pub chapter: String,
    pub page: usize,
    pub scroll: f32,
    pub pages: usize,
}

/// The newest position, held until it is put in the library (see
/// [`AppState::note_reading`]).
static PENDING: Mutex<Option<PendingReading>> = Mutex::new(None);

/// What the reader screen was opened on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderOpen {
    pub series_id: String,
    /// The chapter to open; `None` continues where the user left off.
    pub chapter: Option<String>,
    /// The page "back" returns to.
    pub back: Page,
    /// Changes each time the reader is opened, so opening the same chapter
    /// again starts the screen afresh.
    pub opened: u64,
}

impl AppState {
    /// Open the reader on a downloaded series: on `chapter` if given, otherwise
    /// where the user left off.
    pub fn open_reader(&self, series_id: &str, chapter: Option<String>) {
        let has_chapters = self
            .library
            .peek()
            .find(series_id)
            .is_some_and(|s| !s.chapters.is_empty());
        if !has_chapters {
            self.error("There is nothing downloaded to read for this series yet.");
            return;
        }
        let current = *self.page.peek();
        let back = if current == Page::Reader {
            self.reader.peek().as_ref().map_or(Page::Library, |r| r.back)
        } else {
            current
        };
        let mut reader = self.reader;
        reader.set(Some(ReaderOpen {
            series_id: series_id.to_string(),
            chapter,
            back,
            opened: next_opened(),
        }));
        self.go(Page::Reader);
    }

    /// The user is at this place. It is kept at once, but put in the library
    /// (and so on its way to disk) only every couple of seconds, since it
    /// changes many times a second while scrolling; leaving the reader and
    /// closing the window put the newest in right away.
    pub fn note_reading(&self, place: PendingReading) {
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(place);
        }
        self.save_progress();
    }

    /// Put the held position in the library. True if there was one.
    pub(super) fn apply_pending_reading(&self) -> bool {
        let Some(place) = PENDING.lock().ok().and_then(|mut slot| slot.take()) else {
            return false;
        };
        let mut library = self.library;
        library.write().with_reading(&place.series_id, |p| {
            p.record(&place.chapter, place.page, place.scroll, place.pages, super::now_secs());
        });
        true
    }

    /// Leave the reader for the page it was opened from.
    pub fn close_reader(&self) {
        self.flush_reading();
        let back = self.reader.peek().as_ref().map_or(Page::Library, |r| r.back);
        let mut reader = self.reader;
        reader.set(None);
        // The page it came from may be gone (a series closed meanwhile).
        let back = if back == Page::Series && self.workspace.peek().is_none() {
            Page::Library
        } else {
            back
        };
        self.go(back);
    }
}

/// A number that is different each time the reader is opened.
fn next_opened() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}
