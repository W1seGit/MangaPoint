//! Tab 1: every chapter of the series: pick the ones you want, and see which
//! are downloaded (and how big they are), on their way, or failed.

use crate::logic::{self, plural};
use crate::queue::QueuedChapter;
use crate::state::{AppState, ChaptersState, Modal, ModalAction, Page, PendingDownload};
use crate::ui::chapter_table::{ChapterGrid, GridRow};
use crate::ui::search::run_search;
use crate::ui::widgets::{Chip, EmptyState, Icon, Loading, ProgressBar};
use crate::ui::work::{ask_download, downloaded_keys, merged_key, open_export, retry_batch};
use dioxus::prelude::*;
use pipeline::SourceChoice;
use search::missing_numbers;

/// How far the download of this series has got. It is a part of its own, so
/// the progress coming in redraws only this, not the list of chapters.
#[component]
fn ChaptersProgress(series_id: String) -> Element {
    let state = use_context::<AppState>();
    let running = state
        .queue
        .read()
        .running()
        .filter(|b| b.series_id == series_id)
        .cloned();
    let Some(batch) = running else {
        return rsx! {};
    };
    rsx! {
        div { class: "chapters-progress",
            ProgressBar { fraction: Some(batch.progress.fraction()) }
            p { class: "muted small", style: "margin: 6px 0 0",
                "{batch.progress.label()}"
                if let Some(v) = batch.progress.pages_per_second() { " · about {v:.1} pages a second" }
                if let Some(n) = &batch.progress.note { " · {n}" }
            }
        }
    }
}

