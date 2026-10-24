//! A batch of chapters of one series, and where its chapters stand.

use crate::logic::JobProgress;
use pipeline::{CancelFlag, ChapterRequest, SourceChoice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    Waiting,
    Running,
    /// Set aside by the user; it goes on from the chapters not yet saved when
    /// it is resumed.
    Paused,
    Done,
    /// Stopped by an error, in words.
    Failed(String),
    Cancelled,
}

impl BatchStatus {
    /// Waiting, paused or running: still has work to do.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            BatchStatus::Waiting | BatchStatus::Running | BatchStatus::Paused
        )
    }
}

/// How a finished batch went.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchStats {
    /// How long it took.
    pub secs: u64,
    /// Pages saved.
    pub pages: usize,
}

/// Some chapters of one series, downloaded together.
#[derive(Clone)]
pub struct Batch {
    pub id: u64,
    pub series_id: String,
    pub title: String,
    /// What to download, each chapter with the sources that can supply it.
    pub requests: Vec<ChapterRequest>,
    /// The library key of each request, in the same order.
    pub plan: Vec<String>,
    pub choice: SourceChoice,
    /// Source id to name, for the sources the chapters may come from.
    pub source_names: HashMap<String, String>,
    pub status: BatchStatus,
    pub progress: JobProgress,
    pub cancel: CancelFlag,
    /// Told to stop so that it can be put aside, not given up.
    pub pausing: bool,
    /// One line about how it went, once it has finished.
    pub summary: Option<String>,
    /// How long it took and how much it saved, once it has finished.
    pub stats: Option<BatchStats>,
    /// Chapters that could not be downloaded, in words.
    pub skipped: Vec<String>,
    /// The same chapters, by key.
    pub failed: Vec<String>,
    /// Seconds since the Unix epoch.
    pub added_at: u64,
    /// When it finished, in the same seconds; 0 until it has.
    pub finished_at: u64,
    /// The page quality chosen for just this download; `None` follows the setting.
    pub quality: Option<library::PageQuality>,
    /// The chapters are downloaded already and are replaced by this download,
    /// each only once its new copy is complete.
    pub replace: bool,
}

impl Batch {
    pub fn new(
        series_id: String,
        title: String,
        requests: Vec<ChapterRequest>,
        plan: Vec<String>,
        choice: SourceChoice,
        source_names: HashMap<String, String>,
        added_at: u64,
    ) -> Self {
        debug_assert_eq!(requests.len(), plan.len());
        Self {
            id: 0,
            series_id,
            title,
            requests,
            plan,
            choice,
            source_names,
            status: BatchStatus::Waiting,
            progress: JobProgress::default(),
            cancel: CancelFlag::new(),
            pausing: false,
            summary: None,
            stats: None,
            skipped: Vec::new(),
            failed: Vec::new(),
            added_at,
            finished_at: 0,
            quality: None,
            replace: false,
        }
    }

    /// Download at this page quality whatever the setting says, replacing
    /// chapters that are already downloaded.
    pub fn replacing_at(mut self, quality: library::PageQuality) -> Self {
        self.quality = Some(quality);
        self.replace = true;
        self
    }

    /// Chapters of another batch may join this one only if they are
    /// downloaded the same way.
    pub fn same_mode(&self, other: &Batch) -> bool {
        self.quality == other.quality && self.replace == other.replace
    }

    pub fn chapter_count(&self) -> usize {
        self.plan.len()
    }

    /// Chapters dealt with so far.
    pub fn chapters_done(&self) -> usize {
        match self.status {
            BatchStatus::Done => self.plan.len() - self.failed.len().min(self.plan.len()),
            _ => self.progress.finished,
        }
    }
}

/// Where one chapter stands in the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueuedChapter {
    Waiting,
    Downloading {
        pages_done: usize,
        pages_total: usize,
    },
    Failed,
}

/// How one series stands in the queue. It changes when something of this
/// series does (a chapter joins, a page of it arrives), and not when the queue
/// does something for another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesQueueView {
    /// Something of it is waiting or downloading.
    pub working: bool,
    /// Finished batches of it with chapters that failed, to try again.
    pub retry_ids: Vec<u64>,
    /// The chapters that are in the queue (waiting, downloading or failed), and
    /// where each stands.
    pub states: std::collections::HashMap<String, QueuedChapter>,
}

/// What adding a batch did.
#[derive(Debug, PartialEq, Eq)]
pub struct Added {
    /// Chapters that were new to the queue.
    pub chapters: usize,
    /// They joined a batch for the same series that was already waiting.
    pub joined_existing: bool,
}
