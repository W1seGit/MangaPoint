//! The Library: the books kept in this browser, and the places waiting for
//! their file.

use super::widgets::{Chip, Cover, EmptyState, Icon};
use crate::app::{Ctx, Modal, ModalAction, Page};
use crate::bridge;
use crate::format::{ago, human_bytes, plural};
use crate::model::{Book, LibrarySort, LibraryView};
use dioxus::prelude::*;
use serde_json::json;

/// Open the browser's file picker (it must start inside the click).
pub fn pick_files() {
    bridge::send("pick_files", json!({}));
}

pub fn pick_folder() {
    bridge::send("pick_folder", json!({}));
}

fn kind_label(book: &Book) -> &'static str {
    match book.kind.as_str() {
        "epub" => "EPUB",
        "images" => "Pictures",
        _ => "CBZ",
    }
}

#[component]
pub fn LibraryPage() -> Element {
    let ctx = use_context::<Ctx>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    let mut import_menu = use_signal(|| false);
    let mut menu = use_signal(|| None::<String>);

    let lib = ctx.library.read();
    let order = lib.order(view(), sort(), &filter.read());
    let total_bytes: u64 = lib.books.iter().map(|b| b.size).sum();
    let count = lib.books.len();
    let waiting: Vec<_> = lib.waiting().into_iter().cloned().collect();
    let empty = count == 0;
    let now = bridge::now_ms() / 1000;

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Library" }
                p { class: "muted",
                    if empty {
                        "Your own comics and books, kept on this device."
                    } else {
                        "{plural(count, \"book\")} \u{b7} {human_bytes(total_bytes)} on this device"
                    }
                }
            }
            div { class: "menu-anchor import-anchor",
                button {
                    class: "btn", id: "import-button", title: "Bring in comics, books and pictures you already have",
                    onclick: move |_| import_menu.set(!import_menu()),
                    Icon { name: "file-up", size: 16 } "Import"
                }
                if import_menu() {
                    div { class: "menu-scrim", onclick: move |_| import_menu.set(false) }
                    div { class: "menu down", id: "import-menu",
                        button { class: "menu-item", id: "import-files",
                            onclick: move |_| { import_menu.set(false); pick_files(); },
                            Icon { name: "file-up", size: 15 } "Choose files\u{2026}"
                        }
                        button { class: "menu-item", id: "import-folder",
                            onclick: move |_| { import_menu.set(false); pick_folder(); },
                            Icon { name: "folder-open", size: 15 } "Choose a folder\u{2026}"
                        }
                        div { class: "menu-note muted small", "Or drop files and folders anywhere on this page." }
                    }
                }
            }
        }

        if empty {
            EmptyState {
                title: "Your library is empty",
                message: "Import a CBZ or ZIP of pictures, an EPUB with pictures, a folder of pictures or a few loose pictures. They stay on this device, nothing is uploaded.",
                shelf: true,
                div { class: "row wrap empty-actions",
                    button { class: "btn primary", id: "empty-import-files", onclick: move |_| pick_files(), Icon { name: "file-up", size: 16 } "Choose files" }
                    button { class: "btn", id: "empty-import-folder", onclick: move |_| pick_folder(), Icon { name: "folder-open", size: 16 } "Choose a folder" }
                }
                p { class: "muted small", style: "margin-top: 14px",
                    "RAR, 7z and PDF books are not supported on the web yet."
                }
            }
        } else {
            div { class: "lib-tools",
                div { class: "pillrow",
                    for v in LibraryView::ALL {
                        button {
                            key: "{v.label()}",
                            class: if view() == v { "pill on" } else { "pill" },
                            onclick: move |_| view.set(v),
                            "{v.label()}"
                        }
                    }
                }
                input {
                    class: "lib-filter", r#type: "search", placeholder: "Filter your library", "aria-label": "Filter your library",
                    value: "{filter}", oninput: move |e| filter.set(e.value()),
                }
                select {
                    class: "lib-sort", "aria-label": "Sort", value: "{sort().key()}",
                    onchange: move |e| sort.set(LibrarySort::from_key(&e.value())),
                    for o in LibrarySort::ALL {
                        option { key: "{o.key()}", value: "{o.key()}", selected: sort() == o, "{o.label()}" }
                    }
                }
            }
            if order.is_empty() {
                EmptyState {
                    title: "Nothing matches",
                    message: "No book in this view has that in its name. Try fewer letters or another view.",
                    compact: true,
                }
            }
            div { class: "poster-grid",
                for &i in order.iter() {
                    {
                        let b = lib.books[i].clone();
                        let id = b.id.clone();
                        let menu_open = menu().as_deref() == Some(b.id.as_str());
                        let reading = lib.read_button(&b);
                        let finished = lib.finished(&b);
                        let sub = format!("{} \u{b7} {}", plural(b.chapters.len(), "chapter"), human_bytes(b.size));
                        let (open_id, read_id, text_id, menu_id, export_id, remove_id, again_id) =
                            (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                        let remove_title = b.title.clone();
                        let needs_file = b.needs_file;
                        let file_name = b.file_name.clone();
                        rsx! {
                            div { class: "poster", key: "{id}", "data-book": "{id}",
                                div { class: "poster-art", title: "Open {b.title}",
                                    onclick: move |_| open_book(ctx, &open_id),
                                    Cover { id: id.clone(), has_cover: b.has_cover }
                                    if needs_file {
                                        span { class: "poster-badge saved", title: "The browser did not keep this file", "Needs its file" }
                                    }
                                    button {
                                        class: if menu_open { "poster-more open" } else { "poster-more" },
                                        title: "More actions", "aria-label": "More actions for {b.title}",
                                        onclick: move |e| {
                                            e.stop_propagation();
                                            menu.set(if menu_open { None } else { Some(menu_id.clone()) });
                                        },
                                        Icon { name: "ellipsis", size: 16 }
                                    }
                                }
                                if menu_open {
                                    div { class: "menu-scrim", onclick: move |_| menu.set(None) }
                                    div { class: "menu down poster-menu",
                                        if !needs_file {
                                            button { class: "menu-item",
                                                onclick: { let id = read_id.clone(); move |_| { menu.set(None); super::reader::open_reader(ctx, &id, None); } },
                                                Icon { name: "book-open", size: 15 } "Read"
                                            }
                                            button { class: "menu-item",
                                                onclick: move |_| { menu.set(None); super::export::export_book(ctx, &export_id); },
                                                Icon { name: "file-down", size: 15 } "Export\u{2026}"
                                            }
                                        } else {
                                            button { class: "menu-item",
                                                onclick: move |_| { menu.set(None); ctx.warn(format!("Import {file_name} again to read it.")); pick_files(); let _ = &again_id; },
                                                Icon { name: "file-up", size: 15 } "Import the file again"
                                            }
                                        }
                                        button { class: "menu-item danger",
                                            onclick: move |_| {
                                                menu.set(None);
                                                ctx.ask(Modal {
                                                    title: format!("Remove {remove_title} from this device?"),
                                                    body: "The book's file is deleted from this browser. Your place in it is remembered, so importing the same file again carries on where you stopped.".into(),
                                                    confirm: "Remove".into(), cancel: "Keep".into(), danger: true,
                                                    action: ModalAction::RemoveBook { id: remove_id.clone(), forget: false },
                                                });
                                            },
                                            Icon { name: "trash", size: 15 } "Remove from this device"
                                        }
                                    }
                                }
                                div { class: "poster-text", title: "Open {b.title}",
                                    onclick: move |_| open_book(ctx, &text_id),
                                    strong { class: "poster-title", title: "{b.title}", "{b.title}" }
                                    span { class: "muted small", "{sub}" }
                                    div { class: "poster-tags",
                                        Chip { text: kind_label(&b).to_string(), kind: Some("accent".to_string()) }
                                        if finished { Chip { text: "Finished", kind: Some("ok".to_string()) } }
                                        if needs_file { Chip { text: "Needs its file", kind: Some("warn".to_string()) } }
                                    }
                                }
                                if let Some(reading) = reading {
                                    button {
                                        class: "btn poster-read", id: "read-{id}", "data-book": "{id}",
                                        "aria-label": "{reading.label} {b.title}", title: "Read {b.title}",
                                        onclick: move |_| super::reader::open_reader(ctx, &read_id, None),
                                        Icon { name: "book-open", size: 15 } "{reading.label}"
                                    }
                                    if let Some(done) = reading.fraction {
                                        div { class: "progress poster-read-progress", "aria-label": "Read so far",
                                            title: "{(done * 100.0).round()}% read",
                                            div { style: "width: {(done * 100.0).round()}%" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !waiting.is_empty() {
            section { class: "waiting", id: "waiting",
                h2 { "Waiting for the file" }
                p { class: "muted small",
                    "These books are in your reading state, but their file is not on this device yet. Import the same file again and each one carries on where you stopped."
                }
                div { class: "card waiting-list",
                    for w in waiting.iter() {
                        {
                            let id = w.fingerprint.clone();
                            let title = if w.title.is_empty() { w.file_name.clone() } else { w.title.clone() };
                            let place = if w.last_read_at > 0 {
                                format!("{} \u{b7} page {} \u{b7} {}", w.position.chapter_name, w.position.page + 1, ago(now, w.last_read_at / 1000))
                            } else {
                                "Not started".to_string()
                            };
                            let read = w.chapters_read.len();
                            let forget = id.clone();
                            rsx! {
                                div { class: "list-row waiting-row", key: "{id}", "data-fingerprint": "{id}",
                                    Icon { name: "clock", size: 18 }
                                    div { class: "grow",
                                        strong { class: "waiting-title", "{title}" }
                                        div { class: "muted small", "{w.file_name} \u{b7} {human_bytes(w.size)}" }
                                        div { class: "muted small", "{place}" if read > 0 { " \u{b7} {plural(read, \"chapter\")} read" } }
                                    }
                                    button { class: "btn ghost small", title: "Forget this place",
                                        onclick: move |_| ctx.ask(Modal {
                                            title: "Forget this place?".into(),
                                            body: format!("Your place in {title} and the chapters you read are deleted from this device. They stay in any reading-state file you saved."),
                                            confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                            action: ModalAction::ForgetWaiting(forget.clone()),
                                        }),
                                        Icon { name: "x", size: 15 } "Forget"
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "row wrap", style: "margin-top: 10px",
                    button { class: "btn", onclick: move |_| pick_files(), Icon { name: "file-up", size: 16 } "Import files" }
                    button { class: "btn", onclick: move |_| pick_folder(), Icon { name: "folder-open", size: 16 } "Import a folder" }
                }
            }
        }
    }
}

/// Open a book from its poster: read it, or explain what it needs.
fn open_book(ctx: Ctx, id: &str) {
    let Some(book) = ctx.book(id) else { return };
    if book.needs_file {
        ctx.warn(format!("The browser did not keep {}. Import the file again to read it.", book.file_name));
        return;
    }
    super::reader::open_reader(ctx, id, None);
}

pub fn goto_library(ctx: Ctx) {
    ctx.go(Page::Library);
}
