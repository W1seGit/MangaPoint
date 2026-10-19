//! What is kept on disk of an export that has not finished.

use crate::ui::work::*;
use library::JsonFile;
use serde::{Deserialize, Serialize};

/// What is kept of an export that has not finished, so it can go on after the
/// app was closed. The chapters already converted are kept too, so going on
/// does not do them again.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedExport {
    pub(super) items: Vec<SavedExportItem>,
    pub(super) grouping: SavedGrouping,
    pub(super) target: Option<ExportTarget>,
    /// The drive the reader was on when the export began, to notice a
    /// different one under the same letter when it is resumed.
    pub(super) drive: Option<logic::DriveIdentity>,
    /// The first series that is not finished.
    pub(super) next: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SavedExportItem {
    pub(super) series_id: String,
    pub(super) keys: Vec<String>,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(super) enum SavedGrouping {
    #[default]
    Single,
    PerChapter,
    EveryN(usize),
}

impl From<Grouping> for SavedGrouping {
    fn from(g: Grouping) -> Self {
        match g {
            Grouping::Single => Self::Single,
            Grouping::PerChapter => Self::PerChapter,
            Grouping::EveryN(n) => Self::EveryN(n),
        }
    }
}

impl From<SavedGrouping> for Grouping {
    fn from(g: SavedGrouping) -> Self {
        match g {
            SavedGrouping::Single => Self::Single,
            SavedGrouping::PerChapter => Self::PerChapter,
            SavedGrouping::EveryN(n) => Self::EveryN(n.max(1)),
        }
    }
}

pub(super) fn save_export_plan(saved: &SavedExport) {
    let _ = JsonFile::new(paths().export_job_file()).save(saved);
}

pub(in crate::ui::work) fn forget_export_plan() {
    let _ = std::fs::remove_file(paths().export_job_file());
}

/// Write down what is left to do: the series being exported (or paused) and
/// those waiting for it. The plan on disk always says where to go on from.
pub(super) fn persist_export_plan(state: AppState, base: &SavedExport, current: &SavedExportItem) {
    let mut items = vec![current.clone()];
    items.extend(state.export_waiting.peek().iter().map(|w| SavedExportItem {
        series_id: w.series_id.clone(),
        keys: w.keys.clone(),
    }));
    save_export_plan(&SavedExport {
        items,
        next: 0,
        ..base.clone()
    });
}

#[cfg(test)]
mod saved_export_tests {
    use super::*;

    #[test]
    fn an_unfinished_export_is_kept_with_where_to_go_on() {
        let saved = SavedExport {
            items: vec![
                SavedExportItem { series_id: "a".into(), keys: vec!["c1".into(), "c2".into()] },
                SavedExportItem { series_id: "b".into(), keys: vec!["c1".into()] },
            ],
            grouping: SavedGrouping::from(Grouping::EveryN(25)),
            target: Some(ExportTarget::Folder { path: "out".into() }),
            drive: None,
            next: 1,
        };
        let back: SavedExport = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.next, 1);
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.items[0].keys, ["c1", "c2"]);
        assert_eq!(back.target, saved.target);
        assert_eq!(Grouping::from(back.grouping), Grouping::EveryN(25));
        // Nothing saved is nothing to resume.
        assert!(serde_json::from_str::<SavedExport>("{}").unwrap().items.is_empty());
    }
}
