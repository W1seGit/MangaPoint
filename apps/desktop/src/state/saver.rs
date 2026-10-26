//! Saving the library and the download queue without holding up the window.
//!
//! Both are saved after nearly every change, and a download changes them once
//! per chapter. Writing them out each time (the library can be a megabyte)
//! froze the window, so a change only marks the file as needing a save; one
//! background task saves what is marked, at most once a second, from a copy of
//! the data. `flush_saves` writes anything still marked right away: it is run
//! when the window closes and before an export starts.

use super::AppState;
use crate::queue::SavedQueue;
use dioxus::prelude::*;
use library::{JsonFile, LibraryDb};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long changes are gathered before they are written.
const SAVE_AFTER_MS: u64 = 1000;
/// How often the background task looks for something to save.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// Decides when a file marked as changed is due to be written. Plain data and
/// times given by the caller, so it is tested without a clock.
#[derive(Debug, Default)]
pub struct Debouncer {
    first_marked: Option<u64>,
}

impl Debouncer {
    /// Something changed at `now_ms`. Changes while one is already waiting
    /// join it: the wait is counted from the first.
    pub fn mark(&mut self, now_ms: u64) {
        self.first_marked.get_or_insert(now_ms);
    }

    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.first_marked.is_some()
    }

    /// Is a save due? If so the mark is cleared: the caller saves now.
    pub fn take_if_due(&mut self, now_ms: u64, wait_ms: u64) -> bool {
        match self.first_marked {
            Some(first) if now_ms.saturating_sub(first) >= wait_ms => {
                self.first_marked = None;
                true
            }
            _ => false,
        }
    }

    /// Take the mark whatever the time (for the save at exit).
    pub fn take(&mut self) -> bool {
        self.first_marked.take().is_some()
    }
}

/// How often reading progress, which changes every moment, may ask for the
/// library to be saved. The position is always kept in memory; it reaches the
/// disk at most this often, and at once when the reader is left or the window
/// closes.
const PROGRESS_EVERY_MS: u64 = 2000;

/// Decides when a change of reading position may mark the library as needing a
/// save: the first one at once, the ones that follow within [`PROGRESS_EVERY_MS`]
/// are held and marked by the background task when the time is up, or by
/// [`AppState::flush_saves`]. Plain data and times given by the caller.
#[derive(Debug, Default)]
pub struct SaveGate {
    last: Option<u64>,
    pending: bool,
}

impl SaveGate {
    /// The position changed at `now_ms`. True if the library is to be marked now.
    pub fn changed(&mut self, now_ms: u64, every_ms: u64) -> bool {
        match self.last {
            Some(last) if now_ms.saturating_sub(last) < every_ms => {
                self.pending = true;
                false
            }
            _ => {
                self.last = Some(now_ms);
                self.pending = false;
                true
            }
        }
    }

    /// Called now and then: true if a held change is due to be marked.
    pub fn tick(&mut self, now_ms: u64, every_ms: u64) -> bool {
        let due = self.pending && self.last.is_none_or(|l| now_ms.saturating_sub(l) >= every_ms);
        if due {
            self.pending = false;
            self.last = Some(now_ms);
        }
        due
    }

    /// Take a held change whatever the time (leaving the reader, closing the window).
    pub fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

static PROGRESS: Mutex<SaveGate> = Mutex::new(SaveGate { last: None, pending: false });

/// One file kept by the saver: when it is due, and which copy of it is the
/// newest to have been written.
struct Slot {
    debounce: Mutex<Debouncer>,
    /// Numbers the copies taken to write, oldest first.
    taken: AtomicU64,
    /// The newest copy written; a slower, older copy must not overwrite it.
    written: Mutex<u64>,
}

impl Slot {
    const fn new() -> Self {
        Self {
            debounce: Mutex::new(Debouncer { first_marked: None }),
            taken: AtomicU64::new(0),
            written: Mutex::new(0),
        }
    }

    fn mark(&self) {
        if let Ok(mut d) = self.debounce.lock() {
            d.mark(now_ms());
        }
    }

