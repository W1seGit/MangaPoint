//! The decisions an import makes that do not depend on the UI: which series a
//! book goes into, whether a file was imported before, and the one record of
//! an import that is running (so deleting a series and closing the window can
//! see it).

use library::LibraryDb;
use pipeline::CancelFlag;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The series a book is written into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The id the series is kept under. A series that was renamed keeps the id
    /// it had, so this is never made from the title of one that exists.
    pub id: String,
    pub title: String,
    /// The import makes the series (it is not in the library yet).
    pub new_series: bool,
}

/// What a book is called as a series when the person has not named it.
pub fn wanted_title(title: &str, label: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        pipeline::import::series_title(label)
    } else {
        title.to_string()
    }
}

/// Which series a book goes into: the one being added to, an imported one that
/// already has this title, or a new one (with a free title if another kind of
/// series has the name). `Err` says why not: the series being added to has been
/// deleted, and nothing may bring it back.
pub fn resolve_series(
    library: &LibraryDb,
    target: Option<&str>,
    wanted: &str,
) -> Result<Resolved, &'static str> {
    if let Some(id) = target {
        return library
            .find(id)
            .map(|s| Resolved {
                id: s.id.clone(),
                title: s.title.clone(),
                new_series: false,
            })
            .ok_or("The series these pages were being added to is no longer in your library.");
    }
    Ok(match library.find(&library::series_id(wanted)) {
        Some(s) if s.is_imported() => Resolved {
            id: s.id.clone(),
            title: s.title.clone(),
            new_series: false,
        },
        Some(_) => {
            let title = library.free_title(wanted);
            Resolved {
                id: library::series_id(&title),
                title,
                new_series: true,
            }
        }
        None => Resolved {
            id: library::series_id(wanted),
            title: wanted.to_string(),
            new_series: true,
        },
    })
}

/// Has every file of this book (by fingerprint) been imported into the series
/// before? `known` is each imported series with the fingerprints it holds. A
/// book with no fingerprints (loose pictures) is never "already imported".
pub fn already_imported(known: &[(String, Vec<String>)], series_id: &str, prints: &[String]) -> bool {
    !prints.is_empty()
        && known
            .iter()
            .any(|(id, have)| id == series_id && prints.iter().all(|p| have.contains(p)))
}

/// Numbers the reads and runs of the import dialog (and its closing) in order,
/// so that one that was left behind can tell it is no longer wanted.
#[derive(Default)]
pub struct RunCounter(AtomicU64);

impl RunCounter {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Start something new: its number, which makes every older one stale.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Is `mine` still the latest number given out?
    pub fn is_current(&self, mine: u64) -> bool {
        self.0.load(Ordering::SeqCst) == mine
    }
}

struct Active {
    run: u64,
    series: Vec<String>,
    cancel: CancelFlag,
}

/// The import that is running, if one is: which run it is, the series it
/// writes into, and how to stop it.
#[derive(Default)]
pub struct ImportRegistry {
    slot: Mutex<Option<Active>>,
}

impl ImportRegistry {
    pub const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    /// Note that run number `run` has started. False (and nothing noted) if
    /// an import is already running: two never write at once.
    pub fn begin(&self, run: u64, series: Vec<String>, cancel: CancelFlag) -> bool {
        let Ok(mut slot) = self.slot.lock() else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(Active { run, series, cancel });
        true
    }

    /// Note that run number `run` has ended. Another run's record is left alone.
    pub fn end(&self, run: u64) {
        if let Ok(mut slot) = self.slot.lock() {
            if slot.as_ref().is_some_and(|a| a.run == run) {
                *slot = None;
            }
        }
    }

    pub fn running(&self) -> bool {
        self.slot.lock().is_ok_and(|s| s.is_some())
    }

    /// The series the running import writes into (none when none is running).
    pub fn series(&self) -> Vec<String> {
        self.slot
            .lock()
            .ok()
            .and_then(|s| s.as_ref().map(|a| a.series.clone()))
            .unwrap_or_default()
    }

