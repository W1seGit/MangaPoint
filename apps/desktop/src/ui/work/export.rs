//! The Export screen: its list of series and the export itself.

use super::*;
use library::JsonFile;
use serde::{Deserialize, Serialize};

/// Open the Export screen for these series. Only series with downloaded
/// chapters can be exported, so the others are left out.
pub fn open_export(state: AppState, series_ids: Vec<String>) {
    let items: Vec<ExportItem> = series_ids
        .iter()
        .filter(|id| {
            state
                .library
                .peek()
                .find(id)
                .is_some_and(|s| !s.chapters.is_empty())
        })
        .map(|id| ExportItem {
            series_id: id.clone(),
            excluded: already_exported(state, id),
            ..Default::default()
        })
        .collect();
    if items.is_empty() {
        state.error("Download some chapters first, then export them.");
        return;
    }
    let mut plan = state.export_plan;
    plan.set(ExportPlan { items, active: 0 });
    activate_export_series(state, 0);
    state.go(Page::Export);
}

/// The chapters of a series to leave out when it is exported again: the ones
/// that were exported before, so what is left is "the other half". Nothing is
/// left out if everything was exported already (or nothing was).
pub fn already_exported(state: AppState, series_id: &str) -> HashSet<String> {
    let exported = state.exports.peek().exported_keys(series_id);
    let library = state.library.peek();
    let Some(series) = library.find(series_id) else {
        return HashSet::new();
    };
    let done: HashSet<String> = series
        .chapters
        .iter()
        .filter(|c| exported.contains(&c.key))
        .map(|c| c.key.clone())
        .collect();
    if done.len() == series.chapters.len() {
        HashSet::new()
    } else {
        done
    }
}

/// Open the Export screen on the sample pages, to look around and try presets,
/// when nothing has been chosen to export.
pub fn open_sample(state: AppState) {
    let id = crate::sample::ensure_sample(state);
    let mut plan = state.export_plan;
    plan.set(ExportPlan {
        items: vec![ExportItem {
            series_id: id,
            ..Default::default()
        }],
        active: 0,
    });
    activate_export_series(state, 0);
    state.go(Page::Export);
}

/// Make the Export screen's list agree with the library: a series that was
/// removed, or whose downloads were all deleted, cannot be previewed or
/// exported, so it goes off the list. With none left the sample pages come
/// back; otherwise the series on show is one that is still there. Does
/// nothing when the list is already right.
pub fn reconcile_export_plan(state: AppState) {
    let plan = state.export_plan.peek().clone();
    if plan.items.is_empty() {
        return;
    }
    let usable = |id: &str| {
        state
            .library
            .peek()
            .find(id)
            .is_some_and(|s| !s.chapters.is_empty())
    };
    let kept: Vec<ExportItem> = plan
        .items
        .iter()
        .filter(|i| usable(&i.series_id))
        .cloned()
        .collect();
    if kept.len() != plan.items.len() {
        if kept.is_empty() {
            // Nothing left: the sample pages, as when the screen is first opened.
            let id = crate::sample::ensure_sample(state);
            let mut plan = state.export_plan;
            plan.set(ExportPlan {
                items: vec![ExportItem {
                    series_id: id,
                    ..Default::default()
                }],
                active: 0,
            });
            activate_export_series(state, 0);
        } else {
            let active = plan.active.min(kept.len() - 1);
            let mut plan = state.export_plan;
            plan.set(ExportPlan { items: kept, active });
            activate_export_series(state, active);
        }
        return;
    }
    // The list is right, but what is on show is a series that is gone.
    let shown = state.workspace.peek().as_ref().map(|w| w.series_id.clone());
    if shown.as_deref().is_some_and(|id| !usable(id)) {
        activate_export_series(state, plan.active.min(plan.items.len() - 1));
    }
}

