//! The open series: its details, its merged chapter lists and the chapter table.

use super::Page;
use crate::logic::SortKey;
use search::{MergedChapter, RankedSeries};
use std::rc::Rc;

/// What the sources say about a series opened from a search that is not in
/// the library yet: shown on its overview.
#[derive(Clone)]
pub struct OpenDetails {
    pub series_id: String,
    pub info: library::SeriesInfo,
    /// Where the cover comes from (see `covers`).
    pub cover: Option<Rc<str>>,
}

/// The series the user is working on.
#[derive(Clone)]
pub struct Workspace {
    /// The library id (from the title), under which downloads are kept.
    pub series_id: String,
    pub title: String,
    /// Present when opened from a search; needed to find more chapters.
    pub ranked: Option<RankedSeries>,
    /// Where the back button goes.
    pub origin: Page,
}

/// Chapter lists from the sources, merged.
pub struct LoadedChapters {
    pub merged: Vec<MergedChapter>,
    /// Source id of each member, best first; chapter options point into this.
    pub member_sources: Vec<String>,
    pub member_names: Vec<String>,
    /// The same members as they are kept in the library.
    pub saved: Vec<library::SavedSource>,
    pub unreachable: usize,
}

#[derive(Clone)]
pub enum ChaptersState {
    /// Nothing to load (opened from the library).
    Idle,
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// How the chapter table is filtered and sorted.
#[derive(Clone)]
pub struct TableView {
    pub filter: String,
    /// Column, and whether descending.
    pub sort: (SortKey, bool),
}

impl Default for TableView {
    fn default() -> Self {
        Self {
            filter: String::new(),
            sort: (SortKey::Number, true),
        }
    }
}
