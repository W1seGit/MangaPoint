//! Tab 2: what is downloaded, and what is on its way for this series. The
//! whole queue, for every series, is on the Downloads page.

use crate::logic::{self, plural, ChapterStatus};
use crate::queue::{BatchStatus, QueuedChapter};
use crate::state::{AppState, Modal, ModalAction, Page};
use crate::ui::widgets::{Chip, EmptyState, Icon, ProgressBar};
use crate::ui::work::{open_export, retry_batch, start_import};
use dioxus::prelude::*;
use library::PageQuality;
use std::collections::HashSet;

/// The chapters of an imported series: the list of what is saved, which can be
/// edited, with a button to import more pages into the series.
#[component]
pub fn ImportedChapters() -> Element {
    let state = use_context::<AppState>();
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let id = ws.series_id.clone();
    rsx! {
        div { class: "row wrap", style: "margin-bottom: 12px",
            div { class: "grow",
                span { class: "muted", "Edit a number or a name right in the list." }
            }
            button { class: "btn", title: "Add more pages or chapters to this series",
                onclick: move |_| start_import(state, Some(id.clone()), false),
                Icon { name: "file-up", size: 16 } "Import pages"
            }
        }
        DownloadsTab {}
    }
}

/// A chapter shown in the table: saved already, or on its way, or failed.
struct Row {
    key: String,
    number: Option<f64>,
    name: String,
    pages: Option<usize>,
    bytes: Option<u64>,
}

