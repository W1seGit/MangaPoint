//! The queue as it is kept on disk, so a restart does not lose it.

use super::{Batch, BatchStats, BatchStatus, DownloadQueue};
use pipeline::{ChapterRequest, SourceChoice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A batch as it is kept on disk.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SavedBatch {
    id: u64,
    series_id: String,
    title: String,
    requests: Vec<ChapterRequest>,
    pub(super) plan: Vec<String>,
    /// The one source it was limited to, if it was.
    only_source: Option<String>,
    source_names: HashMap<String, String>,
    status: BatchStatus,
    summary: Option<String>,
    stats: Option<BatchStats>,
    skipped: Vec<String>,
    failed: Vec<String>,
    added_at: u64,
    finished_at: u64,
    #[serde(default)]
    quality: Option<library::PageQuality>,
    #[serde(default)]
    replace: bool,
}

/// The queue as it is kept on disk, so a restart does not lose it.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedQueue {
    pub(super) batches: Vec<SavedBatch>,
    next_id: u64,
    paused: bool,
}

impl DownloadQueue {
    pub fn to_saved(&self) -> SavedQueue {
        SavedQueue {
            batches: self
                .batches
                .iter()
                .map(|b| SavedBatch {
                    id: b.id,
                    series_id: b.series_id.clone(),
                    title: b.title.clone(),
                    requests: b.requests.clone(),
                    plan: b.plan.clone(),
                    only_source: match &b.choice {
                        SourceChoice::Adaptive => None,
                        SourceChoice::Only(id) => Some(id.clone()),
                    },
                    source_names: b.source_names.clone(),
                    status: b.status.clone(),
                    summary: b.summary.clone(),
                    stats: b.stats,
                    skipped: b.skipped.clone(),
                    failed: b.failed.clone(),
                    added_at: b.added_at,
                    finished_at: b.finished_at,
                    quality: b.quality,
                    replace: b.replace,
                })
                .collect(),
            next_id: self.next_id,
            paused: self.paused,
        }
    }

    /// Bring a saved queue back. A batch that was downloading when the app
    /// closed waits to start again; what it had already saved is kept.
    pub fn from_saved(saved: SavedQueue) -> Self {
        let batches = saved
            .batches
            .into_iter()
            .map(|mut b| {
                // A file edited by hand must not bring the app down.
                let n = b.requests.len().min(b.plan.len());
                b.requests.truncate(n);
                b.plan.truncate(n);
                let mut batch = Batch::new(
                    b.series_id,
                    b.title,
                    b.requests,
                    b.plan,
                    match b.only_source {
                        Some(id) => SourceChoice::Only(id),
                        None => SourceChoice::Adaptive,
                    },
                    b.source_names,
                    b.added_at,
                );
                batch.id = b.id;
                batch.status = match b.status {
                    BatchStatus::Running => BatchStatus::Waiting,
                    other => other,
                };
                batch.summary = b.summary;
                batch.stats = b.stats;
                batch.skipped = b.skipped;
                batch.failed = b.failed;
                batch.finished_at = b.finished_at;
                batch.quality = b.quality;
                batch.replace = b.replace;
                batch
            })
            .collect::<Vec<_>>();
        let next_id = saved
            .next_id
            .max(batches.iter().map(|b| b.id).max().unwrap_or(0));
        Self {
            batches,
            next_id,
            paused: saved.paused,
            runner_active: false,
        }
    }
}
