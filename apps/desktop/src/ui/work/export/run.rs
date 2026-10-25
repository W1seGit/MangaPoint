//! Running a saved export, one series after another, and pausing, resuming and restoring it.

use crate::ui::work::*;
use super::*;
use crate::state::{Modal, ModalAction};
use library::JsonFile;

/// A series of the saved plan, as it is listed while it waits its turn.
fn waiting_export(state: AppState, item: &SavedExportItem) -> Option<WaitingExport> {
    let library = state.library.peek();
    let series = library.find(&item.series_id)?;
    let keys: Vec<String> = item
        .keys
        .iter()
        .filter(|k| series.chapters.iter().any(|c| &c.key == *k))
        .cloned()
        .collect();
    (!keys.is_empty()).then(|| WaitingExport {
        series_id: series.id.clone(),
        title: series.title.clone(),
        keys,
    })
}

/// The export that goes next, taken off the waiting list.
fn take_next_waiting(state: AppState) -> Option<WaitingExport> {
    let mut waiting = state.export_waiting;
    let mut list = waiting.write();
    (!list.is_empty()).then(|| list.remove(0))
}

/// Export the saved series one after another, from the first unfinished one.
/// The one running is the first of the plan; the others wait behind it, like
/// downloads, and each is finished (and announced) on its own.
pub(super) fn run_saved_export(state: AppState, saved: SavedExport) {
    // An export can take long and be cut short; what is waiting to be saved
    // is written first.
    state.flush_saves();
    // The ones behind the first, in order, ready before the first starts.
    let mut waiting = state.export_waiting;
    let mut entries = saved
        .items
        .iter()
        .skip(saved.next)
        .filter_map(|item| waiting_export(state, item))
        .collect::<Vec<_>>()
        .into_iter();
    let first = entries.next();
    waiting.set(entries.collect());
    spawn_forever(async move {
        let settings = state.ui.settings.peek().clone();
        let Some(target) = saved.target.clone() else {
            forget_export_plan();
            return;
        };
        let grouping = Grouping::from(saved.grouping);
        let mut current = first;
        while let Some(item) = current.take() {
            let Some(series) = state.library.peek().find(&item.series_id).cloned() else {
                current = take_next_waiting(state);
                continue;
            };
            let keys = item.keys.clone();
            persist_export_plan(
                state,
                &saved,
                &SavedExportItem {
                    series_id: series.id.clone(),
                    keys: keys.clone(),
                },
            );
            let cancel = CancelFlag::new();
            let mut export = ExportJob::new(
                series.clone(),
                keys.clone(),
                settings.device_profile(),
                target.clone(),
                paths().spool_dir(),
            );
            export.language = settings.language.clone();
            export.grouping = grouping;
            if let Some(format) = saved.original_format {
                let library = state.library.peek();
                export.original = Some(pipeline::OriginalExport {
                    format,
                    rtl: logic::export_rtl(library.reading_of(&series.id).map(|p| &p.prefs), series.info.as_ref()),
                });
            }
            let mut job = state.job;
            // Picking up after a pause: carry on from the numbers it stopped at.
            let resumed = job
                .peek()
                .as_ref()
                .filter(|v| v.paused && v.series_id == series.id)
                .map_or((0, 0), |v| (v.progress.done(), v.progress.total()));
            job.set(Some(JobView {
                kind: JobKind::Export,
                title: series.title.clone(),
                series_id: series.id.clone(),
                paused: false,
                progress: JobProgress { resumed, ..Default::default() },
                result: None,
                cancel: cancel.clone(),
            }));
            info!(
                "Export started: \"{}\", {} chapter(s) as {}, {}, to {}",
                series.title,
                keys.len(),
                export
                    .original
                    .as_ref()
                    .map_or_else(|| series.settings.output().extension(), |o| o.format.extension())
                    .to_uppercase(),
                match grouping {
                    Grouping::Single => "one file".to_string(),
                    Grouping::PerChapter => "one file per chapter".to_string(),
                    Grouping::EveryN(n) => format!("one file per {n} chapters"),
                },
                describe_target(&target)
            );
            run_export(state, series, export, target.clone(), cancel).await;
            // Given up while held: the one that took its place is paused, so nothing goes on.
            if !job.peek().as_ref().is_some_and(|v| v.series_id == item.series_id) {
                return;
            }
            let paused = job
                .peek()
                .as_ref()
                .is_some_and(|v| v.paused && v.result.is_none());
            if paused {
                info!("Export paused; it goes on from \"{}\"", item.series_id);
                return;
            }
            // Finished, failed or stopped on purpose: the next one goes on.
            current = take_next_waiting(state);
        }
        forget_export_plan();
    });
}