/// Edit and preview this series of the Export screen's list.
pub fn activate_export_series(state: AppState, index: usize) {
    let id = state
        .export_plan
        .peek()
        .items
        .get(index)
        .map(|i| i.series_id.clone());
    let Some(id) = id else { return };
    // Whatever series was open waits until the Export screen is left.
    let open = state.workspace.peek().clone();
    if let Some(open) = open.filter(|w| w.origin != Page::Export) {
        let mut parked = state.parked_workspace;
        parked.set(Some(open));
    }
    let series_id = id.clone();
    let title = ws_title(state, &id);
    let mut plan = state.export_plan;
    plan.write().active = index;
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: id,
        title,
        ranked: None,
        origin: Page::Export,
    }));
    {
        let mut preview = state.preview;
        let mut p = preview.write();
        p.chapter_key = None;
        p.pages.clear();
        p.fresh = 0;
        p.selected = 0;
        p.thumb_start = 0;
        p.window = (0, 0);
        p.draft_crop = None;
        p.shading_samples.clear();
        // Back where it was, if the screen was left and opened again.
        let remembered = state
            .export_plan
            .peek()
            .items
            .get(index)
            .and_then(|i| i.chapter.clone());
        let valid = remembered.filter(|key| {
            state
                .library
                .peek()
                .find(&series_id)
                .is_some_and(|s| s.chapters.iter().any(|c| &c.key == key))
        });
        p.chapter_key = valid;
    }
    let mut draft = state.crop_draft;
    draft.set(library::CropRect::default());
    prepare_preview(state);
}

/// Add a series to the Export screen's list.
pub fn add_export_series(state: AppState, series_id: String) {
    // A series that is gone from the list's point of view must not hold the
    // place the new one is meant to take.
    reconcile_export_plan(state);
    let has_chapters = state
        .library
        .peek()
        .find(&series_id)
        .is_some_and(|s| !s.chapters.is_empty());
    if !has_chapters {
        return;
    }
    let samples: Vec<String> = state
        .library
        .peek()
        .series
        .iter()
        .filter(|s| s.is_sample())
        .map(|s| s.id.clone())
        .collect();
    let mut plan = state.export_plan;
    let replaced_sample = {
        let mut p = plan.write();
        if p.items.iter().any(|i| i.series_id == series_id) {
            return;
        }
        let excluded = already_exported(state, &series_id);
        let only_sample = p.items.iter().all(|i| samples.contains(&i.series_id));
        p.items.push(ExportItem {
            series_id,
            excluded,
            ..Default::default()
        });
        // The sample pages are only there to look around with until a real
        // series is added: they go, and the new series is the one on show.
        if only_sample {
            p.items.retain(|i| !samples.contains(&i.series_id));
            p.active = 0;
        }
        only_sample
    };
    if replaced_sample {
        activate_export_series(state, 0);
    }
}

/// Take a series off the Export screen's list. With none left, go back.
pub fn remove_export_series(state: AppState, index: usize) {
    let (left, active) = {
        let mut plan = state.export_plan;
        let mut p = plan.write();
        if index >= p.items.len() {
            return;
        }
        p.items.remove(index);
        let last = p.items.len().saturating_sub(1);
        p.active = if index < p.active || p.active > last {
            p.active.saturating_sub(1).min(last)
        } else {
            p.active
        };
        (p.items.len(), p.active)
    };
    if left == 0 {
        // Nothing left to export: the sample pages come back, as when the
        // screen was first opened, and the screen stays where it is.
        open_sample(state);
    } else {
        activate_export_series(state, active);
    }
}

