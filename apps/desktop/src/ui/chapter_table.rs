//! The chapter table, shared by the Chapters tab and the download planner:
//! tick boxes with a select-all in the header, sortable columns, and for each
//! chapter whether it is downloaded (and how big it is). It only draws what it
//! is given; whoever uses it decides what ticking and sorting do.

use super::widgets::{Chip, EmptyState, Icon};
use crate::logic::SortKey;
use dioxus::prelude::*;
use std::collections::HashSet;

/// One chapter as the table shows it.
#[derive(Clone, Default, PartialEq)]
pub struct GridRow {
    /// Its position in the merged chapter list; what the handlers are given.
    pub index: usize,
    pub number: String,
    pub name: String,
    /// The names of the sources that have it; shown when hovering the count.
    pub source_names: Vec<String>,
    pub date: String,
    pub saved: bool,
    pub queued: bool,
    /// Pages done and in all, while it downloads.
    pub progress: Option<(usize, usize)>,
    pub failed: bool,
    /// Its size on this computer; empty until it is downloaded.
    pub size: String,
    /// The source the downloaded copy came from.
    pub from: String,
    /// The key of the downloaded chapter, when it can be read.
    pub read_key: Option<String>,
    /// The reader has read it to the end.
    pub read: bool,
}

impl GridRow {
    /// Downloaded, in the queue or on its way: nothing left to download.
    fn is_settled(&self) -> bool {
        self.saved || self.queued || self.progress.is_some()
    }
}

/// How tall a row is, in pixels (the stylesheet fixes it to this).
const ROW_HEIGHT: usize = 42;
/// How many rows are drawn at once in a long table; the rest are only space.
const DRAWN_ROWS: usize = 60;
/// Scrolling moves the drawn rows in steps of this many.
const STEP: usize = 10;

fn sort_icon(current: (SortKey, bool), column: SortKey) -> &'static str {
    if current.0 != column {
        "chevrons-up-down"
    } else if current.1 {
        "chevron-down"
    } else {
        "chevron-up"
    }
}

/// The table. `rows` are already filtered and in the order to show.
/// With `lock_done`, chapters that are downloaded or queued cannot be ticked.
#[component]
pub fn ChapterGrid(
    rows: Vec<GridRow>,
    picked: HashSet<usize>,
    sort: (SortKey, bool),
    lock_done: bool,
    /// A column heading was clicked.
    on_sort: EventHandler<SortKey>,
    /// One row was clicked.
    on_toggle: EventHandler<usize>,
    /// The header box: tick (`true`) or clear (`false`) all these rows.
    on_set: EventHandler<(Vec<usize>, bool)>,
    /// The Read button of a downloaded chapter was pressed (given its key).
    /// Without it the table has no Read buttons.
    on_read: Option<EventHandler<String>>,
    /// What to show when there are no rows.
    empty_message: String,
    /// Tallest the table gets before it scrolls, for example "360px".
    max_height: Option<String>,
) -> Element {
    // A long list is not drawn whole: only the rows in view (and a few more
    // each way) are, with empty space where the others would be, so the page
    // stays light however many chapters there are.
    let mut block = use_signal(|| 0usize);
    let total = rows.len();
    let long = total > DRAWN_ROWS + 2 * STEP;
    let (first, last) = if long {
        let first = (block() * STEP).saturating_sub(STEP).min(total - DRAWN_ROWS);
        (first, first + DRAWN_ROWS)
    } else {
        (0, total)
    };
    let tickable: Vec<usize> = rows
        .iter()
        .filter(|r| !(lock_done && r.is_settled()))
        .map(|r| r.index)
        .collect();
    let all_on = !tickable.is_empty() && tickable.iter().all(|i| picked.contains(i));
    let none_to_tick = tickable.is_empty();
    let scroll_style = max_height
        .map(|h| format!("max-height: {h}"))
        .unwrap_or_default();

    let header = |key: SortKey, class: &'static str, label: &'static str| {
        let sorted = sort.0 == key;
        rsx! {
            th {
                class: if sorted { format!("{class} sortable sorted") } else { format!("{class} sortable") },
                onclick: move |_| on_sort.call(key),
                span { class: "th", "{label}" Icon { name: sort_icon(sort, key), size: 14 } }
            }
        }
    };

    rsx! {
        div { class: "table-wrap",
            div { class: "table-scroll", style: "{scroll_style}",
                onscroll: move |e| {
                    let now = e.data().scroll_top() as usize / ROW_HEIGHT / STEP;
                    if now != *block.peek() {
                        block.set(now);
                    }
                },
                table { class: "chapters",
                    thead {
                        tr {
                            th { class: "check",
                                input {
                                    r#type: "checkbox", checked: all_on, disabled: none_to_tick,
                                    title: "Tick or clear everything shown",
                                    onclick: move |_| on_set.call((tickable.clone(), !all_on)),
                                }
                            }
                            {header(SortKey::Number, "num", "No.")}
                            {header(SortKey::Name, "", "Title")}
                            th { class: "status", "Status" }
                            th { class: "size", "Size" }
                            {header(SortKey::Sources, "src", "Sources")}
                            {header(SortKey::Date, "date", "Updated")}
                        }
                    }
                    tbody {
                        if first > 0 {
                            tr { class: "spacer", style: "height: {first * ROW_HEIGHT}px", td { colspan: "7" } }
                        }
                        for r in rows[first..last].iter().cloned() {
                            ChapterRow {
                                key: "{r.index}",
                                on: picked.contains(&r.index),
                                locked: lock_done && r.is_settled(),
                                row: r,
                                on_toggle,
                                on_read,
                            }
                        }
                        if last < total {
                            tr { class: "spacer", style: "height: {(total - last) * ROW_HEIGHT}px", td { colspan: "7" } }
                        }
                    }
                }
                if rows.is_empty() {
                    EmptyState { title: "No chapters match", message: empty_message, compact: true }
                }
            }
        }
    }
}

