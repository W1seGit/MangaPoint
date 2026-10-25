//! Types shared by the download and export stages.

use library::DownloadedChapter;
use search::merge::MergedChapter;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// One way to get a chapter: a specific source's copy of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterRef {
    pub source_id: String,
    pub chapter_id: i64,
}

/// A chapter to download, with every source that can supply it (best first).
/// If the first source fails, the next is tried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChapterRequest {
    pub number: Option<f64>,
    pub name: String,
    /// The library key the chapter is saved under. When it is not given it is
    /// worked out from the first source's copy, which changes if the sources
    /// are put in a different order, so callers that reorder them set it.
    #[serde(default)]
    pub key: Option<String>,
    pub options: Vec<ChapterRef>,
}

/// Build requests from a merged chapter list. `member_sources[i]` is the
/// source id of member `i` used when merging.
pub fn requests_from_merged(
    merged: &[MergedChapter],
    member_sources: &[String],
) -> Vec<ChapterRequest> {
    merged
        .iter()
        .map(|m| ChapterRequest {
            number: m.number,
            name: m.name.clone(),
            key: Some(library::chapter_key(
                m.number,
                m.options.first().map_or(0, |o| o.chapter_id),
            )),
            options: m
                .options
                .iter()
                .filter_map(|o| {
                    Some(ChapterRef {
                        source_id: member_sources.get(o.member)?.clone(),
                        chapter_id: o.chapter_id,
                    })
                })
                .collect(),
        })
        .collect()
}

/// Progress reports while a download or export runs.
#[derive(Clone, Debug, PartialEq)]
pub enum JobEvent {
    ChapterStarted {
        index: usize,
        total: usize,
        name: String,
    },
    /// One more page of the current chapter is done.
    PageDone {
        chapter_index: usize,
        done: usize,
        total: usize,
    },
    /// The first source failed, so another is being used.
    UsedFallback {
        chapter_index: usize,
        from: String,
        to: String,
        reason: String,
    },
    ChapterSkipped {
        chapter_index: usize,
        name: String,
        reason: String,
    },
    /// A chapter is completely downloaded and safe on disk.
    ChapterSaved {
        chapter: DownloadedChapter,
    },
    /// A chapter is finished with: saved, skipped, or (for an export) read.
    ChapterDone {
        index: usize,
    },
    /// A source pushed back (or recovered), so the number of requests allowed
    /// at once changed.
    PagesAtOnce {
        now: usize,
        chosen: usize,
    },
    Writing {
        file: String,
    },
    /// How much of the finished files has reached the target, counting all of
    /// them: `done` of `total` bytes.
    Sent {
        done: u64,
        total: u64,
    },
    /// The file is on the drive, but the drive may still be holding the end of
    /// it in memory: it is being forced out, which can take a while.
    Flushing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedChapter {
    pub index: usize,
    pub name: String,
    pub reason: String,
}

/// How a source did, so the caller can update its health record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUse {
    pub source_id: String,
    pub ok: bool,
    pub ms: u64,
}

/// Lets the UI stop a running download or export.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>, Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// Hold the job where it is (at the next chapter) until [`resume`](Self::resume); nothing is lost.
    pub fn pause(&self) {
        self.1.store(true, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        self.1.store(false, Ordering::SeqCst);
    }
    pub fn is_paused(&self) -> bool {
        self.1.load(Ordering::SeqCst)
    }
    /// Returns at once unless paused; then waits for a resume or a cancel.
    pub async fn wait_while_paused(&self) {
        while self.is_paused() && !self.is_cancelled() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    /// Completes once cancelled (checked every 100 ms). Race it against slow
    /// work with `tokio::select!`; the work is dropped when this wins.
    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