/// Export every series switched on in the Export screen's list, one after
/// another, to the chosen place.
pub fn start_export_plan(state: AppState) {
    if job_running(state) {
        state.error("Something is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let plan = state.export_plan.peek().clone();
    let opts = state.export_opts.peek().clone();
    let settings = state.settings.peek().clone();
    let list: Vec<(library::DownloadedSeries, Vec<String>)> = plan
        .items
        .iter()
        .filter(|i| !i.off)
        .filter_map(|i| {
            let series = state.library.peek().find(&i.series_id).cloned()?;
            if series.is_sample() {
                return None;
            }
            let keys: Vec<String> = series
                .chapters
                .iter()
                .filter(|c| !i.excluded.contains(&c.key))
                .map(|c| c.key.clone())
                .collect();
            (!keys.is_empty()).then_some((series, keys))
        })
        .collect();
    if list.is_empty() {
        state.error("Choose at least one chapter to export.");
        return;
    }
    let target = match logic::export_target(&opts.target, &settings, &paths().default_export_dir())
    {
        Ok(t) => t,
        Err(message) => {
            state.error(message);
            return;
        }
    };
    let saved = SavedExport {
        items: list
            .iter()
            .map(|(series, keys)| SavedExportItem {
                series_id: series.id.clone(),
                keys: keys.clone(),
            })
            .collect(),
        grouping: SavedGrouping::from(grouping_of(&opts)),
        target: Some(target),
        next: 0,
    };
    save_export_plan(&saved);
    run_saved_export(state, saved);
}

/// What is kept of an export that has not finished, so it can go on after the
/// app was closed. The chapters already converted are kept too, so going on
/// does not do them again.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedExport {
    items: Vec<SavedExportItem>,
    grouping: SavedGrouping,
    target: Option<ExportTarget>,
    /// The first series that is not finished.
    next: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedExportItem {
    series_id: String,
    keys: Vec<String>,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
enum SavedGrouping {
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

fn save_export_plan(saved: &SavedExport) {
    let _ = JsonFile::new(paths().export_job_file()).save(saved);
}

fn forget_export_plan() {
    let _ = std::fs::remove_file(paths().export_job_file());
}

/// Write down what is left to do: the series being exported (or paused) and
/// those waiting for it. The plan on disk always says where to go on from.
fn persist_export_plan(state: AppState, base: &SavedExport, current: &SavedExportItem) {
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
fn run_saved_export(state: AppState, saved: SavedExport) {
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
        let settings = state.settings.peek().clone();
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
            let mut job = state.job;
            job.set(Some(JobView {
                kind: JobKind::Export,
                title: series.title.clone(),
                series_id: series.id.clone(),
                paused: false,
                progress: JobProgress::default(),
                result: None,
                cancel: cancel.clone(),
            }));
            info!(
                "Export started: \"{}\", {} chapter(s) as {}, {}, to {}",
                series.title,
                keys.len(),
                series.settings.output().extension().to_uppercase(),
                match grouping {
                    Grouping::Single => "one file".to_string(),
                    Grouping::PerChapter => "one file per chapter".to_string(),
                    Grouping::EveryN(n) => format!("one file per {n} chapters"),
                },
                describe_target(&target)
            );
            run_export(state, series, export, target.clone(), cancel).await;
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
            view.paused = true;
            view.cancel.cancel();
            view.progress.stopping = true;
        }
    }
}

/// Carry on with a paused export.
pub fn resume_export(state: AppState) {
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
                format: series.settings.output().extension().to_string(),
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
            if cancelled {
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

/// One readable line per meaningful step; page progress every 25 pages of
/// each chapter (chapters can download together, so progress is kept per chapter).
pub(super) fn log_event(event: &JobEvent, logged_pages: &mut HashMap<usize, usize>) {
    match event {
        JobEvent::ChapterStarted { index, total, name } => {
            logged_pages.insert(*index, 0);
            info!("Chapter {}/{}: {name}", index + 1, total);
        }
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        } => {
            let last = logged_pages.entry(*chapter_index).or_insert(0);
            if done / 25 > *last / 25 || done == total {
                info!("  chapter {}: {done}/{total} pages", chapter_index + 1);
            }
            *last = *done;
        }
        JobEvent::UsedFallback {
            from, to, reason, ..
        } => {
            warn!("  source {from} failed ({reason}); trying {to}");
        }
        JobEvent::ChapterSkipped { name, reason, .. } => {
            warn!("  skipped {name}: {reason}");
        }
        JobEvent::ChapterSaved { chapter } => {
            info!(
                "  saved {} ({} pages, {})",
                chapter.name,
                chapter.pages,
                logic::human_bytes(chapter.bytes)
            );
        }
        JobEvent::ChapterDone { .. } => {}
        JobEvent::PagesAtOnce { now, chosen } => {
            if now < chosen {
                warn!("  the source is pushing back: {now} requests at once instead of {chosen}");
            } else {
                info!("  the source recovered: back to {chosen} requests at once");
            }
        }
        JobEvent::Writing { file } => info!("Writing {file}"),
        JobEvent::Sent { .. } => {}
        JobEvent::Flushing => info!("The file is copied; waiting for the drive to store it"),
    }
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
