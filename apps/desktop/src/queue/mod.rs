//! The download queue.
//!
//! Chapters picked for download are added here as a batch for their series and
//! the queue works through the batches one after another, so there is nothing
//! to start by hand: keep picking chapters, from this series or others, and
//! they download in turn. Everything here is plain data and rules; the work of
//! actually downloading is in `ui/work.rs`.

mod batch;
mod saved;
#[cfg(test)]
mod tests;

pub use batch::*;
pub use saved::SavedQueue;

use crate::logic::JobProgress;
use pipeline::CancelFlag;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Clone, Default)]
pub struct DownloadQueue {
    pub batches: Vec<Batch>,
    next_id: u64,
    /// While paused nothing new starts; a running batch carries on.
    pub paused: bool,
    /// A task is working through the queue right now.
    pub runner_active: bool,
}

impl DownloadQueue {
    /// Add a batch. Chapters of the same series that are already waiting or
    /// downloading are left out, and what is new joins the series' waiting
    /// batch if it has one.
    pub fn add(&mut self, mut batch: Batch) -> Added {
        let taken: Vec<&str> = self
            .batches
            .iter()
            .filter(|b| b.status.is_active() && b.series_id == batch.series_id)
            .flat_map(|b| b.plan.iter().map(String::as_str))
            .collect();
        let mut fresh_requests = Vec::new();
        let mut fresh_plan = Vec::new();
        for (request, key) in batch.requests.drain(..).zip(batch.plan.drain(..)) {
            if !taken.contains(&key.as_str()) && !fresh_plan.contains(&key) {
                fresh_requests.push(request);
                fresh_plan.push(key);
            }
        }
        let chapters = fresh_plan.len();
        if chapters == 0 {
            return Added {
                chapters: 0,
                joined_existing: false,
            };
        }
        if let Some(existing) = self.batches.iter_mut().find(|b| {
            b.status == BatchStatus::Waiting
                && b.series_id == batch.series_id
                && b.choice == batch.choice
                && b.same_mode(&batch)
        }) {
            existing.requests.extend(fresh_requests);
            existing.plan.extend(fresh_plan);
            existing.source_names.extend(batch.source_names);
            return Added {
                chapters,
                joined_existing: true,
            };
        }
        batch.requests = fresh_requests;
        batch.plan = fresh_plan;
        self.next_id += 1;
        batch.id = self.next_id;
        self.batches.push(batch);
        Added {
            chapters,
            joined_existing: false,
        }
    }

    pub fn get(&self, id: u64) -> Option<&Batch> {
        self.batches.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Batch> {
        self.batches.iter_mut().find(|b| b.id == id)
    }

    /// The batch to start next, unless the queue is paused or one is running.
    pub fn next_to_start(&self) -> Option<u64> {
        if self.paused || self.running().is_some() {
            return None;
        }
        self.batches
            .iter()
            .find(|b| b.status == BatchStatus::Waiting)
            .map(|b| b.id)
    }

    pub fn running(&self) -> Option<&Batch> {
        self.batches
            .iter()
            .find(|b| b.status == BatchStatus::Running)
    }

    pub fn start(&mut self, id: u64) {
        if let Some(b) = self.get_mut(id) {
            b.status = BatchStatus::Running;
        }
    }

    pub fn finish(
        &mut self,
        id: u64,
        status: BatchStatus,
        summary: String,
        skipped: Vec<String>,
        failed: Vec<String>,
    ) {
        if let Some(b) = self.get_mut(id) {
            b.status = status;
            b.finished_at = now_secs();
            b.summary = Some(summary);
            b.skipped = skipped;
            b.failed = failed;
        }
    }

    pub fn set_stats(&mut self, id: u64, stats: BatchStats) {
        if let Some(b) = self.get_mut(id) {
            b.stats = Some(stats);
        }
    }

    /// Stop a batch. One that has not started is simply dropped from the
    /// queue; one that is running is told to stop and ends as cancelled.
    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(at) = self.batches.iter().position(|b| b.id == id) else {
            return false;
        };
        match self.batches[at].status {
            BatchStatus::Waiting | BatchStatus::Paused => {
                self.batches.remove(at);
                true
            }
            BatchStatus::Running => {
                let b = &mut self.batches[at];
                // A cancel wins over a pause that has not landed yet.
                b.pausing = false;
                b.cancel.cancel();
                b.progress.stopping = true;
                true
            }
            _ => false,
        }
    }

