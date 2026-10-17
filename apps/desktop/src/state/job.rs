//! The running download or export, and the exports waiting behind it.

use crate::logic::JobProgress;
use pipeline::CancelFlag;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobKind {
    Export,
}

/// A series whose export is lined up behind the one running.
#[derive(Clone, Debug, PartialEq)]
pub struct WaitingExport {
    pub series_id: String,
    pub title: String,
    /// The chapters to export, by key.
    pub keys: Vec<String>,
}

#[derive(Clone)]
pub struct JobView {
    pub kind: JobKind,
    pub title: String,
    /// The series being exported, for its cover.
    pub series_id: String,
    /// Stopped for now; it goes on from where it was when resumed (even after
    /// the app is closed and opened again).
    pub paused: bool,
    pub progress: JobProgress,
    /// `None` while running, then a one-line summary or the error.
    pub result: Option<Result<String, String>>,
    pub cancel: CancelFlag,
}
