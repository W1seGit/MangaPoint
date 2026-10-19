//! Choosing what to download from one or more library series, before it goes
//! to the download queue. Each series has its own chapter table, with a
//! filter, sorting and tick boxes.

use super::chapter_table::{ChapterGrid, GridRow};
use super::widgets::{Busy, Cover, EmptyState, Icon, NoCover};
use super::work::{ask_download, downloaded_keys, load_saved_cover, merged_key, saved_cover_key};
use crate::logic::{self, plural};
use crate::state::{AppState, Page, PendingDownload, PlanChapters, PlanEntry};
use dioxus::prelude::*;

/// Change one series' entry in the planner.
fn with_entry(state: AppState, series_id: &str, change: impl FnOnce(&mut PlanEntry)) {
    let mut plan = state.plan;
    let mut entries = plan.write();
    if let Some(entry) = entries.iter_mut().find(|e| e.series_id == series_id) {
        change(entry);
    }
}

fn leave(state: AppState) {
    let mut plan = state.plan;
    plan.set(Vec::new());
    state.go(Page::Library);
}

#[component]
pub fn PlanPage() -> Element {
    let state = use_context::<AppState>();
    let entries = state.plan.read().clone();
    let ids: Vec<String> = entries.iter().map(|e| e.series_id.clone()).collect();
    {
        let ids = ids.clone();
        use_effect(move || {
            for id in &ids {
                load_saved_cover(state, id);
            }
        });
    }
    let chapters: usize = entries.iter().map(|e| e.picked.len()).sum();
    let with_picks = entries.iter().filter(|e| !e.picked.is_empty()).count();
    let loading = entries
        .iter()
        .filter(|e| matches!(e.chapters, PlanChapters::Loading))
        .count();

    if entries.is_empty() {
        return rsx! {
            EmptyState {
                title: "Nothing to plan",
                message: "Select series in your library and choose Download.",
                shelf: true,
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.go(Page::Library), "Back to the library" }
            }
        };
    }

    rsx! {
        button { class: "btn ghost small back", onclick: move |_| leave(state),
            Icon { name: "arrow-left", size: 14 } "Library"
        }
        h1 { "Choose what to download" }
        p { class: "muted",
            "Tick the chapters you want from each series. They go to the download queue and download one series after another."
        }
        for id in ids.iter() {
            PlanSection { key: "{id}", series_id: id.clone() }
        }
        div { class: "action-bar",
            div { class: "bar-count",
                strong { "{plural(chapters, \"chapter\")} chosen" }
                span { class: "muted small",
                    if with_picks > 0 { "from {with_picks} of {entries.len()} series" }
                    if with_picks == 0 && loading > 0 { "Loading the chapter lists…" }
                    if with_picks == 0 && loading == 0 { "Tick some chapters" }
                }
            }
            span { class: "grow" }
            button { class: "btn", onclick: move |_| leave(state), "Cancel" }
            button { class: "btn primary", disabled: chapters == 0, onclick: move |_| ask_download(
                    state,
                    PendingDownload::Plan,
                    format!("{} from {}", plural(chapters, "chapter"), plural(with_picks, "series")),
                ),
                Icon { name: "download", size: 16 } "Add to downloads"
            }
        }
    }
}

