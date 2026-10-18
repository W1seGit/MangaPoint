//! Choosing what to download: the dialog and the planner.

use super::workspace::LoadedChapters;
use crate::logic::SortKey;
use std::collections::HashSet;
use std::rc::Rc;

/// What a download dialog will start once the user confirms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PendingDownload {
    /// The chapters ticked on the open series.
    Chapters,
    /// Everything chosen in the download planner.
    Plan,
}

/// The dialog that asks where downloaded files go, before they start.
#[derive(Clone, PartialEq, Debug)]
pub struct DownloadDialog {
    pub pending: PendingDownload,
    /// What is about to download, in words.
    pub summary: String,
}

/// What the download planner knows about one series.
#[derive(Clone)]
pub enum PlanChapters {
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// One series in the download planner, with the chapters ticked for it.
#[derive(Clone)]
pub struct PlanEntry {
    pub series_id: String,
    pub title: String,
    pub chapters: PlanChapters,
    /// Positions in the merged chapter list.
    pub picked: HashSet<usize>,
    pub open: bool,
    /// The words typed in this series' filter box.
    pub filter: String,
    pub sort: (SortKey, bool),
}
