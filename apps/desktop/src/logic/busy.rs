//! Whether a series is being worked on, so deleting it waits for the work to
//! stop, and what a download may still record about a series.

use crate::queue::DownloadQueue;

/// What is running for one series right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    /// A batch of the queue is waiting or downloading for it.
    pub download: bool,
    /// An export of it is running (a paused one is not: nothing is writing).
    pub export: bool,
    /// An import is writing into it (or is about to).
    pub import: bool,
}

impl Busy {
    pub fn any(self) -> bool {
        self.download || self.export || self.import
    }
}

/// `running_export` is the series of the export that is running, if one is;
/// `importing` the series a running import writes into (none when it is not
/// running).
pub fn series_busy(
    queue: &DownloadQueue,
    running_export: Option<&str>,
    importing: &[String],
    series_id: &str,
) -> Busy {
    Busy {
        download: queue.series_has_work(series_id),
        export: running_export == Some(series_id),
        import: importing.iter().any(|s| s == series_id),
    }
}

/// May a chapter that was just saved be recorded in the library? Not when the
/// series was in the library (or this download had already recorded a chapter
/// of it) and has since been removed: recording would bring it back.
pub fn may_record_chapter(known_before: bool, exists_now: bool) -> bool {
    exists_now || !known_before
}

/// What to tell someone closing the window while work is running; `None` when
/// nothing is, and the window can just close.
pub fn quit_warning(
    download_running: bool,
    export_running: bool,
    import_running: bool,
) -> Option<&'static str> {
    match (download_running, export_running, import_running) {
        (false, false, false) => None,
        (true, false, false) => Some("A download is running. Chapters that have finished are kept, but the one in progress will have to start again."),
        (false, true, false) => Some("An export is running. Closing now can leave a partly written file on the reader or in the folder."),
        (false, false, true) => Some("An import is running. Closing now stops it: the books being imported are not added, and you will have to import them again."),
        (true, true, false) => Some("A download and an export are running. Closing now can leave a partly written file on the reader or in the folder, and the chapter being downloaded will have to start again."),
        (true, false, true) => Some("A download and an import are running. Closing now stops both: the chapter being downloaded will have to start again, and the books being imported are not added."),
        (false, true, true) => Some("An export and an import are running. Closing now can leave a partly written file on the reader or in the folder, and the books being imported are not added."),
        (true, true, true) => Some("A download, an export and an import are running. Closing now can leave a partly written file on the reader or in the folder, the chapter being downloaded will have to start again, and the books being imported are not added."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::Batch;
    use pipeline::SourceChoice;
    use std::collections::HashMap;

    fn queue_with(series: &str) -> DownloadQueue {
        let mut q = DownloadQueue::default();
        q.add(Batch::new(
            series.into(),
            "T".into(),
            vec![pipeline::ChapterRequest {
                number: Some(1.0),
                name: "c".into(),
                key: Some("c1".into()),
                options: vec![],
            }],
            vec!["c1".into()],
            SourceChoice::Adaptive,
            HashMap::new(),
            0,
        ));
        q
    }

    #[test]
    fn a_series_is_busy_while_it_downloads_or_exports() {
        let q = queue_with("a");
        assert!(series_busy(&q, None, &[], "a").download);
        assert!(!series_busy(&q, None, &[], "b").any());
        let idle = DownloadQueue::default();
        let b = series_busy(&idle, Some("b"), &[], "b");
        assert!(b.export && !b.download && b.any());
        assert!(!series_busy(&idle, Some("b"), &[], "a").any());
        // An import is busy with every series it is about to write into.
        let ids = ["x".to_string(), "y".to_string()];
        let i = series_busy(&idle, None, &ids, "y");
        assert!(i.import && !i.download && !i.export && i.any());
        assert!(!series_busy(&idle, None, &ids, "z").any());
        assert!(!series_busy(&idle, None, &[], "x").any());
    }

    #[test]
    fn a_removed_series_is_not_brought_back_by_a_late_chapter() {
        assert!(may_record_chapter(false, false), "a first download makes the series");
        assert!(may_record_chapter(true, true));
        assert!(may_record_chapter(false, true));
        assert!(!may_record_chapter(true, false), "removed meanwhile");
    }

    #[test]
    fn closing_while_work_runs_warns_and_closing_when_idle_does_not() {
        assert_eq!(quit_warning(false, false, false), None);
        assert!(quit_warning(true, false, false).unwrap().contains("download"));
        assert!(quit_warning(false, true, false).unwrap().contains("export"));
        let both = quit_warning(true, true, false).unwrap();
        assert!(both.contains("download") && both.contains("export"));
    }

    #[test]
    fn closing_while_an_import_runs_asks_first_alone_or_with_other_work() {
        assert!(quit_warning(false, false, true).unwrap().contains("import"));
        assert!(quit_warning(true, false, true).unwrap().contains("import"));
        assert!(quit_warning(false, true, true).unwrap().contains("import"));
        let all = quit_warning(true, true, true).unwrap();
        assert!(all.contains("download") && all.contains("export") && all.contains("import"));
    }
}