    /// The number of the copy to write, if a save is due (or `force`d and
    /// anything is waiting).
    fn claim(&self, force: bool) -> Option<u64> {
        let mut d = self.debounce.lock().ok()?;
        let go = if force {
            d.take()
        } else {
            d.take_if_due(now_ms(), SAVE_AFTER_MS)
        };
        go.then(|| self.taken.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Write copy `number` unless a newer one has been written already. The
    /// error, if it could not be written, is for the person to be told.
    fn write(&self, number: u64, save: impl FnOnce() -> Result<(), library::JsonError>) -> Option<String> {
        let mut written = self.written.lock().ok()?;
        if number <= *written {
            return None;
        }
        match save() {
            Ok(()) => {
                *written = number;
                None
            }
            // Marked as changed again, so the saver (and the save at exit) tries once more.
            Err(e) => {
                self.mark();
                Some(e.to_string())
            }
        }
    }

    /// Wait until a write that is under way has finished.
    fn wait_idle(&self) {
        drop(self.written.lock());
    }
}

static LIBRARY: Slot = Slot::new();
static QUEUE: Slot = Slot::new();

fn now_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn write_library(number: u64, copy: LibraryDb) -> Option<String> {
    LIBRARY.write(number, || JsonFile::new(super::paths().library_file()).save(&copy))
}

fn write_queue(number: u64, copy: SavedQueue) -> Option<String> {
    QUEUE.write(number, || JsonFile::new(super::paths().queue_file()).save(&copy))
}

/// Tell the person a file could not be saved, once for each different reason
/// (the saver would otherwise say it again every second).
fn report(state: AppState, what: &str, failure: Option<String>) {
    static LAST: Mutex<Option<String>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    match failure {
        Some(reason) => {
            let message = format!("Could not save {what}: {reason}");
            if last.as_deref() != Some(message.as_str()) {
                tracing::warn!("{message}");
                state.error(message.clone());
                *last = Some(message);
            }
        }
        None => *last = None,
    }
}

impl AppState {
    /// The library changed: it is saved soon, with any other changes made in
    /// the meantime.
    pub fn save_library(&self) {
        LIBRARY.mark();
    }

    /// The reading position changed (it is already in the library in memory):
    /// written to disk at most every couple of seconds, and when the reader is
    /// left (see [`AppState::flush_reading`]) or the window closes.
    pub fn save_progress(&self) {
        let go = PROGRESS.lock().is_ok_and(|mut g| g.changed(now_ms(), PROGRESS_EVERY_MS));
        if go {
            self.apply_pending_reading();
            LIBRARY.mark();
        }
    }

    /// Mark any held reading position for saving now.
    pub fn flush_reading(&self) {
        let held = PROGRESS.lock().is_ok_and(|mut g| g.take_pending());
        if self.apply_pending_reading() || held {
            LIBRARY.mark();
        }
    }

    /// The queue changed: saved soon, like the library.
    pub(super) fn mark_queue_changed(&self) {
        QUEUE.mark();
    }

    /// Start the task that writes what has changed. Once, when the app opens.
    pub fn start_saver(&self) {
        let state = *self;
        super::spawn_forever(async move {
            loop {
                tokio::time::sleep(LOOK_EVERY).await;
                if PROGRESS.lock().is_ok_and(|mut g| g.tick(now_ms(), PROGRESS_EVERY_MS)) {
                    state.apply_pending_reading();
                    LIBRARY.mark();
                }
                if let Some(number) = LIBRARY.claim(false) {
                    let copy = state.library.peek().clone();
                    if let Ok(failure) = tokio::task::spawn_blocking(move || write_library(number, copy)).await {
                        report(state, "your library", failure);
                    }
                }
                if let Some(number) = QUEUE.claim(false) {
                    let copy = state.queue.peek().to_saved();
                    if let Ok(failure) = tokio::task::spawn_blocking(move || write_queue(number, copy)).await {
                        report(state, "the download queue", failure);
                    }
                }
            }
        });
    }

    /// Write anything marked as changed, now. For when the window closes and
    /// before an export starts.
    pub fn flush_saves(&self) {
        self.flush_reading();
        if let Some(number) = LIBRARY.claim(true) {
            report(*self, "your library", write_library(number, self.library.peek().clone()));
        }
        if let Some(number) = QUEUE.claim(true) {
            report(*self, "the download queue", write_queue(number, self.queue.peek().to_saved()));
        }
        // A write the background task began a moment ago must be finished, too.
        LIBRARY.wait_idle();
        QUEUE.wait_idle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_gathered_and_written_once_after_the_wait() {
        let mut d = Debouncer::default();
        assert!(!d.is_dirty());
        assert!(!d.take_if_due(5_000, 1000), "nothing changed, nothing to save");
        d.mark(10_000);
        d.mark(10_300);
        d.mark(10_900);
        assert!(!d.take_if_due(10_999, 1000), "still gathering");
        assert!(d.take_if_due(11_000, 1000), "due a second after the first change");
        assert!(!d.take_if_due(20_000, 1000), "and written only once");
        assert!(!d.is_dirty());
    }

    #[test]
    fn a_change_during_a_save_is_saved_again_and_a_steady_stream_cannot_starve_the_save() {
        let mut d = Debouncer::default();
        d.mark(0);
        assert!(d.take_if_due(1000, 1000));
        d.mark(1200);
        assert!(!d.take_if_due(2100, 1000));
        assert!(d.take_if_due(2200, 1000));
        // A change every 300 ms is still written every second or two, never starved.
        let mut saves = 0;
        for t in (0..10_000).step_by(300) {
            d.mark(t + 100_000);
            if d.take_if_due(t + 100_000, 1000) {
                saves += 1;
            }
        }
        assert!((5..=10).contains(&saves), "{saves} saves for 10 s of changes");
    }

    #[test]
    fn reading_progress_is_marked_at_most_every_two_seconds_and_never_lost() {
        let mut g = SaveGate::default();
        assert!(g.changed(10_000, 2000), "the first change is marked at once");
        // A scroll report every 250 ms for five seconds.
        let mut marks = 0;
        for t in (10_250..15_000).step_by(250) {
            if g.changed(t, 2000) || g.tick(t, 2000) {
                marks += 1;
            }
        }
        assert!((2..=3).contains(&marks), "{marks} marks in 5 s");
        // Held changes are not lost: the task's tick marks them when due ...
        let mut h = SaveGate::default();
        assert!(h.changed(0, 2000));
        assert!(!h.changed(500, 2000));
        assert!(!h.tick(1_999, 2000));
        assert!(h.tick(2_000, 2000));
        assert!(!h.tick(9_000, 2000), "and only once");
        // ... and leaving the reader takes them at once.
        assert!(!h.changed(2_100, 2000));
        assert!(h.take_pending());
        assert!(!h.take_pending());
        assert!(!SaveGate::default().tick(5, 2000), "nothing changed, nothing to mark");
    }

    #[test]
    fn exit_takes_whatever_is_waiting_at_once() {
        let mut d = Debouncer::default();
        assert!(!d.take());
        d.mark(5);
        assert!(d.take());
        assert!(!d.take());
    }

    #[test]
    fn an_older_copy_never_overwrites_a_newer_one() {
        let slot = Slot::new();
        slot.mark();
        let first = slot.claim(true).unwrap();
        slot.mark();
        let second = slot.claim(true).unwrap();
        assert!(second > first);
        let log = std::cell::RefCell::new(Vec::new());
        slot.write(second, || {
            log.borrow_mut().push("new");
            Ok(())
        });
        slot.write(first, || {
            log.borrow_mut().push("old");
            Ok(())
        });
        assert_eq!(*log.borrow(), ["new"], "the slow older write is dropped");
        assert!(slot.claim(true).is_none(), "nothing left waiting");
    }

    #[test]
    fn a_save_that_fails_is_reported_and_tried_again_with_the_next_copy() {
        let slot = Slot::new();
        slot.mark();
        let first = slot.claim(true).unwrap();
        let failure = slot.write(first, || {
            Err(library::JsonError::Unreadable(std::path::PathBuf::from("library.json")))
        });
        assert!(failure.is_some_and(|m| m.contains("library.json")), "the person is told why");
        // A failed copy is not counted as written, so a copy that follows still is.
        slot.mark();
        let second = slot.claim(true).unwrap();
        assert_eq!(slot.write(second, || Ok(())), None);
    }

    #[test]
    fn a_failed_write_is_marked_for_another_try() {
        let slot = Slot::new();
        slot.mark();
        let first = slot.claim(true).unwrap();
        assert!(slot
            .write(first, || Err(library::JsonError::Unreadable(std::path::PathBuf::from("x.json"))))
            .is_some());
        assert!(slot.claim(true).is_some(), "the save at exit still has it to do");
    }
}
