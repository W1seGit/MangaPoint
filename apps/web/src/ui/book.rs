//! A book's overview, opened from its card in the Library: what it is, the chapters it holds (read
//! from any of them), and the way to add more chapters to it. The app's series overview, for a book
//! of your own.

use super::widgets::{Cover, EmptyState, Help, Icon};
use crate::app::{Ctx, Modal, ModalAction, Page};
use crate::bridge;
use crate::format::{human_bytes, plural};
use dioxus::prelude::*;
use serde_json::json;

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

const FORGET_HELP: &str = "Forget takes the book out of your library: its file, your place in it and the chapters you read are deleted from this device. To free the space but keep your place, remove the download instead.";

#[component]
pub fn BookPage() -> Element {
    let ctx = use_context::<Ctx>();
    let mut more = use_signal(|| false);
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
                                                action: ModalAction::RemoveBook { id: remove_id.clone(), forget: false },
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
                                            action: ModalAction::RemoveBook { id: forget_id.clone(), forget: true },
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
        div { class: "chapters-summary book-chapters-head",
            h2 { "Chapters" }
            span { class: "muted small", "{plural(book.chapters.len(), \"chapter\")} on this device" }
        }
        if book.chapters.is_empty() {
            EmptyState { title: "No chapters", message: "This book has no pages. Import more chapters to add some.", compact: true }
        } else {
            div { class: "table-wrap",
                div { class: "table-scroll",
                    table { class: "chapters", id: "book-chapters",
                        thead {
                            tr {
                                th { class: "num", "No." }
                                th { "Title" }
                                th { class: "size", "Pages" }
                                th { class: "status", "Status" }
                            }
                        }
                        tbody {
                            for (i, c) in book.chapters.iter().enumerate() {
                                {
                                    let id = book.id.clone();
                                    let name = c.label(i);
                                    let number = c.number.map(|n| n.to_string()).unwrap_or_else(|| (i + 1).to_string());
                                    rsx! {
                                        tr { key: "{i}", class: "book-chapter", "data-index": "{i}",
                                            onclick: move |_| { if !needs_file { super::reader::open_reader(ctx, &id, Some(i)); } },
                                            td { class: "num", "{number}" }
                                            td { class: "name", title: "{name}", "{name}" }
                                            td { class: "size", "{c.count}" }
                                            td { class: "status",
                                                if read[i] { span { class: "chip", "Read" } }
                                                if !needs_file {
                                                    button { class: "btn small chapter-read", "aria-label": "Read {name}", title: "Read this chapter",
                                                        Icon { name: "book-open", size: 14 } "Read"
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
        }
    }
}