#[component]
pub fn DownloadsTab() -> Element {
    let state = use_context::<AppState>();
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let queue = state.queue.read();

    // Saved chapters, then the others that are in the queue for this series.
    let mut rows: Vec<Row> = state
        .library
        .read()
        .find(&ws.series_id)
        .map(|s| {
            s.chapters
                .iter()
                .map(|c| Row {
                    key: c.key.clone(),
                    number: c.number,
                    name: c.name.clone(),
                    pages: Some(c.pages),
                    bytes: Some(c.bytes),
                })
                .collect()
        })
        .unwrap_or_default();
    let saved_keys: HashSet<String> = rows.iter().map(|r| r.key.clone()).collect();
    let saved_count = rows.len();
    let saved_bytes: u64 = rows.iter().filter_map(|r| r.bytes).sum();
    let mut seen = saved_keys.clone();
    for batch in queue.batches.iter().filter(|b| b.series_id == ws.series_id) {
        for (request, key) in batch.requests.iter().zip(batch.plan.iter()) {
            if seen.insert(key.clone()) && queue.chapter_state(&ws.series_id, key).is_some() {
                rows.push(Row {
                    key: key.clone(),
                    number: request.number,
                    name: request.name.clone(),
                    pages: None,
                    bytes: None,
                });
            }
        }
    }
    rows.sort_by(|a, b| {
        a.number
            .unwrap_or(f64::MAX)
            .total_cmp(&b.number.unwrap_or(f64::MAX))
            .then_with(|| a.name.cmp(&b.name))
    });

    let working = queue.series_has_work(&ws.series_id);
    let on_the_way = rows
        .iter()
        .filter(|r| {
            !saved_keys.contains(&r.key)
                && matches!(
                    queue.chapter_state(&ws.series_id, &r.key),
                    Some(QueuedChapter::Waiting | QueuedChapter::Downloading { .. })
                )
        })
        .count();
    let failed_count = rows
        .iter()
        .filter(|r| {
            !saved_keys.contains(&r.key)
                && queue.chapter_state(&ws.series_id, &r.key) == Some(QueuedChapter::Failed)
        })
        .count();
    let running_here = queue
        .running()
        .filter(|b| b.series_id == ws.series_id)
        .cloned();
    // The finished batches of this series that left chapters undone.
    let retry_ids: Vec<u64> = queue
        .batches
        .iter()
        .filter(|b| b.series_id == ws.series_id && !b.status.is_active() && !b.failed.is_empty())
        .map(|b| b.id)
        .collect();
    let notes: Vec<String> = queue
        .batches
        .iter()
        .rev()
        .filter(|b| b.series_id == ws.series_id && matches!(b.status, BatchStatus::Done))
        .flat_map(|b| b.skipped.iter().cloned())
        .take(3)
        .collect();
    let quality = state.settings.read().page_quality;

    let imported = state
        .library
        .read()
        .find(&ws.series_id)
        .is_some_and(|s| s.is_imported());
    if rows.is_empty() {
        let (title, message) = if imported {
            ("No chapters left", "Use Import pages to add chapters to this series.")
        } else {
            ("Nothing downloaded yet", "Tick some chapters in the list and add them to the downloads.")
        };
        return rsx! {
            EmptyState { title, message }
        };
    }

    let table_rows: Vec<Element> = rows
        .iter()
        .map(|r| {
            let status = if saved_keys.contains(&r.key) {
                ChapterStatus::Saved
            } else {
                match queue.chapter_state(&ws.series_id, &r.key) {
                    Some(QueuedChapter::Downloading { .. }) => ChapterStatus::Downloading,
                    Some(QueuedChapter::Failed) => ChapterStatus::Failed,
                    _ => ChapterStatus::Waiting,
                }
            };
            let progress = match queue.chapter_state(&ws.series_id, &r.key) {
                Some(QueuedChapter::Downloading {
                    pages_done,
                    pages_total,
                }) => Some((pages_done, pages_total)),
                _ => None,
            };
            rsx! {
                DownloadRow {
                    key: "{r.key}",
                    editable: imported,
                    raw_number: r.number.map(pipeline::import::plain).unwrap_or_default(),
                    chapter_key: r.key.clone(),
                    series_id: ws.series_id.clone(),
                    title: ws.title.clone(),
                    number: r.number.map(logic::format_number).unwrap_or_else(|| "–".to_string()),
                    name: r.name.clone(),
                    status,
                    progress,
                    pages: r.pages.map(|p| p.to_string()).unwrap_or_default(),
                    size: r.bytes.map(logic::human_bytes).unwrap_or_default(),
                }
            }
        })
        .collect();

    rsx! {
        div { class: "row wrap",
            div { class: "grow",
                if imported {
                    strong { "{plural(saved_count, \"chapter\")}" }
                    span { class: "muted", " · {logic::human_bytes(saved_bytes)}" }
                } else {
                    strong { "{saved_count} downloaded" }
                    span { class: "muted", " · {logic::human_bytes(saved_bytes)} on this computer" }
                }
                if on_the_way > 0 { Chip { text: format!("{on_the_way} on the way"), kind: Some("accent".to_string()) } }
            }
            if saved_count > 0 {
                button { class: "btn primary", onclick: { let id = ws.series_id.clone(); move |_| open_export(state, vec![id.clone()]) },
                    Icon { name: "file-down", size: 16 } "Export…"
                }
            }
            if working || failed_count > 0 {
                button { class: "btn", onclick: move |_| state.go(Page::Tasks),
                    Icon { name: "download", size: 16 } "All downloads"
                }
            }
            if failed_count > 0 {
                button { class: "btn", onclick: move |_| { for id in retry_ids.iter().copied() { retry_batch(state, id); } },
                    Icon { name: "rotate-cw", size: 16 } "Try {plural(failed_count, \"chapter\")} again"
                }
            }
        }
        if let Some(batch) = running_here {
            div { style: "margin-top: 12px",
                ProgressBar { fraction: Some(batch.progress.fraction()) }
                p { class: "muted small", style: "margin: 6px 0 0",
                    "{batch.progress.label()}"
                    if let Some(v) = batch.progress.pages_per_second() { " · about {v:.1} pages a second" }
                    if let Some(n) = &batch.progress.note { " · {n}" }
                }
            }
        }
        if failed_count > 0 {
            div { class: "banner warn small", style: "margin-bottom: 0",
                Icon { name: "triangle-alert", size: 16 }
                div {
                    "{plural(failed_count, \"chapter\")} could not be downloaded from any source."
                    for line in notes.iter() { div { class: "muted", key: "{line}", "{line}" } }
                }
            }
        }
        div { class: "table-wrap", style: "margin-top: 14px",
            div { class: "table-scroll",
                table { class: "chapters",
                    thead { tr {
                        th { class: "num", "No." }
                        th { "Title" }
                        th { class: "status", "Status" }
                        th { class: "date", "Pages" }
                        th { class: "date", "Size" }
                        th { class: "check" }
                    } }
                    tbody { {table_rows.into_iter()} }
                }
            }
        }
        if !imported { p { class: "muted small", style: "margin: 8px 0 0",
            match quality {
                PageQuality::Ultra => "Pages are saved as small as possible (ultra). Change it in Settings → Downloads.",
                PageQuality::Save => "Tall pages are shrunk as they download to save space. Change it in Settings → Downloads.",
                PageQuality::Full => "Pages are saved exactly as downloaded, for the best quality. Change it in Settings → Downloads.",
            }
        } }
    }
}

