//! Deleting chapters, series and exported files.

use super::*;

/// How long deleting waits for a download or an export to let go of a series.
const STOP_WAIT: Duration = Duration::from_secs(60);

/// The series of the export that is running (not a paused one), if any.
pub(super) fn running_export_series(state: AppState) -> Option<String> {
    state
        .job
        .peek()
        .as_ref()
        .filter(|v| v.kind == JobKind::Export && v.result.is_none() && !v.paused)
        .map(|v| v.series_id.clone())
}

fn any_busy(state: AppState, series_ids: &[String]) -> bool {
    let running = running_export_series(state);
    let importing = importing_series();
    let queue = state.queue.peek();
    series_ids
        .iter()
        .any(|id| logic::series_busy(&queue, running.as_deref(), &importing, id).any())
}

/// Stop the downloads, the export and the import working on these series and
/// wait until they have really stopped, so nothing writes to a folder that is
/// being deleted or records a chapter of a series that is gone. Says so and
/// returns false if they do not stop in time; then nothing should be deleted.
pub(super) async fn stop_work_for(state: AppState, series_ids: &[String]) -> bool {
    for id in series_ids {
        state.edit_queue(|q| q.cancel_series(id));
        remove_waiting_export(state, id);
    }
    // An import writing into any of them is stopped (what it had started is taken
    // away again); it ends before anything is deleted.
    if importing_series().iter().any(|id| series_ids.contains(id)) {
        cancel_import();
    }
    if let Some(running) = running_export_series(state) {
        if series_ids.contains(&running) {
            let mut job = state.job;
            let mut guard = job.write();
            if let Some(view) = guard.as_mut() {
                view.paused = false;
                view.cancel.cancel();
                view.progress.stopping = true;
            }
        }
    }
    // A paused export is not running, but resuming it would fail on pages that
    // are about to be deleted: it is ended instead.
    {
        let mut job = state.job;
        let mut guard = job.write();
        if let Some(view) = guard.as_mut() {
            if view.kind == JobKind::Export
                && view.paused
                && view.result.is_none()
                && series_ids.contains(&view.series_id)
            {
                view.paused = false;
                view.result = Some(Err("Cancelled: the series was deleted.".to_string()));
                super::export::forget_export_plan();
            }
        }
    }
    let started = Instant::now();
    while any_busy(state, series_ids) {
        if started.elapsed() > STOP_WAIT {
            state.error(
                "A download, export or import of this series would not stop, so nothing was deleted. Cancel it and try again."
                    .to_string(),
            );
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    true
}

/// Delete every downloaded page of a series. Exported files are kept.
pub fn remove_from_library(state: AppState, series_ids: Vec<String>) {
    spawn_forever(async move {
        if !stop_work_for(state, &series_ids).await {
            return;
        }
        let mut removed = 0;
        for series_id in series_ids {
            let files = library_files();
            let id = series_id.clone();
            match tokio::task::spawn_blocking(move || files.remove_series(&id)).await {
                Ok(Ok(())) => {
                    let mut library = state.library;
                    let title = library.write().remove_series(&series_id).map(|s| s.title);
                    state.save_library();
                    removed += 1;
                    info!(
                        "Removed {} from the library",
                        title.unwrap_or(series_id.clone())
                    );
                    // If it was open, close it.
                    let open_here = state
                        .workspace
                        .peek()
                        .as_ref()
                        .is_some_and(|w| w.series_id == series_id);
                    if open_here {
                        let mut ws = state.workspace;
                        ws.set(None);
                        if *state.page.peek() == Page::Series {
                            state.go(Page::Library);
                        }
                    }
                }
                Ok(Err(e)) => state.error(format!("Could not delete the downloaded pages: {e}")),
                Err(e) => state.error(format!("Could not delete the downloaded pages: {e}")),
            }
        }
        if removed > 0 {
            state.notify(format!(
                "Removed {} from your library.",
                logic::plural(removed, "series")
            ));
        }
    });
}

/// Delete the downloaded pages of these chapters. Returns how many went.
pub(super) async fn delete_chapter_files(
    state: AppState,
    series_id: &str,
    keys: Vec<String>,
) -> usize {
    let mut deleted = 0;
    for key in keys {
        let files = library_files();
        let (id, k) = (series_id.to_string(), key.clone());
        match tokio::task::spawn_blocking(move || files.remove_chapter(&id, &k)).await {
            Ok(Ok(())) => {
                let mut library = state.library;
                library.write().remove_chapter(series_id, &key);
                deleted += 1;
            }
            Ok(Err(e)) => state.error(format!("Could not delete that chapter: {e}")),
            Err(e) => state.error(format!("Could not delete that chapter: {e}")),
        }
    }
    // With every chapter gone there is nothing left in the series' folder worth
    // keeping (the cover is the only thing): the folder goes too. The series
    // stays in the library, and its cover comes back when it is opened again.
    let emptied = state
        .library
        .peek()
        .find(series_id)
        .is_some_and(|s| s.chapters.is_empty());
    if emptied && deleted > 0 {
        let files = library_files();
        let id = series_id.to_string();
        if let Ok(Ok(())) = tokio::task::spawn_blocking(move || files.remove_series(&id)).await {
            let mut library = state.library;
            if let Some(info) = library
                .write()
                .series
                .iter_mut()
                .find(|s| s.id == series_id)
                .and_then(|s| s.info.as_mut())
            {
                info.has_cover = false;
            }
            let mut covers = state.covers;
            covers.write().remove(&saved_cover_key(series_id));
        }
    }
    state.save_library();
    deleted
}

/// Delete some downloaded chapters of a series; the series stays in the library.
pub fn delete_chapters(state: AppState, series_id: String, keys: Vec<String>) {
    spawn_forever(async move {
        if !stop_work_for(state, std::slice::from_ref(&series_id)).await {
            return;
        }
        let deleted = delete_chapter_files(state, &series_id, keys).await;
        if deleted == 0 {
            return;
        }
        state.notify(format!("Deleted {}.", logic::plural(deleted, "chapter")));
    });
}

/// Delete every downloaded chapter of these series; they stay in the library.
pub fn delete_downloads(state: AppState, series_ids: Vec<String>) {
    spawn_forever(async move {
        if !stop_work_for(state, &series_ids).await {
            return;
        }
        let mut total = 0;
        for id in series_ids {
            let keys: Vec<String> = state
                .library
                .peek()
                .find(&id)
                .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
                .unwrap_or_default();
            total += delete_chapter_files(state, &id, keys).await;
        }
        state.notify(format!(
            "Deleted {}. The series stay in your library.",
            logic::plural(total, "downloaded chapter")
        ));
    });
}

/// Delete an exported file from disk (and forget it).
pub fn delete_exported_file(state: AppState, record_id: String, path: String) {
    let result = library::delete_exported_file(std::path::Path::new(&path));
    match result {
        Ok(_) => {
            let mut exports = state.exports;
            exports.write().forget_file(&record_id, &path);
            state.save_exports();
            info!("Deleted {path}");
        }
        Err(e) => state.error(format!("Could not delete the file: {e}")),
    }
}

/// Delete every file of one export from disk (the ones already gone are
/// simply forgotten) and take it off the list.
pub fn delete_export(state: AppState, record_id: String) {
    let files: Vec<String> = state
        .exports
        .peek()
        .records
        .iter()
        .find(|r| r.id == record_id)
        .map(|r| r.files.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default();
    let mut failed = 0;
    for path in files {
        let there = !path.starts_with("http") && std::path::Path::new(&path).exists();
        if there {
            if let Err(e) = library::delete_exported_file(std::path::Path::new(&path)) {
                warn!("Could not delete {path}: {e}");
                failed += 1;
                continue;
            }
            info!("Deleted {path}");
        }
        let mut exports = state.exports;
        exports.write().forget_file(&record_id, &path);
    }
    state.save_exports();
    if failed > 0 {
        state.error(format!("{failed} file(s) could not be deleted."));
    }
}

/// Forget an exported file that is no longer on disk.
pub fn forget_exported_file(state: AppState, record_id: String, path: String) {
    let mut exports = state.exports;
    exports.write().forget_file(&record_id, &path);
    state.save_exports();
}