/// Take a series that is waiting off the list of exports.
pub fn remove_waiting_export(state: AppState, series_id: &str) {
    let mut waiting = state.export_waiting;
    waiting.write().retain(|w| w.series_id != series_id);
    // The plan on disk keeps the one running or paused first, then the rest.
    let saved = JsonFile::<SavedExport>::new(paths().export_job_file()).load();
    if saved.items.len() > 1 {
        let mut kept = saved.clone();
        let first = kept.items.remove(0);
        kept.items.retain(|i| i.series_id != series_id);
        kept.items.insert(0, first);
        save_export_plan(&kept);
    }
}

/// Stop the export for now. What it has converted is kept, and it goes on
/// from where it was when resumed, even after the app is closed.
pub fn pause_export(state: AppState) {
    let mut job = state.job;
    let mut guard = job.write();
    if let Some(view) = guard.as_mut() {
        if view.result.is_none() && !view.paused {
            // Held where it is: what is under way finishes, nothing new starts, and Resume
            // lets it go on with the same run, so nothing starts over.
            view.paused = true;
            view.cancel.pause();
        }
    }
}

/// Carry on with a paused export. If it was sending to a reader and the drive
/// under that letter is not the one it began on (the letter was given to
/// another card or stick meanwhile), ask before writing anything to it.
pub fn resume_export(state: AppState) {
    let paused = state.job.peek().as_ref().is_some_and(|v| v.paused);
    if !paused {
        return;
    }
    // A run that is only being held (the app was not closed) just goes on.
    let mut job = state.job;
    if let Some(view) = job.write().as_mut().filter(|v| v.cancel.is_paused()) {
        view.cancel.resume();
        view.paused = false;
        return;
    }
    let saved = JsonFile::<SavedExport>::new(paths().export_job_file()).load();
    let place = match (&saved.target, &saved.drive) {
        (Some(ExportTarget::Device { mount, .. }), Some(_)) => Some(mount.clone()),
        _ => None,
    };
    let Some(place) = place else {
        resume_export_unchecked(state);
        return;
    };
    spawn_forever(async move {
        let drives = tokio::task::spawn_blocking(pipeline::export::removable_drives)
            .await
            .unwrap_or_default();
        match logic::check_drive(saved.drive.as_ref(), &place, &drives) {
            logic::DriveCheck::Unchecked | logic::DriveCheck::Same => resume_export_unchecked(state),
            logic::DriveCheck::Missing(was) => state.error(format!(
                "The reader drive ({}) is not connected. Plug it in, then carry on.",
                was.describe()
            )),
            logic::DriveCheck::Changed { was, now } => {
                let mut modal = state.modal;
                modal.set(Some(Modal {
                    title: "Is this the same drive?".into(),
                    body: format!(
                        "This export began on {}. The drive at {} now is {}. Send the files there anyway?",
                        was.describe(),
                        place.display(),
                        now.describe()
                    ),
                    confirm: "Send anyway".into(),
                    cancel: "Not yet".into(),
                    danger: true,
                    action: ModalAction::ResumeExportAnyway,
                }));
            }
        }
    });
}

/// Carry on with a paused export without looking at the drive.
pub fn resume_export_unchecked(state: AppState) {
    let paused = state.job.peek().as_ref().is_some_and(|v| v.paused);
    if !paused {
        return;
    }
    let saved = JsonFile::<SavedExport>::new(paths().export_job_file()).load();
    if saved.items.is_empty() || saved.next >= saved.items.len() {
        let mut job = state.job;
        job.set(None);
        let mut waiting = state.export_waiting;
        waiting.set(Vec::new());
        forget_export_plan();
        return;
    }
    run_saved_export(state, saved);
}

/// A paused export is given up. The one waiting behind it, if any, takes its
/// place (paused, so nothing starts by itself).
pub fn drop_paused_export(state: AppState) {
    let mut job = state.job;
    // A run that is only being held ends; it does not go on to the next one by itself.
    if let Some(view) = job.peek().as_ref() {
        view.cancel.cancel();
    }
    job.set(None);
    let mut saved = JsonFile::<SavedExport>::new(paths().export_job_file()).load();
    if saved.next < saved.items.len() {
        saved.items.remove(saved.next);
    }
    saved.next = 0;
    if saved.items.is_empty() {
        let mut waiting = state.export_waiting;
        waiting.set(Vec::new());
        forget_export_plan();
    } else {
        save_export_plan(&saved);
        restore_export(state);
    }
}

