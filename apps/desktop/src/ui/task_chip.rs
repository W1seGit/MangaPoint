//! The running task, shown to the left of the Tasks and Settings icons: what it is, which series,
//! how far it is. Pressing it opens the Tasks page.

use crate::state::{AppState, Page};
use dioxus::prelude::*;

#[component]
pub fn TaskChip(on_open: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    if *state.page.read() == Page::Tasks {
        return rsx! {};
    }
    let shown = if let Some(job) = state.job.read().as_ref().filter(|j| j.result.is_none()) {
        let verb = if job.paused { "Paused" } else if job.progress.writing.is_some() { "Writing" } else { "Exporting" };
        let detail = if job.progress.chapter_total > 0 {
            format!("{} of {}", job.progress.finished, job.progress.chapter_total)
        } else {
            String::new()
        };
        Some((verb, job.title.clone(), detail, job.progress.fraction(), job.paused))
    } else {
        state.queue.read().running().map(|b| {
            let detail = format!("{} of {}", b.chapters_done(), b.chapter_count());
            ("Downloading", b.title.clone(), detail, b.progress.fraction(), false)
        })
    };
    let Some((verb, title, detail, fraction, paused)) = shown else {
        return rsx! {};
    };
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round();
    rsx! {
        button { class: if paused { "task-chip paused" } else { "task-chip" }, id: "task-chip", title: "Open Tasks", onclick: move |_| on_open.call(()),
            span { class: "task-chip-text",
                span { class: "task-chip-name", "{title}" }
                span { class: "task-chip-sub", "{verb}" if !detail.is_empty() { " · {detail}" } }
            }
            span { class: "task-chip-bar", span { class: "task-chip-fill", style: "width: {percent}%" } }
        }
    }
}