#[component]
pub fn ChaptersTab() -> Element {
    let state = use_context::<AppState>();
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    // Files imported have no source: only what is saved here, which can be edited.
    if state
        .library
        .read()
        .find(&ws.series_id)
        .is_some_and(|s| s.is_imported())
    {
        return rsx! { super::downloads::ImportedChapters {} };
    }
    let mut range_text = use_signal(String::new);
    let mut whole = use_signal(|| false);
    let mut range_error = use_signal(|| None::<String>);
    // What the list needs of the queue: only this series. A download of another
    // series does not draw this list again.
    let queue_series = ws.series_id.clone();
    let queue_view = use_memo(move || state.queue.read().view_of(&queue_series));
    // What is downloaded can be shown even when the sources cannot be reached.
    let has_downloads = state
        .library
        .read()
        .find(&ws.series_id)
        .is_some_and(|s| !s.chapters.is_empty());

    let loaded = match &*state.chapters.read() {
        ChaptersState::Idle => {
            let title = ws.title.clone();
            return rsx! {
                EmptyState {
                    title: "This series is in your Library",
                    message: "Its chapters come from your sources. Search for it to download more.",
                }
                div { style: "text-align: center",
                    button {
                        class: "btn primary",
                        onclick: move |_| {
                            run_search(state, title.clone());
                            state.go(Page::Search);
                        },
                        Icon { name: "search", size: 16 }
                        "Search for more chapters"
                    }
                }
                if has_downloads { super::downloads::DownloadsTab {} }
            };
        }
        ChaptersState::Loading => {
            // How many chapters there are, if the search or the library already
            // knows: a long series takes a while, and the page says so.
            let known = ws
                .ranked
                .as_ref()
                .and_then(|r| {
                    r.members
                        .iter()
                        .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.chapter_count))
                        .max()
                        .map(|n| n as usize)
                })
                .or_else(|| {
                    state
                        .library
                        .read()
                        .find(&ws.series_id)
                        .and_then(|s| s.chapter_total)
                });
            let note = crate::logic::long_series_note(known);
            return rsx! {
                div { class: "page-loading",
                    Loading { label: "Loading chapters from your sources…" }
                    if let Some(note) = note {
                        p { class: "muted small loading-note", "{note}" }
                    }
                }
            };
        }
        ChaptersState::Failed(message) => {
            let message = message.clone();
            return rsx! {
                div { class: "banner bad", Icon { name: "circle-x" } "{message}" }
                if has_downloads { super::downloads::DownloadsTab {} }
            };
        }
        ChaptersState::Ready(loaded) => loaded.clone(),
    };

    let have = downloaded_keys(state, &ws.series_id);
    let has_downloads = !have.is_empty();
    let series = state.library.read().find(&ws.series_id).cloned();
    let saved: std::collections::HashMap<String, (u64, String)> = series
        .iter()
        .flat_map(|s| s.chapters.iter())
        .map(|c| (c.key.clone(), (c.bytes, c.source_id.clone())))
        .collect();
    let saved_count = saved.len();
    let saved_bytes: u64 = saved.values().map(|(bytes, _)| *bytes).sum();
    // Where a downloaded copy came from, by name.
    let source_label = |source_id: &str| -> String {
        series
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .and_then(|i| i.source_name(source_id).map(str::to_string))
            .or_else(|| {
                loaded
                    .member_sources
                    .iter()
                    .position(|id| id == source_id)
                    .and_then(|i| loaded.member_names.get(i).cloned())
            })
            .unwrap_or_default()
    };
    let view = queue_view.read().clone();
    let working = view.working;
    let retry_ids = view.retry_ids.clone();
    let queued = |key: &str| !have.contains(key) && view.states.contains_key(key);
    let total = loaded.merged.len();
    let numbers: Vec<Option<f64>> = loaded.merged.iter().map(|m| m.number).collect();
    let missing = missing_numbers(&loaded.merged);
    let (filter, (sort_key, sort_desc)) = {
        let t = state.table.read();
        (t.filter.clone(), t.sort)
    };
    let order = logic::visible_chapters(&loaded.merged, &filter, sort_key, sort_desc);
    let selected_now = state.selected.read().clone();
    let picked = selected_now.len();
    let already = selected_now
        .iter()
        .filter(|&&i| {
            loaded
                .merged
                .get(i)
                .is_some_and(|c| have.contains(&merged_key(c)))
        })
        .count();
    let queued_picked = selected_now
        .iter()
        .filter(|&&i| loaded.merged.get(i).is_some_and(|c| queued(&merged_key(c))))
        .count();
    let downloaded_picked: Vec<String> = selected_now
        .iter()
        .filter_map(|&i| loaded.merged.get(i).map(merged_key))
        .filter(|k| have.contains(k))
        .collect();
    let delete_modal = Modal {
        title: format!("Delete {}?", plural(downloaded_picked.len(), "downloaded chapter")),
        body: format!(
            "The downloaded pages of the chapters you ticked in {} will be deleted from this computer. The series stays in your library and you can download them again. Files you already exported are kept.",
            ws.title
        ),
        confirm: "Delete".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteChapters {
            series_id: ws.series_id.clone(),
            keys: downloaded_picked,
        },
    };
    // Picked chapters that are neither downloaded nor already in the queue.
    let to_download = picked - already - queued_picked;
    // Which sources can supply the chapters still to download, and how many each.
    let mut per_source: Vec<(String, String, usize)> = Vec::new();
    for &i in selected_now.iter() {
        let Some(ch) = loaded.merged.get(i) else {
            continue;
        };
        if have.contains(&merged_key(ch)) || queued(&merged_key(ch)) {
            continue;
        }
        let mut seen: Vec<&str> = Vec::new();
        for option in &ch.options {
            let Some(id) = loaded.member_sources.get(option.member) else {
                continue;
            };
            if seen.contains(&id.as_str()) {
                continue;
            }
            seen.push(id);
            match per_source.iter_mut().find(|(source, _, _)| source == id) {
                Some((_, _, n)) => *n += 1,
                None => {
                    let name = loaded
                        .member_names
                        .get(option.member)
                        .cloned()
                        .unwrap_or_else(|| id.clone());
                    per_source.push((id.clone(), name, 1));
                }
            }
        }
    }
    per_source.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    let choice_id = match &*state.download_source.read() {
        SourceChoice::Only(id) if per_source.iter().any(|(source, _, _)| source == id) => {
            id.clone()
        }
        _ => "adaptive".to_string(),
    };
    let not_offered = per_source
        .iter()
        .find(|(source, _, _)| *source == choice_id)
        .map_or(0, |(_, _, n)| to_download - n);
    let source_options: Vec<(String, String)> = {
        let speeds = state.speeds.read();
        per_source
            .iter()
            .map(|(id, name, n)| {
                let speed = speeds
                    .speed(id)
                    .map_or(String::new(), |v| format!(" · about {v:.0} pages/s"));
                (id.clone(), format!("{name} · {n} of {to_download}{speed}"))
            })
            .collect()
    };
    let current_sort = (sort_key, sort_desc);
    let numbers_range = numbers.clone();
    let missing_text = {
        let first: Vec<String> = missing.iter().take(12).map(|n| n.to_string()).collect();
        format!(
            "{}{}",
            first.join(", "),
            if missing.len() > 12 { " …" } else { "" }
        )
    };

    // The chapters the reader has read to the end.
    let read_keys: std::collections::HashSet<String> = state
        .library
        .read()
        .reading_of(&ws.series_id)
        .map(|p| p.chapters.iter().filter(|(_, c)| c.read).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let grid_rows: Vec<GridRow> = order
        .iter()
        .map(|&i| {
            let ch = &loaded.merged[i];
            let key = merged_key(ch);
            let copy = saved.get(&key);
            let in_queue = if copy.is_none() {
                view.states.get(&key).cloned()
            } else {
                None
            };
            GridRow {
                index: i,
                number: ch
                    .number
                    .map(logic::format_number)
                    .unwrap_or_else(|| "–".to_string()),
                name: ch.name.clone(),
                source_names: ch
                    .options
                    .iter()
                    .filter_map(|o| loaded.member_names.get(o.member).cloned())
                    .filter(|name| !name.is_empty())
                    .collect(),
                date: logic::format_date(logic::latest_upload(ch)),
                saved: copy.is_some(),
                queued: matches!(in_queue, Some(QueuedChapter::Waiting)),
                progress: match in_queue {
                    Some(QueuedChapter::Downloading {
                        pages_done,
                        pages_total,
                    }) => Some((pages_done, pages_total)),
                    _ => None,
                },
                failed: matches!(in_queue, Some(QueuedChapter::Failed)),
                size: copy
                    .map(|(bytes, _)| logic::human_bytes(*bytes))
                    .unwrap_or_default(),
                from: copy.map(|(_, id)| source_label(id)).unwrap_or_default(),
                read_key: copy.map(|_| key.clone()),
                read: copy.is_some() && read_keys.contains(&key),
            }
        })
        .collect();

    let failed_count = grid_rows.iter().filter(|r| r.failed).count();
    let total_chapters = loaded.merged.len();
    let last_downloaded = series.as_ref().map_or(0, |s| s.updated_at);
    let mut selected = state.selected;
    let mut table = state.table;
    rsx! {
        div { class: "row wrap chapters-summary",
            div { class: "grow",
                if saved_count > 0 {
                    strong { "{saved_count} of {total_chapters} downloaded" }
                    span { class: "muted", " · {logic::human_bytes(saved_bytes)} on this computer" }
                    if last_downloaded > 0 {
                        span { class: "muted", " · last downloaded {logic::ago(crate::state::now_secs(), last_downloaded)}" }
                    }
                } else {
                    span { class: "muted", "Nothing downloaded yet. Tick chapters and add them to the downloads." }
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
        ChaptersProgress { series_id: ws.series_id.clone() }
        if loaded.unreachable > 0 {
            div { class: "banner warn small",
                Icon { name: "triangle-alert", size: 16 }
                "{loaded.unreachable} source(s) did not answer, so some chapters may be missing."
            }
        }
        div { class: "toolbar",
            div { class: "input-icon", style: "max-width: 280px",
                Icon { name: "list-filter", size: 16 }
                input {
                    r#type: "search", placeholder: "Filter chapters",
                    value: "{filter}",
                    oninput: move |e| table.write().filter = e.value(),
                }
            }
            form {
                class: "row", style: "gap: 6px; margin-left: auto",
                onsubmit: move |e| {
                    e.prevent_default();
                    match mangapoint_ui::logic::chapters::select_range(&range_text.read(), &numbers_range, whole()) {
                        Ok(found) => { range_error.set(None); selected.set(found.into_iter().collect()); }
                        Err(msg) => range_error.set(Some(msg)),
                    }
                },
                input {
                    r#type: "text", style: "width: 190px",
                    placeholder: "Range: 1-20, 25, 30-",
                    value: "{range_text}",
                    oninput: move |e| range_text.set(e.value()),
                }
                button { class: "btn small", r#type: "submit", "Select" }
                mangapoint_ui::widgets::WholeOnly { on: whole(), on_change: move |v| whole.set(v) }
            }
            if has_downloads {
                button { class: "btn primary small", title: "Edit and export the chapters you have downloaded",
                    onclick: {
                        let id = ws.series_id.clone();
                        move |_| open_export(state, vec![id.clone()])
                    },
                    Icon { name: "file-down", size: 16 } "Export…"
                }
            }
        }
        if let Some(msg) = range_error() {
            p { class: "small", style: "color: var(--bad); margin: 0 0 8px", "{msg}" }
        }
        ChapterGrid {
            rows: grid_rows,
            picked: selected_now.clone(),
            sort: current_sort,
            lock_done: false,
            on_read: {
                let series_id = ws.series_id.clone();
                move |key: String| state.open_reader(&series_id, Some(key))
            },
            on_sort: move |key| {
                let current = table.peek().sort;
                table.write().sort = logic::next_sort(current, key);
            },
            on_toggle: move |index: usize| {
                let mut s = selected.write();
                if !s.remove(&index) {
                    s.insert(index);
                }
            },
            on_set: move |(indexes, on): (Vec<usize>, bool)| {
                let mut s = selected.write();
                for i in indexes {
                    if on {
                        s.insert(i);
                    } else {
                        s.remove(&i);
                    }
                }
            },
            empty_message: format!("Nothing matches “{filter}”. Try fewer words."),
            max_height: None,
        }
        p { class: "muted small", style: "margin: 8px 0 0",
            "Showing {order.len()} of {total} chapters. Click a heading to sort, a row to tick it. Hover a source count to see which sources have the chapter."
            if !missing.is_empty() { " Not available from any source: {missing_text}." }
        }

        if picked > 0 {
            div { class: "action-bar",
                div { class: "bar-count",
                    strong { "{picked} selected" }
                    span { class: "muted small",
                        if already > 0 { "{already} downloaded" }
                        if already > 0 && queued_picked > 0 { " · " }
                        if queued_picked > 0 { "{queued_picked} in the queue" }
                    }
                }
                span { class: "grow" }
                if per_source.len() >= 2 && to_download > 0 {
                    div { class: "bar-source",
                        title: "Adaptive uses the source with the most of your chapters, tests which is fastest, and fills any gaps from the others. Pick one source to use only that one.",
                        Icon { name: "layers", size: 15 }
                        select {
                            value: "{choice_id}",
                            onchange: move |e| {
                                let mut source = state.download_source;
                                let value = e.value();
                                source.set(if value == "adaptive" { SourceChoice::Adaptive } else { SourceChoice::Only(value) });
                            },
                            option { value: "adaptive", selected: choice_id == "adaptive", "Adaptive (recommended)" }
                            for (id, label) in source_options.iter() {
                                option { key: "{id}", value: "{id}", selected: *id == choice_id, "{label}" }
                            }
                        }
                    }
                }
                if not_offered > 0 {
                    Chip { text: format!("{not_offered} not offered"), kind: Some("warn".to_string()) }
                }
                if already > 0 {
                    button { class: "btn danger", title: "Delete the downloaded pages of the chapters you ticked. The series stays in your library.",
                        onclick: move |_| state.ask(delete_modal.clone()),
                        Icon { name: "trash", size: 15 } "Delete {already} downloaded"
                    }
                }
                if to_download == 0 && queued_picked > 0 {
                    button { class: "btn primary", onclick: move |_| state.go(Page::Tasks),
                        Icon { name: "download", size: 16 } "View downloads"
                    }
                } else if to_download == 0 {
                    button { class: "btn primary",
                        onclick: {
                            let id = ws.series_id.clone();
                            move |_| open_export(state, vec![id.clone()])
                        },
                        Icon { name: "file-down", size: 16 } "Export them"
                    }
                } else {
                    button { class: "btn primary",
                        onclick: {
                            let summary = format!("{} of {}", plural(to_download - not_offered, "chapter"), ws.title);
                            move |_| ask_download(state, PendingDownload::Chapters, summary.clone())
                        },
                        Icon { name: "download", size: 16 } "Add {to_download - not_offered} to downloads"
                    }
                }
            }
        }
    }
}