/// An export that was not finished when the app was closed comes back paused,
/// to be resumed when you want it (it can take a lot of the computer).
pub fn restore_export(state: AppState) {
    let saved = JsonFile::<SavedExport>::new(paths().export_job_file()).load();
    let mut entries = saved
        .items
        .iter()
        .skip(saved.next)
        .filter_map(|item| waiting_export(state, item))
        .collect::<Vec<_>>()
        .into_iter();
    let Some(first) = entries.next() else {
        forget_export_plan();
        return;
    };
    let mut waiting = state.export_waiting;
    waiting.set(entries.collect());
    let mut job = state.job;
    job.set(Some(JobView {
        kind: JobKind::Export,
        title: first.title.clone(),
        series_id: first.series_id.clone(),
        paused: true,
        progress: JobProgress::default(),
        result: None,
        cancel: CancelFlag::new(),
    }));
    info!("An export of \"{}\" was not finished; it waits to be resumed", first.title);
}

/// Run one export, keeping the job bar up to date, and record the result.
pub(super) async fn run_export(
    state: AppState,
    series: library::DownloadedSeries,
    export: ExportJob,
    target: ExportTarget,
    cancel: CancelFlag,
) {
    let mut job = state.job;
    let started = Instant::now();
    let files = library_files();
    let (tx, mut rx) = unbounded_channel();
    let run = export_series(&files, &export, Some(tx), &cancel);
    let watch = async {
        let mut logged = HashMap::new();
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while let Ok(event) = rx.try_recv() {
                batch.push(event);
            }
            for event in &batch {
                log_event(event, &mut logged);
            }
            if let Some(view) = job.write().as_mut() {
                for event in &batch {
                    view.progress.apply(event);
                }
            }
            tokio::time::sleep(PROGRESS_REFRESH).await;
        }
    };
    let (result, ()) = join(run, watch).await;

    match result {
        Ok(outcome) => {
            info!(
                "Export finished in {:.0}s: {} pages from {} chapter(s)",
                started.elapsed().as_secs_f32(),
                outcome.pages,
                outcome.chapters
            );
            for f in &outcome.files {
                info!("Wrote {} ({})", f.location, logic::human_bytes(f.bytes));
            }
            let speed = job
                .peek()
                .as_ref()
                .and_then(|v| v.progress.writing.as_ref())
                .and_then(|w| w.average_bytes_per_second());
            if let Some(speed) = speed {
                info!("Sent to {} at {}/s on average", describe_target(&target), logic::human_bytes(speed as u64));
            }
            let record = ExportRecord {
                id: format!("{}-{}", series.id, now_secs()),
                series_id: series.id.clone(),
                title: series.title.clone(),
                chapters: outcome.chapters,
                pages: outcome.pages,
                format: export
                    .original
                    .as_ref()
                    .map_or_else(|| series.settings.output().extension(), |o| o.format.extension())
                    .to_string(),
                files: outcome
                    .files
                    .iter()
                    .map(|f| ExportedFileRecord {
                        path: f.location.clone(),
                        bytes: f.bytes,
                    })
                    .collect(),
                target: describe_target(&target),
                preset_id: series.preset_id.clone(),
                settings: Some(series.settings.clone()),
                chapter_keys: export.chapter_keys.clone(),
                at: now_secs(),
            };
            let mut exports = state.exports;
            exports.write().add(record);
            state.save_exports();
            let mut summary = format!("Exported {} file(s)", outcome.files.len());
            // Files sent to a drive were flushed to it before they got their names.
            if matches!(target, ExportTarget::Device { .. }) {
                summary.push_str(". Safe to unplug.");
            } else if matches!(target, ExportTarget::WebDav { .. }) {
                summary.push_str(". Sent to the reader.");
            }
            state.notify(format!("{}: {}", series.title, summary.to_lowercase()));
            crate::notify::finished(state, "Export finished", &format!("{}: {summary}", series.title));
            if let Some(view) = job.write().as_mut() {
                view.result = Some(Ok(summary));
            }
        }
        Err(e) => {
            let cancelled = matches!(e, PipelineError::Cancelled);
            let message = if cancelled {
                "Cancelled.".to_string()
            } else {
                e.to_string()
            };
            let paused = job.peek().as_ref().is_some_and(|v| v.paused);
            if cancelled && paused {
                info!("Export stopped for now; the converted chapters are kept");
            } else if cancelled {
                info!("Export cancelled");
            } else {
                warn!("Export failed: {message}");
            }
            if !cancelled {
                crate::notify::finished(state, "Export failed", &format!("{}: {message}", series.title));
                state.error(format!("{}: {message}", series.title));
            }
            if let Some(view) = job.write().as_mut() {
                if cancelled && view.paused {
                    // Paused, not stopped: it stays as it was, to be resumed.
                    view.progress.stopping = false;
                } else {
                    view.result = Some(Err(message));
                }
            }
        }
    }
}

pub fn describe_target(target: &ExportTarget) -> String {
    match target {
        ExportTarget::Folder { path } => format!("folder {}", path.display()),
        ExportTarget::Device { mount, folder } => {
            format!("the reader at {}", mount.join(folder).display())
        }
        ExportTarget::WebDav { base_url, .. } => format!("the reader over Wi-Fi ({base_url})"),
    }
}
