//! A book's overview, opened from its card in the Library: what it is, the chapters it holds (read
//! from any of them), and the way to add more chapters to it. The app's series overview, for a book
//! of your own.

use super::widgets::{Cover, EmptyState, Help, Icon};
use crate::app::{Ctx, Modal, ModalAction, Page};
use crate::bridge;
use crate::format::{human_bytes, plural};
use super::chapter_table::{ChapterGrid, GridRow};
use crate::logic::SortKey;
use dioxus::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;

/// Show the overview of book `id`.
pub fn open_overview(ctx: Ctx, id: &str) {
    let mut open = ctx.book_page;
    open.set(id.to_string());
    ctx.go(Page::Book);
}

/// Open the file picker to add chapters to book `id` (it must start inside the click).
fn pick_more(id: &str) {
    bridge::send("pick_more", json!({ "book": id }));
}

/// Mark chapters of book `id` as read or not read.
fn mark(ctx: Ctx, id: &str, chapters: &BTreeSet<usize>, read: bool) {
    let Some(book) = ctx.book(id) else { return };
    let mut lib = ctx.library;
    {
        let mut guard = lib.write();
        for &c in chapters {
            if read {
                guard.mark_read(&book, c, bridge::now_ms());
            } else {
                guard.mark_unread(&book, c);
            }
        }
    }
    ctx.save_state_of(id);
    ctx.notify(format!("Marked {} as {}.", plural(chapters.len(), "chapter"), if read { "read" } else { "not read" }));
}

const FORGET_HELP: &str = "Forget takes the book out of your library: its file, your place in it and the chapters you read are deleted from this device. To free the space but keep your place, remove the download instead.";

