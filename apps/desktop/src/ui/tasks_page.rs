//! Tasks: the download queue (what is downloading now, what is next, what
//! finished) and the exports (the one running, and every file made so far).
//! Chapters are added from any series; they download in turn, so there is
//! nothing to start by hand.

use super::widgets::{open_folder, show_in_folder, Chip, Cover, EmptyState, Icon, NoCover, ProgressBar};
use super::work::{
    cancel_batch, cancel_job, dismiss_batch, forget_exported_file,
    load_saved_cover, open_series_from_library, pause_batch, pause_export, prioritize_batch,
    remove_waiting_export, resume_batch, resume_export, retry_batch, saved_cover_key, set_queue_paused,
};
use crate::logic::{ago, human_bytes, human_duration, plural, JobProgress};
use crate::queue::BatchStatus;
use crate::state::{library_files, now_secs, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;

#[component]
pub fn TasksPage() -> Element {
    let state = use_context::<AppState>();
    // Covers of whatever is in the queue.
    use_effect(move || {
        let ids: Vec<String> = state
            .queue
            .read()
            .batches
            .iter()
            .map(|b| b.series_id.clone())
            .collect();
        for id in ids {
            load_saved_cover(state, &id);
        }
        let exported: Vec<String> = state
            .exports
            .read()
            .records
            .iter()
            .map(|r| r.series_id.clone())
            .collect();
        for id in exported {
            load_saved_cover(state, &id);
        }
        let waiting: Vec<String> = state
            .export_waiting
            .read()
            .iter()
            .map(|w| w.series_id.clone())
            .collect();
        for id in waiting {
            load_saved_cover(state, &id);
        }
    });

    // How the queue is laid out (which batch is running, which wait, which are
    // finished), not how far the running one has got: each row follows its own
    // progress, and the page is not drawn again for every page downloaded.
    let (running, waiting, finished, (waiting_batches, chapters_left), paused) = use_memo(move || {
        let queue = state.queue.read();
        let waiting: Vec<u64> = queue
            .batches
            .iter()
            .filter(|b| matches!(b.status, BatchStatus::Waiting | BatchStatus::Paused))
            .map(|b| b.id)
            .collect();
        // Newest first.
        let finished: Vec<u64> = queue
            .batches
            .iter()
            .rev()
            .filter(|b| !b.status.is_active())
            .map(|b| b.id)
            .collect();
        let (_, waiting_batches, chapters_left) = queue.counts();
        (
            queue.running().map(|b| b.id),
            waiting,
            finished,
            (waiting_batches, chapters_left),
            queue.paused,
        )
    })();
    let busy = running.is_some() || !waiting.is_empty();
    // Only whether an export is on show matters here (the row itself follows
    // its progress), so this page is not drawn again for every page converted.
    let job_shown = use_memo(move || {
        state
            .job
            .read()
            .as_ref()
            .map(|j| !matches!(j.result, Some(Ok(_))))
    })();
    let nothing_exported = state.exports.read().records.is_empty();
    // The series lined up behind the export that is running.
    let waiting_exports: Vec<String> = state
        .export_waiting
        .read()
        .iter()
        .map(|w| w.series_id.clone())
        .collect();

    rsx! {
        div { class: "rise-in",
        div { class: "q-head",
            div { class: "grow",
                h1 { "Tasks" }
                p { class: "muted", "Downloads and exports, running and finished." }
            }
        }

        if !busy && finished.is_empty() && job_shown.is_none() && nothing_exported {
            EmptyState {
                title: "Nothing to do",
                message: "Pick chapters in any series and add them to the downloads, or export what you have. They go on in the background while you carry on.",
                scene: "waiting",
                side: true,
                button { class: "btn primary", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
            }
        } else {
            div { class: "q-sect",
                h2 { "Downloads" }
                if busy {
                    span { class: "muted", "{plural(chapters_left, \"chapter\")} to go"
                        if waiting_batches > 0 { " · {waiting_batches} more in line" }
                        if paused { " · paused" }
                    }
                }
                span { class: "grow" }
                if !finished.is_empty() {
                    button { class: "btn",
                        onclick: {
                            let finished_count = finished.len();
                            move |_| state.ask(Modal {
                            title: "Clear the finished downloads?".into(),
                            body: format!("{} will be taken off this list. The chapters stay downloaded.", plural(finished_count, "finished download")),
                            confirm: "Clear".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::ClearFinishedDownloads,
                        }) },
                        "Clear all"
                    }
                }
                if busy {
                    button { class: "btn", onclick: move |_| set_queue_paused(state, !paused),
                        if paused { Icon { name: "play", size: 15 } "Resume" } else { Icon { name: "pause", size: 15 } "Pause" }
                    }
                    button { class: "btn danger",
                        onclick: move |_| state.ask(Modal {
                            title: "Cancel all downloads?".into(),
                            body: "The download that is running stops, and the ones waiting are taken out of the queue. What was already downloaded is kept.".into(),
                            confirm: "Cancel all".into(),
                            cancel: "Keep downloading".into(),
                            danger: true,
                            action: ModalAction::CancelAllDownloads,
                        }),
                        Icon { name: "x", size: 15 } "Cancel all"
                    }
                }
            }
            if !busy && finished.is_empty() {
                p { class: "muted small q-hint", "Chapters you add from any series are downloaded here, one series after another." }
            }
            // One list, like the exports: the one downloading, then those
            // waiting, then the finished ones. Each row says which it is.
            if let Some(id) = running {
                BatchRow { key: "{id}", id }
            }
            for id in waiting.iter().copied() {
                BatchRow { key: "{id}", id }
            }
            for id in finished.iter().copied() {
                BatchRow { key: "{id}", id }
            }

            div { class: "q-sect q-sect-next",
                h2 { "Exports" }
            }
            // A finished export is the first file in the list below.
            if job_shown == Some(true) {
                ExportJobRow {}
            }
            for id in waiting_exports.iter() {
                ExportWaitingRow { key: "{id}", series_id: id.clone() }
            }
            ExportedFiles {}
        }
        }
    }
}

/// The export that is running, paused, or has just ended: how far it is, how
/// fast, and how long it should still take.
#[component]
fn ExportJobRow() -> Element {
    let state = use_context::<AppState>();
    let Some(job) = state.job.read().clone() else {
        return rsx! {};
    };
    let running = job.result.is_none() && !job.paused;
    let result_text = match &job.result {
        Some(Ok(text)) | Some(Err(text)) => text.clone(),
        None => String::new(),
    };
    let speed = job.progress.pages_per_second();
    let left = job.progress.eta_secs();
    let has_cover = state
        .library
        .read()
        .find(&job.series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let cover_key = saved_cover_key(&job.series_id);
    let ended = job.result.is_some();
    rsx! {
        div { class: if ended { "q-row" } else { "q-row running" },
            div { class: "q-cover",
                if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
            }
            div { class: "q-main",
                div { class: "row wrap",
                    strong { class: "q-name", "{job.title}" }
                    match &job.result {
                        None if job.paused => rsx! { Chip { text: "paused", kind: Some("warn".to_string()) } },
                        None if job.progress.writing.is_some() => rsx! { Chip { text: "writing", kind: Some("accent".to_string()) } },
                        None => rsx! { Chip { text: "exporting", kind: Some("accent".to_string()) } },
                        Some(Ok(_)) => rsx! { Chip { text: "done", kind: Some("ok".to_string()) } },
                        Some(Err(_)) => rsx! { Chip { text: "stopped", kind: Some("bad".to_string()) } },
                    }
                }
                if !ended {
                    div { class: "q-stats", style: "margin: 2px 0 8px",
                        if let Some(w) = &job.progress.writing {
                            span { "Writing {human_bytes(w.done)} of {human_bytes(w.total)}" }
                            if running {
                                if w.flushing {
                                    span { "making sure the drive has all of it" }
                                } else {
                                    if let Some(v) = w.bytes_per_second() { span { "{human_bytes(v as u64)} a second" } }
                                    if let Some(s) = w.eta_secs() { span { "about {human_duration(s)} left" } }
                                }
                            } else {
                                span { "goes on from here when resumed, even after the app is closed" }
                            }
                        } else {
                            if job.progress.chapter_total > 0 {
                                span { "{job.progress.finished} of {plural(job.progress.chapter_total, \"chapter\")} done" }
                            }
                            if running {
                                if let Some(v) = speed { span { "about {v:.1} pages a second" } }
                                if let Some(s) = left { span { "about {human_duration(s)} left" } }
                            } else {
                                span { "goes on from here when resumed, even after the app is closed" }
                            }
                        }
                    }
                    ProgressBar { fraction: Some(job.progress.fraction()) }
                    if running && job.progress.writing.is_none() { ActiveChapters { progress: job.progress.clone() } }
                    if let Some(note) = &job.progress.note {
                        div { class: "muted small q-line", "{note}" }
                    }
                } else {
                    div { class: "muted small", "{result_text}" }
                }
            }
            div { class: "q-actions",
                if ended {
                    button { class: "btn small ghost", title: "Dismiss", onclick: move |_| { let mut j = state.job; j.set(None); },
                        Icon { name: "x", size: 14 }
                    }
                } else if job.paused {
                    button { class: "btn small", onclick: move |_| resume_export(state),
                        Icon { name: "play", size: 14 } "Resume"
                    }
                    button { class: "btn small ghost", title: "Stop it for good", onclick: move |_| cancel_job(state),
                        Icon { name: "x", size: 14 }
                    }
                } else {
                    button { class: "btn small", disabled: job.progress.stopping, onclick: move |_| pause_export(state),
                        Icon { name: "pause", size: 14 }
                        if job.progress.stopping { "Pausing…" } else { "Pause" }
                    }
                    button { class: "btn small ghost", title: "Stop it for good", disabled: job.progress.stopping, onclick: move |_| cancel_job(state),
                        Icon { name: "x", size: 14 }
                    }
                }
            }
        }
    }
}

/// A series whose export is lined up behind the one running.
#[component]
fn ExportWaitingRow(series_id: String) -> Element {
    let state = use_context::<AppState>();
    let Some(waiting) = state
        .export_waiting
        .read()
        .iter()
        .find(|w| w.series_id == series_id)
        .cloned()
    else {
        return rsx! {};
    };
    let has_cover = state
        .library
        .read()
        .find(&series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let remove_id = series_id.clone();
    rsx! {
        div { class: "q-row",
            div { class: "q-cover",
                if has_cover { Cover { key_id: saved_cover_key(&series_id) } } else { NoCover {} }
            }
            div { class: "q-main",
                div { class: "row wrap",
                    strong { class: "q-name", "{waiting.title}" }
                    Chip { text: "waiting" }
                }
                div { class: "muted small", "{plural(waiting.keys.len(), \"chapter\")} waiting" }
            }
            div { class: "q-actions",
                button { class: "btn small ghost", title: "Take it out of the line",
                    onclick: move |_| remove_waiting_export(state, &remove_id),
                    Icon { name: "x", size: 14 }
                }
            }
        }
    }
}

/// Every export, newest first. The title opens the series; Open shows the
/// file in its folder.
#[component]
fn ExportedFiles() -> Element {
    let state = use_context::<AppState>();
    let mut records = state.exports.read().records.clone();
    records.reverse();
    let now = now_secs();
    // The export whose menu is open.
    let mut menu = use_signal(|| None::<String>);
    // Which of the files are still on disk is looked at in the background, when
    // the list changes and every few seconds, never while drawing: a file on a
    // sleeping drive can take seconds to answer. Until the first answer every
    // file is taken to be there.
    let listed = use_memo(move || {
        state
            .exports
            .read()
            .records
            .iter()
            .flat_map(|r| r.files.iter().map(|f| f.path.clone()))
            .filter(|p| !p.starts_with("http"))
            .collect::<Vec<String>>()
    });
    let mut present = use_resource(move || async move {
        let paths = listed();
        tokio::task::spawn_blocking(move || {
            paths
                .into_iter()
                .filter(|p| std::path::Path::new(p).is_file())
                .collect::<std::collections::HashSet<String>>()
        })
        .await
        .ok()
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            present.restart();
        }
    });
    let present = present.read().clone().flatten();
    let is_there = move |path: &str| path.starts_with("http") || present.as_ref().is_none_or(|set| set.contains(path));
    rsx! {
        if records.is_empty() {
            p { class: "muted small", style: "margin: 4px 0 0",
                "Files you export for your reader are listed here, so you can find or delete them later."
            }
        }
        for r in records.iter() {
            {
                let (has_cover, in_library) = {
                    let library = state.library.read();
                    let series = library.find(&r.series_id);
                    (
                        series.and_then(|s| s.info.as_ref()).is_some_and(|i| i.has_cover),
                        series.is_some(),
                    )
                };
                let first = r.files.first().map(|f| f.path.clone()).unwrap_or_default();
                let can_open = !first.is_empty() && !first.starts_with("http") && is_there(&first);
                let total: u64 = r.files.iter().map(|f| f.bytes).sum();
                let place_full = r.target.strip_prefix("folder ").unwrap_or(&r.target).to_string();
                let place = place_full.clone();
                let series_id = r.series_id.clone();
                let menu_open = menu().as_deref() == Some(r.id.as_str());
                let menu_id = r.id.clone();
                let (preset_title, preset_settings) = (r.title.clone(), r.settings.clone());
                let delete_record = r.id.clone();
                let delete_modal = Modal {
                    title: format!("Delete the export of {}?", r.title),
                    body: format!(
                        "{} will be deleted from your computer. Your downloaded chapters are not affected.",
                        plural(r.files.len(), "file")
                    ),
                    confirm: "Delete".into(),
                    cancel: "Keep".into(),
                    danger: true,
                    action: ModalAction::DeleteExport { record_id: delete_record },
                };
                rsx! {
                    div { class: "q-row q-done", key: "{r.id}",
                        div { class: "q-cover",
                            if has_cover { Cover { key_id: saved_cover_key(&r.series_id) } } else { NoCover {} }
                        }
                        div { class: "q-main",
                            div { class: "row wrap",
                                button { class: "link-title", disabled: !in_library,
                                    title: if in_library { "Open this series" } else { "This series is no longer in your library" },
                                    onclick: move |_| open_series_from_library(state, &series_id),
                                    "{r.title}"
                                }
                                Chip { text: r.format.to_uppercase() }
                            }
                            div { class: "q-stats",
                                span { "{plural(r.chapters, \"chapter\")} · {plural(r.pages, \"page\")}" }
                                span { "{human_bytes(total)}" }
                            }
                            div { class: "q-when-line", "Exported {ago(now, r.at)}" }
                            if r.files.len() > 1 {
                                for f in r.files.iter() {
                                    {
                                        let path = f.path.clone();
                                        let missing = !is_there(&path);
                                        let name = std::path::Path::new(&path)
                                            .file_name()
                                            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                                        let size = human_bytes(f.bytes);
                                        let record_id = r.id.clone();
                                        let (p_delete, p_forget) = (path.clone(), path.clone());
                                        let (id_delete, id_forget) = (record_id.clone(), record_id.clone());
                                        rsx! {
                                            div { class: "exp-file", key: "{path}",
                                                span { class: "exp-path", title: "{path}", "{name}" }
                                                if missing { Chip { text: "missing", kind: Some("warn".to_string()) } } else { span { class: "muted small", "{size}" } }
                                                if missing {
                                                    button { class: "btn small ghost", title: "Take it off this list",
                                                        onclick: move |_| forget_exported_file(state, id_forget.clone(), p_forget.clone()),
                                                        Icon { name: "x", size: 14 }
                                                    }
                                                } else {
                                                    button { class: "btn small ghost danger", title: "Delete this file",
                                                        onclick: move |_| ask_delete_export(state, id_delete.clone(), p_delete.clone()),
                                                        Icon { name: "trash", size: 14 }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "q-aside",
                            button { class: "path-link", disabled: !can_open,
                                title: if can_open { "Show the file in its folder: {place_full}" } else { "The file is not there any more" },
                                onclick: move |_| show_in_folder(&first),
                                span { class: "path-text", "{place}" }
                                Icon { name: "external-link", size: 15 }
                            }
                            div { class: "menu-anchor",
                                button { class: "btn ghost open-big square", title: "More actions",
                                    onclick: move |_| menu.set(if menu_open { None } else { Some(menu_id.clone()) }),
                                    Icon { name: "ellipsis", size: 18 }
                                }
                                if menu_open {
                                    div { class: "menu-scrim", onclick: move |_| menu.set(None) }
                                    div { class: "menu down",
                                        button { class: "menu-item", disabled: preset_settings.is_none(),
                                            title: if preset_settings.is_some() { "Keep how this series was converted for this export as a preset" } else { "This export did not keep its settings" },
                                            onclick: move |_| {
                                                menu.set(None);
                                                save_export_preset(state, &preset_title, preset_settings.clone());
                                            },
                                            Icon { name: "layers", size: 15 } "Save its settings as a preset"
                                        }
                                        button { class: "menu-item danger",
                                            onclick: move |_| {
                                                menu.set(None);
                                                state.ask(delete_modal.clone());
                                            },
                                            Icon { name: "trash", size: 15 } "Delete"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Ask before a file that was exported is deleted from the computer.
fn ask_delete_export(state: AppState, record_id: String, path: String) {
    state.ask(Modal {
        title: "Delete this file?".into(),
        body: format!(
            "{path} will be deleted from your computer. Your downloaded chapters are not affected."
        ),
        confirm: "Delete file".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteExportedFile { record_id, path },
    });
}

/// One batch: its series, how far it is, and what can be done with it.
#[component]
fn BatchRow(id: u64) -> Element {
    let state = use_context::<AppState>();
    let Some(batch) = state.queue.read().get(id).cloned() else {
        return rsx! {};
    };
    let has_cover = state
        .library
        .read()
        .find(&batch.series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let in_library = state.library.read().find(&batch.series_id).is_some();
    let series_id = batch.series_id.clone();
    let total = batch.chapter_count();
    let done = batch.chapters_done();
    let running = batch.status == BatchStatus::Running;
    let waiting = batch.status == BatchStatus::Waiting;
    let paused_batch = batch.status == BatchStatus::Paused;
    let speed = batch.progress.pages_per_second();
    let stopping = batch.progress.stopping;
    let failed = batch.failed.len();
    let skipped_lines: Vec<String> = batch.skipped.iter().take(3).cloned().collect();
    let status_chip = match &batch.status {
        BatchStatus::Done if failed == 0 => Some(("done", "ok")),
        BatchStatus::Done => Some(("partly done", "warn")),
        BatchStatus::Failed(_) => Some(("failed", "bad")),
        BatchStatus::Cancelled => Some(("stopped", "warn")),
        BatchStatus::Paused => Some(("paused", "warn")),
        _ => None,
    };
    let cover_key = saved_cover_key(&batch.series_id);
    let (downloaded, downloaded_bytes) = state
        .library
        .read()
        .find(&batch.series_id)
        .map_or((0, 0), |s| (s.chapters.len(), s.bytes()));
    let delete_modal = Modal {
        title: format!("Delete the downloads of {}?", batch.title),
        body: format!(
            "The downloaded chapters ({}) will be deleted from this computer. The series stays in your library, so you can download it again later. Files you already exported are kept.",
            human_bytes(downloaded_bytes)
        ),
        confirm: "Delete downloads".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteDownloads { series_ids: vec![batch.series_id.clone()] },
    };
    let open_id = series_id.clone();
    let folder_id = series_id.clone();
    let folder_full = library_files()
        .cover_path(&series_id)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let folder_text = folder_full.clone();

    rsx! {
        div { class: if running { "q-row running" } else { "q-row" },
            div { class: "q-cover",
                if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
            }
            div { class: "q-main",
                div { class: "row wrap",
                    button { class: "link-title q-name", disabled: !in_library,
                        title: if in_library { "Open this series" } else { "" },
                        onclick: move |_| open_series_from_library(state, &open_id),
                        "{batch.title}"
                    }
                    if let Some((text, kind)) = status_chip { Chip { text: text.to_string(), kind: Some(kind.to_string()) } }
                    if batch.choice != pipeline::SourceChoice::Adaptive { Chip { text: "one source" } }
                }
                if running {
                    div { class: "q-stats", style: "margin: 2px 0 8px",
                        span { "{done} of {plural(total, \"chapter\")} done" }
                        if let Some(v) = speed { span { "about {v:.1} pages a second" } }
                        if let Some(s) = batch.progress.eta_secs() { span { "about {human_duration(s)} left" } }
                    }
                    ProgressBar { fraction: Some(batch.progress.fraction()) }
                    ActiveChapters { progress: batch.progress.clone() }
                    if let Some(note) = &batch.progress.note {
                        div { class: "muted small q-line", "{note}" }
                    }
                } else if waiting || paused_batch {
                    div { class: "muted small",
                        if paused_batch { "{plural(total, \"chapter\")} left · paused" } else { "{plural(total, \"chapter\")} waiting" }
                    }
                } else {
                    div { class: "muted small",
                        if let Some(summary) = &batch.summary { "{summary}" }
                    }
                    if let Some(stats) = batch.stats {
                        div { class: "q-stats",
                            span { "took {human_duration(stats.secs)}" }
                            span { "{plural(stats.pages, \"page\")}" }
                            if stats.secs > 0 && stats.pages > 0 {
                                span { "{stats.pages as f32 / stats.secs as f32:.1} pages a second" }
                            }
                        }
                    }
                    if batch.finished_at > 0 {
                        div { class: "q-when-line", "Completed {ago(now_secs(), batch.finished_at)}" }
                    }
                    for line in skipped_lines.iter() {
                        div { class: "muted small q-line", key: "{line}", "{line}" }
                    }
                }
            }
            div { class: "q-actions",
                if running {
                    button { class: "btn small", disabled: stopping, onclick: move |_| pause_batch(state, id),
                        Icon { name: "pause", size: 14 }
                        if stopping { "Pausing…" } else { "Pause" }
                    }
                    button { class: "btn small ghost", title: "Stop it for good", disabled: stopping, onclick: move |_| cancel_batch(state, id),
                        Icon { name: "x", size: 14 }
                    }
                } else if waiting {
                    button { class: "btn small", title: "Download this next", onclick: move |_| prioritize_batch(state, id),
                        Icon { name: "arrow-up-to-line", size: 14 } "Next"
                    }
                    button { class: "btn small", title: "Set it aside for now", onclick: move |_| pause_batch(state, id),
                        Icon { name: "pause", size: 14 } "Pause"
                    }
                    button { class: "btn small ghost", title: "Take it out of the queue", onclick: move |_| cancel_batch(state, id),
                        Icon { name: "x", size: 14 }
                    }
                } else if paused_batch {
                    button { class: "btn small", onclick: move |_| resume_batch(state, id),
                        Icon { name: "play", size: 14 } "Resume"
                    }
                    button { class: "btn small ghost", title: "Take it out of the queue", onclick: move |_| cancel_batch(state, id),
                        Icon { name: "x", size: 14 }
                    }
                } else {
                    div { class: "q-aside",
                        if failed > 0 {
                            button { class: "btn small", onclick: move |_| retry_batch(state, id),
                                Icon { name: "rotate-cw", size: 14 } "Try {plural(failed, \"chapter\")} again"
                            }
                        }
                        if batch.status == BatchStatus::Done {
                            button { class: "path-link", title: "Open the folder this series is kept in: {folder_full}",
                                onclick: move |_| {
                                    let dir = library_files().cover_path(&folder_id).parent().map(|p| p.to_path_buf());
                                    if let Some(dir) = dir {
                                        let _ = std::fs::create_dir_all(&dir);
                                        open_folder(&dir);
                                    }
                                },
                                span { class: "path-text", "{folder_text}" }
                                Icon { name: "external-link", size: 15 }
                            }
                        }
                        if downloaded > 0 {
                            button { class: "btn ghost open-big square danger", title: "Delete the downloaded chapters of this series",
                                onclick: move |_| state.ask(delete_modal.clone()),
                                Icon { name: "trash", size: 18 }
                            }
                        }
                        button { class: "btn ghost open-big square", title: "Take it off the list",
                            onclick: move |_| dismiss_batch(state, id),
                            Icon { name: "x", size: 18 }
                        }
                    }
                }
            }
        }
    }
}

/// The chapters being worked on, each with its own small bar. `limit` is how
/// many are shown (the others are counted).
#[component]
pub(super) fn ActiveChapters(progress: JobProgress, limit: Option<usize>) -> Element {
    let shown = limit.unwrap_or(12);
    let more = progress.active.len().saturating_sub(shown);
    rsx! {
        if !progress.active.is_empty() {
            div { class: "q-chapters",
                for (index, chapter) in progress.active.iter().take(shown) {
                    div { class: "q-chap", key: "{index}",
                        div { class: "q-chap-top",
                            span { class: "q-chap-name", title: "{chapter.name}", "{chapter.name}" }
                            span { class: "muted",
                                if chapter.pages_total > 0 { "{chapter.pages_done}/{chapter.pages_total}" } else { "starting" }
                            }
                        }
                        ProgressBar {
                            fraction: (chapter.pages_total > 0)
                                .then(|| chapter.pages_done as f32 / chapter.pages_total as f32),
                        }
                    }
                }
                if more > 0 {
                    div { class: "q-chap more muted", "and {more} more" }
                }
            }
        }
    }
}

/// Keep the way an export was converted as a preset, named after the series.
fn save_export_preset(state: AppState, title: &str, settings: Option<library::ConversionSettings>) {
    let Some(settings) = settings else {
        state.error("This export did not keep its settings.");
        return;
    };
    let mut last_error = String::new();
    for n in 1..=20 {
        let name = if n == 1 { title.to_string() } else { format!("{title} {n}") };
        match state.edit_presets(|p| p.save_new(&name, settings.clone())) {
            Ok(_) => {
                state.save_presets();
                state.notify(format!("Saved \"{name}\" to your presets."));
                return;
            }
            Err(e) => last_error = e.to_string(),
        }
    }
    state.error(last_error);
}