/// One row, a component of its own so that ticking a chapter redraws that
/// row and not the whole table.
#[component]
fn ChapterRow(
    row: GridRow,
    on: bool,
    locked: bool,
    on_toggle: EventHandler<usize>,
    on_read: Option<EventHandler<String>>,
) -> Element {
    let index = row.index;
    let class = match (on, locked) {
        (_, true) => "locked",
        (true, _) => "on",
        _ => "",
    };
    let sources_tip = row.source_names.join("\n");
    let from_tip = if row.from.is_empty() {
        "Downloaded".to_string()
    } else {
        format!("Downloaded from {}", row.from)
    };
    rsx! {
        tr {
            class: "{class}",
            onclick: move |_| if !locked { on_toggle.call(index) },
            td { class: "check", input { r#type: "checkbox", checked: on, disabled: locked, readonly: true, style: "pointer-events: none" } }
            td { class: "num", "{row.number}" }
            td { class: "name", title: "{row.name}", "{row.name}" }
            td { class: "status",
                if row.saved {
                    span { class: "tip", "data-tip": "{from_tip}", Chip { text: "Downloaded", kind: Some("ok".to_string()) } }
                    if row.read {
                        span { class: "tip", "data-tip": "You have read this chapter", Chip { text: "Read" } }
                    }
                    if let (Some(read), Some(key)) = (on_read, row.read_key.clone()) {
                        button {
                            class: "btn small chapter-read", title: "Read this chapter",
                            "aria-label": "Read chapter {row.number}", "data-chapter": "{key}",
                            onclick: move |e| { e.stop_propagation(); read.call(key.clone()); },
                            Icon { name: "book-open", size: 14 } "Read"
                        }
                    }
                } else if let Some((done, total)) = row.progress {
                    Chip {
                        text: if total > 0 { format!("{done} / {total} pages") } else { "Starting…".to_string() },
                        kind: Some("accent".to_string()),
                    }
                } else if row.failed {
                    Chip { text: "Couldn't download", kind: Some("bad".to_string()) }
                } else if row.queued {
                    span { class: "tip", "data-tip": "In the download queue", Chip { text: "Queued" } }
                }
            }
            td { class: "size", "{row.size}" }
            td { class: "src", title: "{sources_tip}",
                for name in row.source_names.iter().take(3) {
                    span { key: "{name}", class: if *name == row.from { "src-tag used" } else { "src-tag" }, "{name}" }
                }
                if row.source_names.len() > 3 {
                    span { class: "src-tag", "+{row.source_names.len() - 3}" }
                }
            }
            td { class: "date", "{row.date}" }
        }
    }
}