#[component]
pub fn BookPage() -> Element {
    let ctx = use_context::<Ctx>();
    let mut more = use_signal(|| false);
    let mut picked = use_signal(BTreeSet::<usize>::new);
    let mut filter = use_signal(String::new);
    let mut sort = use_signal(|| (SortKey::Number, false));
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    let id = ctx.book_page.read().clone();
    let lib = ctx.library.read();
    let Some(book) = lib.find(&id).cloned() else {
        return rsx! {
            button { class: "btn ghost back-link", id: "book-back", onclick: move |_| ctx.go(Page::Library),
                Icon { name: "arrow-left", size: 16 } "Library"
            }
            EmptyState { title: "This book is not in your library", message: "It may have been forgotten. Go back to the library to see what you have." }
        };
    };
    let reading = lib.read_button(&book);
    let finished = lib.finished(&book);
    let read: Vec<bool> = (0..book.chapters.len()).map(|i| lib.is_read(&book, i)).collect();
    drop(lib);
    let read_count = read.iter().filter(|r| **r).count();
    // The rows as the table shows them: filtered by the words typed, in the order asked for.
    let words: Vec<String> = filter.read().split_whitespace().map(str::to_lowercase).collect();
    let mut order: Vec<usize> = (0..book.chapters.len())
        .filter(|&i| {
            let c = &book.chapters[i];
            let hay = format!("{} {}", c.label(i).to_lowercase(), c.number.map(crate::format::format_number).unwrap_or_default());
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect();
    let (key, descending) = sort();
    order.sort_by(|&a, &b| {
        let (x, y) = (&book.chapters[a], &book.chapters[b]);
        let by = match key {
            SortKey::Name => x.label(a).to_lowercase().cmp(&y.label(b).to_lowercase()),
            _ => x.number.unwrap_or(a as f64).total_cmp(&y.number.unwrap_or(b as f64)),
        };
        if descending { by.reverse() } else { by }
    });
    let shown = order.len();
    let all_shown = order.clone();
    let all_ticked = !all_shown.is_empty() && all_shown.iter().all(|i| picked.read().contains(i));
    let picked_read = picked.read().iter().filter(|i| read.get(**i).copied().unwrap_or(false)).count();
    let numbers: Vec<Option<f64>> = book.chapters.iter().map(|c| c.number).collect();
    let needs_file_rows = book.needs_file;
    let grid_rows: Vec<GridRow> = order
        .iter()
        .map(|&i| {
            let c = &book.chapters[i];
            GridRow {
                index: i,
                number: c.number.map(crate::format::format_number).unwrap_or_else(|| "\u{2013}".to_string()),
                name: c.label(i),
                saved: !needs_file_rows,
                read_key: (!needs_file_rows).then(|| i.to_string()),
                read: read[i],
                ..GridRow::default()
            }
        })
        .collect();
    let kind = match book.kind.as_str() {
        "epub" => "EPUB",
        "images" => "Pictures",
        _ => "CBZ",
    };
    let pages = book.chapters.iter().map(|c| c.count).sum::<usize>();
    let (read_id, export_id, more_id, again_id, remove_id, forget_id) =
        (book.id.clone(), book.id.clone(), book.id.clone(), book.id.clone(), book.id.clone(), book.id.clone());
    let (remove_title, forget_title) = (book.title.clone(), book.title.clone());
    let needs_file = book.needs_file;
    rsx! {
        button { class: "btn ghost back-link", id: "book-back", onclick: move |_| ctx.go(Page::Library),
            Icon { name: "arrow-left", size: 16 } "Library"
        }
        div { class: "overview", id: "book-overview", "data-book": "{book.id}",
            div { class: "overview-cover",
                Cover { id: book.id.clone(), has_cover: book.has_cover }
            }
            div { class: "overview-body",
                div { class: "overview-title",
                    h1 { id: "book-title", "{book.title}" }
                }
                p { class: "muted", id: "book-stats",
                    "{plural(book.chapters.len(), \"chapter\")} \u{b7} {plural(pages, \"page\")} \u{b7} {human_bytes(book.size)} on this device \u{b7} {kind}"
                    if finished { " \u{b7} Finished" }
                }
                div { class: "row wrap book-actions",
                    if needs_file {
                        button { class: "btn primary", id: "book-import-again",
                            onclick: move |_| super::library::pick_for(&again_id),
                            Icon { name: "file-up", size: 16 } "Import its file"
                        }
                    } else {
                        if let Some(r) = reading.clone() {
                            button { class: "btn primary", id: "book-read",
                                onclick: move |_| super::reader::open_reader(ctx, &read_id, None),
                                Icon { name: "book-open", size: 16 } "{r.label}"
                            }
                        }
                        button { class: "btn", id: "book-export",
                            onclick: move |_| super::export::export_book(ctx, &export_id),
                            Icon { name: "file-down", size: 16 } "Export\u{2026}"
                        }
                        button { class: "btn", id: "book-import-more", title: "Add the chapters of more files to this book",
                            onclick: move |_| pick_more(&more_id),
                            Icon { name: "file-up", size: 16 } "Import more chapters"
                        }
                    }
                    div { class: "bar-menu",
                        button { class: "btn", id: "book-more", title: "More actions", onclick: move |_| more.set(!more()),
                            Icon { name: "ellipsis", size: 18 }
                        }
                        if more() {
                            div { class: "menu-scrim", onclick: move |_| more.set(false) }
                            div { class: "menu down",
                                if !needs_file {
                                    button { class: "menu-item", id: "book-remove-download",
                                        onclick: move |_| {
                                            more.set(false);
                                            ctx.ask(Modal {
                                                title: format!("Remove the download of {remove_title}?"),
                                                body: "Its file is deleted from this device to free the space. The book stays in your library with your place in it: import the file again to carry on.".into(),
                                                confirm: "Remove download".into(), cancel: "Keep".into(), danger: true,
                                                action: ModalAction::RemoveDownloads(vec![remove_id.clone()]),
                                            });
                                        },
                                        Icon { name: "x", size: 15 } "Remove download"
                                    }
                                }
                                button { class: "menu-item danger forget-item", id: "book-forget",
                                    onclick: move |_| {
                                        more.set(false);
                                        ctx.ask(Modal {
                                            title: format!("Forget {forget_title}?"),
                                            body: "The book, its file and your place in it are deleted from this device. Import the file again to read it from the start.".into(),
                                            confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                            action: ModalAction::Forget(vec![forget_id.clone()]),
                                        });
                                    },
                                    Icon { name: "trash", size: 15 } span { class: "grow", "Forget" }
                                    span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                                }
                            }
                        }
                    }
                }
            }
        }
        div { class: "row wrap chapters-summary book-chapters-head",
            div { class: "grow",
                strong { "{plural(book.chapters.len(), \"chapter\")}" }
                span { class: "muted", " \u{b7} {read_count} read" }
            }
        }
        div { class: "toolbar",
            div { class: "input-icon", style: "max-width: 280px",
                Icon { name: "list-filter", size: 16 }
                input {
                    r#type: "search", placeholder: "Filter chapters", id: "book-filter",
                    value: "{filter}",
                    oninput: move |e| filter.set(e.value()),
                }
            }
            form {
                class: "row", style: "gap: 6px; margin-left: auto",
                onsubmit: move |e| {
                    e.prevent_default();
                    match crate::ranges::select_by_text(&range_text.read(), &numbers) {
                        Ok(found) => { range_error.set(None); picked.set(found.into_iter().collect()); }
                        Err(msg) => range_error.set(Some(msg)),
                    }
                },
                input {
                    r#type: "text", style: "width: 190px", id: "book-range",
                    placeholder: "Range: 1-20, 25, 30-",
                    value: "{range_text}",
                    oninput: move |e| range_text.set(e.value()),
                }
                button { class: "btn small", r#type: "submit", "Select" }
            }
        }
        if let Some(msg) = range_error() {
            p { class: "small", style: "color: var(--bad); margin: 0 0 8px", "{msg}" }
        }
        ChapterGrid {
            rows: grid_rows,
            picked: picked.read().iter().copied().collect::<std::collections::HashSet<usize>>(),
            sort: sort(),
            lock_done: false,
            compact: true,
            on_read: {
                let id = book.id.clone();
                move |key: String| {
                    if let Ok(i) = key.parse::<usize>() { super::reader::open_reader(ctx, &id, Some(i)); }
                }
            },
            on_sort: move |key| {
                let current = sort();
                sort.set(crate::logic::next_sort(current, key));
            },
            on_toggle: move |index: usize| {
                let mut s = picked.write();
                if !s.remove(&index) {
                    s.insert(index);
                }
            },
            on_set: move |(indexes, on): (Vec<usize>, bool)| {
                let mut s = picked.write();
                for i in indexes {
                    if on { s.insert(i); } else { s.remove(&i); }
                }
            },
            empty_message: if book.chapters.is_empty() { "This book has no chapters. Import more chapters to add some.".to_string() } else { format!("Nothing matches \u{201c}{}\u{201d}. Try fewer words.", filter()) },
            max_height: None,
        }
        p { class: "muted small", style: "margin: 8px 0 0",
            "Showing {shown} of {book.chapters.len()} chapters. Click a heading to sort, a row to tick it."
        }

        if !picked.read().is_empty() {
            div { class: "action-bar", id: "book-bar",
                div { class: "bar-count",
                    strong { id: "book-bar-count", "{picked.read().len()} selected" }
                    span { class: "muted small", "{picked_read} read" }
                }
                button { class: "btn ghost small", id: "book-select-all",
                    onclick: move |_| {
                        if all_ticked { picked.set(BTreeSet::new()); } else { picked.set(all_shown.iter().copied().collect()); }
                    },
                    if all_ticked { "Select none" } else { "Select all" }
                }
                span { class: "grow" }
                if !needs_file {
                    button { class: "btn primary", id: "book-export-picked", title: "Edit and export the chapters you ticked",
                        onclick: {
                            let id = book.id.clone();
                            move |_| {
                                let chosen: BTreeSet<usize> = picked.read().iter().copied().collect();
                                picked.set(BTreeSet::new());
                                super::export::export_book(ctx, &id);
                                super::export::edit(ctx, |e| e.chosen = chosen.clone());
                            }
                        },
                        Icon { name: "file-down", size: 15 } "Export\u{2026}"
                    }
                }
                button { class: "btn", id: "book-mark-read", title: "Mark the chapters you ticked as read",
                    onclick: {
                        let id = book.id.clone();
                        move |_| mark(ctx, &id, &picked.read(), true)
                    },
                    Icon { name: "check", size: 15 } "Mark as read"
                }
                button { class: "btn", id: "book-mark-unread", title: "Mark the chapters you ticked as not read",
                    onclick: {
                        let id = book.id.clone();
                        move |_| mark(ctx, &id, &picked.read(), false)
                    },
                    "Mark as unread"
                }
            }
        }
    }
}
