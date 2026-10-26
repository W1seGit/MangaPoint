//! Where the user is in each series they read, and how they like to read it.
//!
//! Kept in the library file next to the series (see [`crate::LibraryDb::reading`]),
//! keyed by series id, so older library files load unchanged and deleting a
//! series or chapter takes its progress with it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One page at a time, or one long strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadMode {
    Paged,
    Scroll,
}

/// Which way pages turn in [`ReadMode::Paged`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageDirection {
    LeftToRight,
    RightToLeft,
}

/// How a page is fitted to the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadFit {
    #[default]
    Width,
    Height,
    Original,
}

/// The reader's choices for one series. A choice that was never made is
/// `None`, and the app picks a default from what the series is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderPrefs {
    pub mode: Option<ReadMode>,
    pub direction: Option<PageDirection>,
    pub fit: Option<ReadFit>,
    /// Zoom on top of the fit, as a percentage (100 = none).
    pub zoom: Option<u16>,
    /// The user closed the note about pages saved small for e-readers.
    pub hide_small_banner: bool,
}

/// How far the user got in one chapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChapterRead {
    /// The furthest page reached (0-based).
    pub furthest: usize,
    /// Finished: the last page was reached.
    pub read: bool,
}

/// Where the user is in one series.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadProgress {
    /// The chapter being read, by key.
    pub chapter: Option<String>,
    /// The page in it (0-based), for paged reading.
    pub page: usize,
    /// How far down the chapter in scroll reading, 0.0 to 1.0.
    pub scroll: f32,
    pub chapters: BTreeMap<String, ChapterRead>,
    /// Seconds since the Unix epoch; 0 until something was read.
    pub last_read_at: u64,
    pub prefs: ReaderPrefs,
}

/// A fraction held to 0..=1; anything else (NaN, huge) becomes 0.
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl ReadProgress {
    /// Note where the user is now, in the chapter `key` that has `pages` pages.
    /// `page` and `scroll` are the same place in two ways (in a strip, `page`
    /// is the last page in view); reaching the last page counts the chapter as read.
    pub fn record(&mut self, key: &str, page: usize, scroll: f32, pages: usize, now: u64) {
        let last = pages.saturating_sub(1);
        let page = page.min(last);
        let scroll = unit(scroll);
        self.chapter = Some(key.to_string());
        self.page = page;
        self.scroll = scroll;
        self.last_read_at = now;
        let entry = self.chapters.entry(key.to_string()).or_default();
        entry.furthest = entry.furthest.max(page);
        if pages > 0 && page == last {
            entry.read = true;
        }
    }

    /// Mark a chapter as read, with `pages` pages.
    pub fn mark_read(&mut self, key: &str, pages: usize, now: u64) {
        let entry = self.chapters.entry(key.to_string()).or_default();
        entry.read = true;
        entry.furthest = entry.furthest.max(pages.saturating_sub(1));
        self.last_read_at = self.last_read_at.max(now);
    }

    /// Mark a chapter as not read any more.
    pub fn mark_unread(&mut self, key: &str) {
        self.chapters.remove(key);
        if self.chapter.as_deref() == Some(key) {
            self.chapter = None;
            self.page = 0;
            self.scroll = 0.0;
        }
    }

    pub fn is_read(&self, key: &str) -> bool {
        self.chapters.get(key).is_some_and(|c| c.read)
    }

    /// Forget one chapter (it was deleted).
    pub fn forget_chapter(&mut self, key: &str) {
        self.chapters.remove(key);
        if self.chapter.as_deref() == Some(key) {
            self.chapter = None;
            self.page = 0;
            self.scroll = 0.0;
        }
    }

    /// Forget everything about chapters the series no longer has.
    pub fn retain_chapters(&mut self, has: impl Fn(&str) -> bool) {
        self.chapters.retain(|k, _| has(k));
        if self.chapter.as_deref().is_some_and(|k| !has(k)) {
            self.chapter = None;
            self.page = 0;
            self.scroll = 0.0;
        }
    }

    /// Nothing read and nothing chosen: not worth keeping in the file.
    pub fn is_empty(&self) -> bool {
        self.chapter.is_none()
            && self.chapters.is_empty()
            && self.last_read_at == 0
            && self.prefs == ReaderPrefs::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_keeps_the_furthest_page_and_finishing_marks_the_chapter_read() {
        let mut p = ReadProgress::default();
        p.record("c1", 3, 0.0, 10, 100);
        p.record("c1", 1, 0.0, 10, 101);
        assert_eq!((p.page, p.chapters["c1"].furthest), (1, 3));
        assert!(!p.is_read("c1"));
        p.record("c1", 9, 0.0, 10, 102);
        assert!(p.is_read("c1"));
        assert_eq!(p.last_read_at, 102);
        // A page past the end is held to the last one.
        p.record("c2", 99, 0.5, 4, 103);
        assert_eq!((p.chapter.as_deref(), p.page), (Some("c2"), 3));
    }

    #[test]
    fn a_hostile_fraction_is_held_to_the_strip() {
        let mut p = ReadProgress::default();
        p.record("c1", 0, f32::NAN, 5, 1);
        assert_eq!(p.scroll, 0.0);
        p.record("c1", 0, 7.0, 5, 1);
        assert_eq!(p.scroll, 1.0);
    }

    #[test]
    fn forgetting_a_chapter_resets_the_place_only_if_it_was_the_current_one() {
        let mut p = ReadProgress::default();
        p.record("c1", 2, 0.4, 5, 1);
        p.mark_read("c0", 3, 1);
        p.forget_chapter("c0");
        assert_eq!(p.chapter.as_deref(), Some("c1"));
        p.forget_chapter("c1");
        assert!(p.chapter.is_none() && p.chapters.is_empty() && p.scroll == 0.0);
        p.record("c3", 1, 0.0, 5, 2);
        p.retain_chapters(|k| k == "zzz");
        assert!(p.chapter.is_none() && p.chapters.is_empty());
    }
}