    /// Stop everything for one series: its waiting batches go, and its running
    /// one is told to stop.
    pub fn cancel_series(&mut self, series_id: &str) {
        self.batches.retain(|b| {
            b.series_id != series_id
                || !matches!(b.status, BatchStatus::Waiting | BatchStatus::Paused)
        });
        let running = self
            .running()
            .filter(|b| b.series_id == series_id)
            .map(|b| b.id);
        if let Some(id) = running {
            self.cancel(id);
        }
    }

    /// Stop everything: waiting batches go, the running one is told to stop.
    pub fn cancel_all(&mut self) {
        self.batches
            .retain(|b| !matches!(b.status, BatchStatus::Waiting | BatchStatus::Paused));
        if let Some(id) = self.running().map(|b| b.id) {
            self.cancel(id);
        }
    }

    /// Take a finished batch out of the list.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.batches.len();
        self.batches.retain(|b| b.id != id || b.status.is_active());
        self.batches.len() != before
    }

    pub fn clear_finished(&mut self) {
        self.batches.retain(|b| b.status.is_active());
    }

    /// Put a batch aside. A waiting one is paused at once; a running one is
    /// told to stop and is paused when it has (see [`DownloadQueue::park`]).
    pub fn pause(&mut self, id: u64) -> bool {
        let Some(b) = self.get_mut(id) else {
            return false;
        };
        match b.status {
            BatchStatus::Waiting => {
                b.status = BatchStatus::Paused;
                true
            }
            BatchStatus::Running => {
                // Already being cancelled: the cancel stands.
                if b.cancel.is_cancelled() && !b.pausing {
                    return false;
                }
                b.pausing = true;
                b.cancel.cancel();
                b.progress.stopping = true;
                true
            }
            _ => false,
        }
    }

    /// A running batch that was told to pause has stopped: what is saved
    /// stays, and the chapters still to do wait to be resumed.
    pub fn park(&mut self, id: u64, saved: &std::collections::HashSet<String>) {
        let Some(b) = self.get_mut(id) else {
            return;
        };
        let keep: Vec<usize> = (0..b.plan.len())
            .filter(|&i| !saved.contains(&b.plan[i]))
            .collect();
        b.requests = keep.iter().map(|&i| b.requests[i].clone()).collect();
        b.plan = keep.iter().map(|&i| b.plan[i].clone()).collect();
        b.status = BatchStatus::Paused;
        b.pausing = false;
        b.progress = JobProgress::default();
        b.cancel = CancelFlag::new();
    }

    /// Let a paused batch take its turn again.
    pub fn resume(&mut self, id: u64) -> bool {
        match self.get_mut(id) {
            Some(b) if b.status == BatchStatus::Paused => {
                b.status = BatchStatus::Waiting;
                b.cancel = CancelFlag::new();
                true
            }
            _ => false,
        }
    }

    /// Move a waiting batch to the front of the line.
    pub fn prioritize(&mut self, id: u64) {
        let Some(at) = self.batches.iter().position(|b| b.id == id) else {
            return;
        };
        if self.batches[at].status != BatchStatus::Waiting {
            return;
        }
        let batch = self.batches.remove(at);
        let first_waiting = self
            .batches
            .iter()
            .position(|b| b.status == BatchStatus::Waiting)
            .unwrap_or(self.batches.len());
        self.batches.insert(first_waiting, batch);
    }

    /// Queue the chapters of a finished batch that could not be downloaded,
    /// to try again. Returns how many were queued.
    pub fn retry(&mut self, id: u64, now: u64) -> usize {
        let Some(old) = self.get(id) else {
            return 0;
        };
        if old.status.is_active() || old.failed.is_empty() {
            return 0;
        }
        let (mut requests, mut plan) = (Vec::new(), Vec::new());
        for key in &old.failed {
            if let Some(at) = old.plan.iter().position(|k| k == key) {
                requests.push(old.requests[at].clone());
                plan.push(key.clone());
            }
        }
        let batch = Batch::new(
            old.series_id.clone(),
            old.title.clone(),
            requests,
            plan,
            old.choice.clone(),
            old.source_names.clone(),
            now,
        );
        let batch = Batch { quality: old.quality, replace: old.replace, ..batch };
        let queued = self.add(batch).chapters;
        // The retried chapters are no longer failures of the old batch.
        if let Some(old) = self.get_mut(id) {
            old.failed.clear();
        }
        queued
    }

    /// Batches running or waiting, and how many chapters are still to come
    /// (not counting those already finished in the running one).
    pub fn counts(&self) -> (usize, usize, usize) {
        let running = usize::from(self.running().is_some());
        let waiting = self
            .batches
            .iter()
            .filter(|b| b.status == BatchStatus::Waiting)
            .count();
        let chapters = self
            .batches
            .iter()
            .filter(|b| b.status.is_active())
            .map(|b| b.chapter_count().saturating_sub(b.progress.finished))
            .sum();
        (running, waiting, chapters)
    }

    /// Is there anything left to do?
    #[cfg(test)]
    pub fn has_work(&self) -> bool {
        self.batches.iter().any(|b| b.status.is_active())
    }

    /// Does the queue have work for this series?
    pub fn series_has_work(&self, series_id: &str) -> bool {
        self.batches
            .iter()
            .any(|b| b.status.is_active() && b.series_id == series_id)
    }

    /// The series that have work in the queue, in a fixed order, so a screen
    /// that only needs to know which can ignore the queue's progress.
    pub fn series_with_work(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .batches
            .iter()
            .filter(|b| b.status.is_active())
            .map(|b| b.series_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// What the chapter list of one series needs of the queue, with none of the
    /// progress inside it, so the list is not drawn again for every page.
    pub fn view_of(&self, series_id: &str) -> SeriesQueueView {
        let mut states = std::collections::HashMap::new();
        for batch in self.batches.iter().filter(|b| b.series_id == series_id) {
            for key in &batch.plan {
                if !states.contains_key(key) {
                    if let Some(state) = self.chapter_state(series_id, key) {
                        states.insert(key.clone(), state);
                    }
                }
            }
        }
        SeriesQueueView {
            working: self.series_has_work(series_id),
            retry_ids: self
                .batches
                .iter()
                .filter(|b| b.series_id == series_id && !b.status.is_active() && !b.failed.is_empty())
                .map(|b| b.id)
                .collect(),
            states,
        }
    }

    /// Where a chapter of a series stands in the queue, if it is in it.
    /// Work still to do wins over an old failure.
    pub fn chapter_state(&self, series_id: &str, key: &str) -> Option<QueuedChapter> {
        let mut failed = false;
        for batch in self
            .batches
            .iter()
            .rev()
            .filter(|b| b.series_id == series_id)
        {
            let Some(index) = batch.plan.iter().position(|k| k == key) else {
                continue;
            };
            match &batch.status {
                BatchStatus::Waiting | BatchStatus::Paused => {
                    return Some(QueuedChapter::Waiting)
                }
                BatchStatus::Running => {
                    return Some(match batch.progress.active.get(&index) {
                        Some(a) => QueuedChapter::Downloading {
                            pages_done: a.pages_done,
                            pages_total: a.pages_total,
                        },
                        None => QueuedChapter::Waiting,
                    })
                }
                _ => failed |= batch.failed.iter().any(|k| k == key),
            }
        }
        failed.then_some(QueuedChapter::Failed)
    }
}