#[component]
fn PlanSection(series_id: String) -> Element {
    let state = use_context::<AppState>();
    let Some(entry) = state
        .plan
        .read()
        .iter()
        .find(|e| e.series_id == series_id)
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
    let have = downloaded_keys(state, &series_id);
    let series_chapters = state
        .library
        .read()
        .find(&series_id)
        .map(|s| s.chapters.clone())
        .unwrap_or_default();
    let queue = state.queue.read();

    let loaded = match &entry.chapters {
        PlanChapters::Ready(loaded) => Some(loaded.clone()),
        _ => None,
    };
    let is_locked =
        |key: &str| have.contains(key) || queue.chapter_state(&series_id, key).is_some();

    // Everything that can still be picked, oldest to newest.
    let open_positions: Vec<usize> = loaded
        .as_ref()
        .map(|l| {
            l.merged
                .iter()
                .enumerate()
                .filter(|(_, c)| !is_locked(&merged_key(c)))
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default();
    let new_count = open_positions.len();
    let total = loaded.as_ref().map_or(0, |l| l.merged.len());
    let picked = entry.picked.len();

    // The rows the table shows: filtered and sorted.
    let rows: Vec<GridRow> = match &loaded {
        Some(l) if entry.open => {
            logic::visible_chapters(&l.merged, &entry.filter, entry.sort.0, entry.sort.1)
                .into_iter()
                .map(|i| {
                    let c = &l.merged[i];
                    let key = merged_key(c);
                    let saved = series_chapters.iter().find(|c| c.key == key);
                    GridRow {
                        index: i,
                        number: c
                            .number
                            .map(logic::format_number)
                            .unwrap_or_else(|| "–".into()),
                        name: c.name.clone(),
                        source_names: c
                            .options
                            .iter()
                            .filter_map(|o| l.member_names.get(o.member).cloned())
                            .collect(),
                        date: logic::format_date(logic::latest_upload(c)),
                        saved: have.contains(&key),
                        queued: !have.contains(&key)
                            && queue.chapter_state(&series_id, &key).is_some(),
                        size: saved
                            .map(|c| logic::human_bytes(c.bytes))
                            .unwrap_or_default(),
                        ..Default::default()
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    };

    let latest: Vec<usize> = open_positions.iter().rev().take(10).copied().collect();
    let all_new = open_positions.clone();
    let (id_a, id_b, id_c, id_d, id_e, id_f, id_g, id_h) = (
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
        series_id.clone(),
    );
    let toggle_title = if entry.open {
        "Hide the chapters"
    } else {
        "Show the chapters"
    };
    let toggle_icon = if entry.open {
        "chevron-up"
    } else {
        "chevron-down"
    };
    let filter_text = entry.filter.clone();
    let empty_message = format!("Nothing matches “{}”. Try fewer words.", entry.filter);

    rsx! {
        div { class: "plan-section",
            div { class: "plan-head",
                div { class: "plan-cover",
                    if has_cover { Cover { key_id: saved_cover_key(&series_id) } } else { NoCover {} }
                }
                div { class: "grow",
                    strong { class: "q-name", "{entry.title}" }
                    div { class: "muted small",
                        match &entry.chapters {
                            PlanChapters::Loading => rsx! { Busy { label: "Loading chapters…", size: 28 } },
                            PlanChapters::Failed(message) => rsx! { "{message}" },
                            PlanChapters::Ready(_) => rsx! {
                                "{plural(new_count, \"new chapter\")} of {total}"
                                if picked > 0 { " · {picked} chosen" }
                            },
                        }
                    }
                }
                if loaded.is_some() {
                    button {
                        class: "btn small ghost", title: toggle_title,
                        onclick: move |_| with_entry(state, &id_a, |e| e.open = !e.open),
                        Icon { name: toggle_icon, size: 16 }
                    }
                }
            }
            if entry.open && loaded.is_some() {
                div { class: "toolbar",
                    div { class: "input-icon", style: "max-width: 260px",
                        Icon { name: "list-filter", size: 16 }
                        input {
                            r#type: "search", placeholder: "Filter chapters", value: "{filter_text}",
                            oninput: move |e| with_entry(state, &id_b, |x| x.filter = e.value()),
                        }
                    }
                    if new_count > 0 {
                        button { class: "btn small", onclick: move |_| {
                                let all = all_new.clone();
                                with_entry(state, &id_c, |e| e.picked = all.into_iter().collect());
                            },
                            "All new"
                        }
                        button { class: "btn small", onclick: move |_| {
                                let some = latest.clone();
                                with_entry(state, &id_d, |e| e.picked = some.into_iter().collect());
                            },
                            "Latest 10"
                        }
                        button { class: "btn small", onclick: move |_| with_entry(state, &id_e, |e| e.picked.clear()), "None" }
                    }
                }
                ChapterGrid {
                    rows,
                    picked: entry.picked.clone(),
                    sort: entry.sort,
                    lock_done: true,
                    on_sort: move |key| with_entry(state, &id_f, |e| e.sort = logic::next_sort(e.sort, key)),
                    on_toggle: move |index: usize| with_entry(state, &id_g, |e| {
                        if !e.picked.remove(&index) {
                            e.picked.insert(index);
                        }
                    }),
                    on_set: move |(indexes, on): (Vec<usize>, bool)| with_entry(state, &id_h, |e| {
                        for i in indexes {
                            if on {
                                e.picked.insert(i);
                            } else {
                                e.picked.remove(&i);
                            }
                        }
                    }),
                    empty_message,
                    max_height: Some("max(280px, calc(100vh - 420px))".to_string()),
                }
            }
        }
    }
}