/// One row of the table. It is its own component, with plain values as its
/// inputs, so a progress update redraws only the rows whose own values changed
/// instead of the whole table.
#[component]
fn DownloadRow(
    /// The number and name can be typed over (an imported series).
    editable: bool,
    /// The number as it is typed, empty when there is none.
    raw_number: String,
    chapter_key: String,
    series_id: String,
    title: String,
    number: String,
    name: String,
    status: ChapterStatus,
    /// Pages done and in all, while this chapter is downloading.
    progress: Option<(usize, usize)>,
    pages: String,
    size: String,
) -> Element {
    let state = use_context::<AppState>();
    let chip = match status {
        ChapterStatus::Saved if editable => rsx! { Chip { text: "imported", kind: Some("accent".to_string()) } },
        ChapterStatus::Saved => rsx! { Chip { text: "downloaded", kind: Some("ok".to_string()) } },
        ChapterStatus::Downloading => {
            let text = match progress {
                Some((done, total)) if total > 0 => format!("{done} / {total} pages"),
                _ => "starting…".to_string(),
            };
            rsx! { Chip { text, kind: Some("accent".to_string()) } }
        }
        ChapterStatus::Waiting => rsx! { Chip { text: "waiting" } },
        ChapterStatus::Failed => {
            rsx! { Chip { text: "couldn't download", kind: Some("bad".to_string()) } }
        }
    };
    let delete_name = name.clone();
    // Keep what is typed in the other box when one is changed.
    let (number_series, number_key, number_name) = (series_id.clone(), chapter_key.clone(), name.clone());
    let (name_series, name_key, name_number) = (series_id.clone(), chapter_key.clone(), raw_number.clone());
    let save = move |series: String, key: String, number: String, name: String| {
        let value = number
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0);
        let mut library = state.library;
        library.write().set_chapter_details(&series, &key, value, name.trim());
        state.save_library();
    };
    let save_number = save;
    rsx! {
        tr {
            td { class: "num",
                if editable {
                    input {
                        class: "chapter-edit num", r#type: "text", value: "{raw_number}", placeholder: "No.",
                        "aria-label": "Chapter number", spellcheck: "false",
                        onchange: move |e| save_number(number_series.clone(), number_key.clone(), e.value(), number_name.clone()),
                    }
                } else { "{number}" }
            }
            td { class: "name", title: "{name}",
                if editable {
                    input {
                        class: "chapter-edit", r#type: "text", value: "{name}", "aria-label": "Chapter name",
                        onchange: move |e| save(name_series.clone(), name_key.clone(), name_number.clone(), e.value()),
                    }
                } else { "{name}" }
            }
            td { class: "status", {chip} }
            td { class: "date", "{pages}" }
            td { class: "date", "{size}" }
            td { class: "check",
                if status == ChapterStatus::Saved {
                    button {
                        class: "btn ghost small", title: "Delete the downloaded pages of this chapter",
                        onclick: move |_| state.ask(Modal {
                            title: "Delete this chapter?".into(),
                            body: format!("The downloaded pages of {delete_name} in {title} will be deleted. Files you already exported are kept."),
                            confirm: "Delete".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::DeleteChapters { series_id: series_id.clone(), keys: vec![chapter_key.clone()] },
                        }),
                        Icon { name: "trash", size: 14 }
                    }
                }
            }
        }
    }
}
