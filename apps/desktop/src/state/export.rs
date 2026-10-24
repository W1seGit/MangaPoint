//! The Export screen: the series being exported together, and the choices that cover them all.

use crate::logic::TargetChoice;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupingChoice {
    Single,
    PerChapter,
    EveryN,
}

/// Choices on the Export screen that cover every series being exported.
#[derive(Clone)]
pub struct ExportOpts {
    pub target: TargetChoice,
    pub grouping: GroupingChoice,
    /// The "N" of "every N chapters", as typed.
    pub every_n: String,
    /// What a phone or tablet gets: a CBZ or a PDF of the pages as they are.
    pub original_format: pipeline::OriginalFormat,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            target: TargetChoice::Folder,
            grouping: GroupingChoice::Single,
            every_n: "10".into(),
            original_format: pipeline::OriginalFormat::Cbz,
        }
    }
}

/// One series in the Export screen's list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportItem {
    pub series_id: String,
    /// Downloaded chapters left out of the export.
    pub excluded: HashSet<String>,
    /// Switched off, a series stays in the list but is not exported.
    pub off: bool,
    /// The chapter being previewed, kept for when the screen is opened again.
    pub chapter: Option<String>,
}

/// The series being edited and exported together, and the one on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportPlan {
    pub items: Vec<ExportItem>,
    pub active: usize,
}
