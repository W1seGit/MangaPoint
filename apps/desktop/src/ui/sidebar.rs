//! What is running, in the sidebar: the download in progress and the export,
//! each with the cover of its series and the chapters it is on. Nothing takes
//! room at the top of a page.

use super::tasks_page::ActiveChapters;
use super::widgets::{Cover, Icon, NoCover, ProgressBar};
use super::work::{
    cancel_job, load_saved_cover, pause_export, resume_export, saved_cover_key, set_queue_paused,
};
use crate::logic::{human_duration, plural};
use crate::state::{AppState, Page};
use dioxus::prelude::*;

#[component]
pub fn SidebarJobs() -> Element {
    rsx! {
        ExportCard {}
        DownloadCard {}
    }
}

/// The cover of a series, small, loaded once.
#[component]
fn JobCover(series_id: String) -> Element {
    let state = use_context::<AppState>();
    let has_cover = state
        .library
        .read()
        .find(&series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    {
        let id = series_id.clone();
        use_effect(move || load_saved_cover(state, &id));
    }
    rsx! {
        div { class: "side-job-cover",
            if has_cover { Cover { key_id: saved_cover_key(&series_id) } } else { NoCover {} }
        }
    }
}

/// What the card shows of the batch that is downloading.
struct RunningDownload {
    title: String,
    series_id: String,
    chapters_done: usize,
    chapter_count: usize,
    pages_per_second: Option<f32>,
    fraction: f32,
    progress: crate::logic::JobProgress,
}

/// The download in progress, or the queue when it is paused.
#[component]
fn DownloadCard() -> Element {
    let state = use_context::<AppState>();
    // Only what is drawn is taken from the queue: it holds every request and
    // plan of every batch, and this is drawn again for each progress tick.
    let (running, waiting, chapters_left, paused, first_active) = {
        let queue = state.queue.read();
        let (_, waiting, chapters_left) = queue.counts();
        (
            queue.running().map(|b| RunningDownload {
                title: b.title.clone(),
                series_id: b.series_id.clone(),
                chapters_done: b.chapters_done(),
                chapter_count: b.chapter_count(),
                pages_per_second: b.progress.pages_per_second(),
                fraction: b.progress.fraction(),
                progress: b.progress.clone(),
            }),
            waiting,
            chapters_left,
            queue.paused,
            queue
                .batches
                .iter()
                .find(|b| b.status.is_active())
                .map(|b| b.series_id.clone()),
        )
    };
    if running.is_none() && !(paused && waiting > 0) {
        return rsx! {};
    }
    let (toggle_icon, toggle_title) = if paused {
        ("play", "Carry on with the downloads")
    } else {
        ("pause", "Pause the downloads")
    };
    // The series on the cover: the one running, or the next in line.
    let cover_of = running
        .as_ref()
        .map(|b| b.series_id.clone())
        .or(first_active)
        .unwrap_or_default();
    rsx! {
        div { class: "side-job", onclick: move |_| state.go(Page::Tasks),
            div { class: "side-job-top",
                for key in [cover_of.clone()] {
                    JobCover { key: "{key}", series_id: key.clone() }
                }
                div { class: "side-job-info",
                    div { class: "side-job-head",
                        Icon { name: "download", size: 14 }
                        if let Some(batch) = &running {
                            span { class: "side-job-title", title: "{batch.title}", "{batch.title}" }
                        } else {
                            span { class: "side-job-title", "Downloads paused" }
                        }
                        button { class: "side-job-btn", title: "{toggle_title}",
                            onclick: move |e| {
                                e.stop_propagation();
                                set_queue_paused(state, !paused);
                            },
                            Icon { name: toggle_icon, size: 13 }
                        }
                    }
                    if let Some(batch) = &running {
                        div { class: "side-job-meta",
                            span { "{batch.chapters_done} of {plural(batch.chapter_count, \"chapter\")}" }
                            if let Some(v) = batch.pages_per_second { span { "{v:.1} pages/s" } }
                        }
                    } else {
                        div { class: "side-job-meta", "{plural(chapters_left, \"chapter\")} waiting" }
                    }
                }
            }
            if let Some(batch) = &running {
                ProgressBar { fraction: Some(batch.fraction) }
                ActiveChapters { progress: batch.progress.clone(), limit: 3 }
                if waiting > 0 { div { class: "side-job-meta", "{waiting} more in line" } }
            }
        }
    }
}

/// The export that is running, paused, or has just ended.
#[component]
fn ExportCard() -> Element {
    let state = use_context::<AppState>();
    let Some(job) = state.job.read().clone() else {
        return rsx! {};
    };
    let ended = job.result.is_some();
    let running = !ended && !job.paused;
    let (meta_class, meta) = match &job.result {
        Some(Ok(text)) => ("side-job-meta", text.clone()),
        Some(Err(text)) => ("side-job-meta bad", text.clone()),
        None => ("side-job-meta", String::new()),
    };
    let left = job.progress.eta_secs().map(human_duration);
    let series_id = job.series_id.clone();
    rsx! {
        div { class: "side-job", onclick: move |_| state.go(Page::Tasks),
            div { class: "side-job-top",
                for key in [series_id.clone()] {
                    JobCover { key: "{key}", series_id: key.clone() }
                }
                div { class: "side-job-info",
                    div { class: "side-job-head",
                        Icon { name: "file-down", size: 14 }
                        span { class: "side-job-title", title: "{job.title}", "{job.title}" }
                        if ended {
                            button { class: "side-job-btn", title: "Dismiss",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    let mut j = state.job;
                                    j.set(None);
                                },
                                Icon { name: "x", size: 13 }
                            }
                        } else if job.paused {
                            button { class: "side-job-btn", title: "Carry on with the export",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    resume_export(state);
                                },
                                Icon { name: "play", size: 13 }
                            }
                            button { class: "side-job-btn", title: "Stop it for good",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    cancel_job(state);
                                },
                                Icon { name: "x", size: 13 }
                            }
                        } else {
                            button { class: "side-job-btn", title: "Pause the export", disabled: job.progress.stopping,
                                onclick: move |e| {
                                    e.stop_propagation();
                                    pause_export(state);
                                },
                                Icon { name: "pause", size: 13 }
                            }
                            button { class: "side-job-btn", title: "Stop it for good", disabled: job.progress.stopping,
                                onclick: move |e| {
                                    e.stop_propagation();
                                    cancel_job(state);
                                },
                                Icon { name: "x", size: 13 }
                            }
                        }
                    }
                    if ended {
                        div { class: "{meta_class}", "{meta}" }
                    } else if job.paused {
                        div { class: "side-job-meta",
                            if job.progress.chapter_total > 0 {
                                "Paused · {job.progress.finished} of {plural(job.progress.chapter_total, \"chapter\")}"
                            } else {
                                "Paused"
                            }
                        }
                    } else {
                        div { class: "side-job-meta",
                            span { "{job.progress.finished} of {plural(job.progress.chapter_total, \"chapter\")}" }
                            if let Some(left) = left { span { "{left} left" } }
                        }
                    }
                }
            }
            if !ended {
                ProgressBar { fraction: Some(job.progress.fraction()) }
                if running { ActiveChapters { progress: job.progress.clone(), limit: 3 } }
            }
        }
    }
}