    /// Stop the running import at its next page.
    pub fn cancel(&self) {
        if let Ok(slot) = self.slot.lock() {
            if let Some(active) = slot.as_ref() {
                active.cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::{ConversionSettings, DownloadedChapter};

    fn chapter(number: f64, id: i64) -> DownloadedChapter {
        DownloadedChapter {
            key: library::chapter_key(Some(number), id),
            number: Some(number),
            name: "c".into(),
            source_id: library::IMPORT_SOURCE.into(),
            chapter_id: id,
            pages: 1,
            bytes: 1,
            downloaded_at: 1,
            quality: None,
        }
    }

    #[test]
    fn a_renamed_imported_series_keeps_receiving_pages_under_its_own_id() {
        let mut db = LibraryDb::default();
        let defaults = ConversionSettings::default();
        let id = db.add_imported("Old Name", &defaults, None, vec![], vec![chapter(1.0, -1)], 1);
        db.set_title(&id, "New Name");
        // Adding to it, and importing a book that still has the old title.
        for (target, wanted) in [(Some(id.as_str()), "New Name"), (None, "Old Name")] {
            let got = resolve_series(&db, target, wanted).unwrap();
            assert_eq!(got.id, id, "the id the series has, not one made from a title");
            assert_eq!(got.title, "New Name");
            assert!(!got.new_series);
        }
        // The pages go in under that id: still one series, with both chapters.
        db.add_imported_to(&id, "New Name", &defaults, None, vec![], vec![chapter(2.0, -2)], 2, false);
        assert_eq!(db.series.len(), 1);
        assert_eq!(db.find(&id).unwrap().chapters.len(), 2);
    }

    #[test]
    fn a_new_title_makes_a_new_series_and_a_taken_title_gets_a_free_one() {
        let mut db = LibraryDb::default();
        let defaults = ConversionSettings::default();
        let fresh = resolve_series(&db, None, "Fresh").unwrap();
        assert!(fresh.new_series && fresh.title == "Fresh" && fresh.id == library::series_id("Fresh"));
        // A downloaded series with the name is not an imported one: not joined.
        db.record_chapter("Taken", &defaults, chapter(1.0, 5), 1);
        let taken = resolve_series(&db, None, "Taken").unwrap();
        assert!(taken.new_series);
        assert_eq!(taken.title, "Taken (2)");
        assert_eq!(taken.id, library::series_id("Taken (2)"));
    }

    #[test]
    fn pages_for_a_series_that_was_deleted_are_not_given_a_series_of_their_own() {
        let db = LibraryDb::default();
        let err = resolve_series(&db, Some("gone-00000000"), "Whatever").unwrap_err();
        assert!(err.contains("no longer in your library"), "{err}");
    }

    #[test]
    fn a_book_is_already_imported_when_every_one_of_its_files_was() {
        let known = vec![("s".to_string(), vec!["a|1|2".to_string(), "b|3|4".to_string()])];
        let prints = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(already_imported(&known, "s", &prints(&["a|1|2"])));
        assert!(already_imported(&known, "s", &prints(&["a|1|2", "b|3|4"])));
        assert!(!already_imported(&known, "s", &prints(&["a|1|2", "c|5|6"])));
        assert!(!already_imported(&known, "other", &prints(&["a|1|2"])));
        assert!(!already_imported(&known, "s", &[]), "loose pictures have no fingerprint");
    }

    #[test]
    fn the_title_wanted_is_the_one_typed_else_the_one_the_file_name_gives() {
        assert_eq!(wanted_title("  My Title ", "x.cbz"), "My Title");
        assert_eq!(wanted_title("", "One Piece - c1050.cbz"), "One Piece");
    }

    #[test]
    fn only_one_import_runs_and_only_its_own_end_lets_go() {
        let reg = ImportRegistry::new();
        assert!(!reg.running() && reg.series().is_empty());
        let first = CancelFlag::new();
        assert!(reg.begin(1, vec!["a".into(), "b".into()], first.clone()));
        assert!(reg.running() && reg.series() == ["a", "b"]);
        // A second one is refused, and does not disturb the first.
        let second = CancelFlag::new();
        assert!(!reg.begin(2, vec!["c".into()], second.clone()));
        assert_eq!(reg.series(), ["a", "b"]);
        // An older run ending does not clear a newer record.
        reg.end(2);
        assert!(reg.running());
        reg.cancel();
        assert!(first.is_cancelled() && !second.is_cancelled());
        reg.end(1);
        assert!(!reg.running() && reg.series().is_empty());
        assert!(reg.begin(3, vec![], CancelFlag::new()), "free again");
    }

    #[test]
    fn an_older_read_or_run_is_stale_once_a_newer_one_starts_or_the_dialog_closes() {
        let runs = RunCounter::new();
        let read = runs.next();
        assert!(runs.is_current(read));
        let run = runs.next();
        assert!(!runs.is_current(read), "a newer one makes the older stale");
        assert!(runs.is_current(run));
        let _closed = runs.next();
        assert!(!runs.is_current(run), "closing the dialog makes what was running stale");
    }
}
