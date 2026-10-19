//! The Export screen's list of series, and starting the export.

use crate::ui::work::*;
use super::*;

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
    // Whatever the screen shows, the files are made in something the reader opens.
    crate::ui::presets::fit_output_to_reader(state);
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
    // Which drive the reader is on now, so a resume after a restart can tell
    // if another drive has taken its letter.
    let drive = match &target {
        ExportTarget::Device { mount, .. } => {
            logic::identity_of(mount, &pipeline::export::removable_drives())
        }
        _ => None,
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
        drive,
        next: 0,
    };
    save_export_plan(&saved);
    run_saved_export(state, saved);
}
