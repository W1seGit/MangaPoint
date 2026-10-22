//! The Library: the books kept in this browser, and the places waiting for
//! their file, as cards in one grid. Cards can be selected for the bar at the bottom.

use super::widgets::{Cover, EmptyState, Help, Icon, NoCover};
use crate::app::{Ctx, Modal, ModalAction, Page};
use crate::bridge;
use crate::format::{ago, human_bytes, plural};
use crate::model::{Book, LibrarySort, LibraryView};
use dioxus::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;

/// Open the browser's file picker (it must start inside the click).
pub fn pick_files() {
    bridge::send("pick_files", json!({}));
}

pub fn pick_folder() {
    bridge::send("pick_folder", json!({}));
}

/// Open the file picker for one book that is waiting for its file: only that book's file is taken.
pub fn pick_for(fingerprint: &str) {
    bridge::send("pick_for", json!({ "fingerprint": fingerprint }));
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
    let mut picked = use_signal(BTreeSet::<String>::new);
    let mut more = use_signal(|| false);

    let lib = ctx.library.read();
    let order = lib.order(view(), sort(), &filter.read());
    let total_bytes: u64 = lib.books.iter().map(|b| b.size).sum();
    let count = lib.books.len();
    // The places in the reading state whose file is not on this device yet are cards too.
    let needle = filter.read().trim().to_lowercase();
    let waiting: Vec<_> = if view() == LibraryView::All {
        lib.waiting()
            .into_iter()
            .filter(|w| needle.is_empty() || w.title.to_lowercase().contains(&needle) || w.file_name.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let empty = count == 0 && lib.waiting().is_empty();
    let now = bridge::now_ms() / 1000;
    let everything: Vec<String> = order.iter().map(|&i| lib.books[i].id.clone()).chain(waiting.iter().map(|w| w.fingerprint.clone())).collect();
    // As in the app: ticking a card starts a selection, and the bar shows while anything is ticked.
    let chosen: BTreeSet<String> = picked.read().iter().filter(|id| everything.contains(id)).cloned().collect();
    let selecting = !chosen.is_empty();
    let all_ticked = selecting && chosen.len() == everything.len();
    let chosen_bytes: u64 = chosen.iter().filter_map(|id| lib.find(id)).map(|b| b.size).sum();
    let chosen_books: Vec<String> = chosen.iter().filter(|id| lib.find(id).is_some_and(|b| !b.needs_file)).cloned().collect();

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
            if order.is_empty() && waiting.is_empty() {
                EmptyState {
                    title: "Nothing matches",
                    message: "No book in this view has that in its name. Try fewer letters or another view.",
                    compact: true,
                }
            }
            div { class: if selecting { "poster-grid selecting" } else { "poster-grid" },
                for &i in order.iter() {
                    {
                        let b = lib.books[i].clone();
                        let id = b.id.clone();
                        let menu_open = menu().as_deref() == Some(b.id.as_str());
                        let reading = lib.read_button(&b);
                        let finished = lib.finished(&b);
                        let have = b.chapters.len().to_string();
                        let stored = human_bytes(b.size);
                        let (open_id, read_id, text_id, menu_id, export_id, remove_id, again_id) =
                            (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                        let remove_title = b.title.clone();
                        let undownload_id = id.clone();
                        let needs_file = b.needs_file;
                        let is_on = picked.read().contains(&id);
                        let toggle_id = id.clone();
                        let tick_id = id.clone();
                        let read_pick = id.clone();
                        rsx! {
                            div { class: if is_on { "poster on" } else { "poster" }, key: "{id}", "data-book": "{id}",
                                div { class: "poster-art", title: "Open {b.title}",
                                    onclick: move |_| {
                                        if selecting { toggle(picked, &toggle_id); } else { open_book(ctx, &open_id); }
                                    },
                                    Cover { id: id.clone(), has_cover: b.has_cover }
                                    if needs_file {
                                        span { class: "poster-badge saved", title: "The browser did not keep this file", "Needs its file" }
                                    }
                                    button {
                                        class: if is_on { "poster-check on" } else { "poster-check" },
                                        title: if is_on { "Deselect" } else { "Select" }, "aria-label": if is_on { "Deselect" } else { "Select" },
                                        onclick: move |e| { e.stop_propagation(); toggle(picked, &tick_id); },
                                        if is_on { Icon { name: "check", size: 14 } }
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
                                                onclick: move |_| { menu.set(None); pick_for(&again_id); },
                                                Icon { name: "file-up", size: 15 } "Import"
                                            }
                                        }
                                        if !needs_file {
                                            button { class: "menu-item", id: "remove-download",
                                                onclick: {
                                                    let title = remove_title.clone();
                                                    let id = undownload_id.clone();
                                                    move |_| {
                                                        menu.set(None);
                                                        ctx.ask(Modal {
                                                            title: format!("Remove the download of {title}?"),
                                                            body: "Its file is deleted from this device to free the space. The book stays in your library with your place in it: import the file again to carry on.".into(),
                                                            confirm: "Remove download".into(), cancel: "Keep".into(), danger: true,
                                                            action: ModalAction::RemoveBook { id: id.clone(), forget: false },
                                                        });
                                                    }
                                                },
                                                Icon { name: "x", size: 15 } "Remove download"
                                            }
                                        }
                                        button { class: "menu-item danger forget-item",
                                            onclick: move |_| {
                                                menu.set(None);
                                                ctx.ask(Modal {
                                                    title: format!("Forget {remove_title}?"),
                                                    body: "The book, its file and your place in it are deleted from this device. Import the file again to read it from the start.".into(),
                                                    confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                                    action: ModalAction::RemoveBook { id: remove_id.clone(), forget: true },
                                                });
                                            },
                                            Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                                        }
                                    }
                                }
                                div { class: "poster-text", title: "Open {b.title}",
                                    onclick: move |_| { if selecting { toggle(picked, &text_id); } else { open_book(ctx, &text_id); } },
                                    strong { class: "poster-title", title: "{b.title}", "{b.title}" }
                                    div { class: "poster-stats",
                                        div { span { class: "poster-stat-label", "Chapters" } strong { "{have}" } }
                                        div { span { class: "poster-stat-label", "Storage" } strong { "{stored}" } }
                                    }
                                    span { class: "muted small poster-status",
                                        "{kind_label(&b)}"
                                        if finished { " · Finished" }
                                        if needs_file { " · Needs its file" }
                                    }
                                }
                                if let Some(reading) = reading {
                                    button {
                                        class: "btn poster-read", id: "read-{id}", "data-book": "{id}",
                                        "aria-label": "{reading.label} {b.title}", title: "Read {b.title}",
                                        onclick: move |_| { if selecting { toggle(picked, &read_pick); } else { super::reader::open_reader(ctx, &read_id, None); } },
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
                for w in waiting.iter() {
                    {
                        let fp = w.fingerprint.clone();
                        let title = if w.title.is_empty() { w.file_name.clone() } else { w.title.clone() };
                        let place = if w.last_read_at > 0 {
                            format!("{} \u{b7} page {} \u{b7} {}", w.position.chapter_name, w.position.page + 1, ago(now, w.last_read_at / 1000))
                        } else {
                            "Not started".to_string()
                        };
                        let menu_open = menu().as_deref() == Some(fp.as_str());
                        let is_on = picked.read().contains(&fp);
                        let (toggle_id, text_id, menu_id, import_id, button_id, forget_id) = (fp.clone(), fp.clone(), fp.clone(), fp.clone(), fp.clone(), fp.clone());
                        let tick_id = fp.clone();
                        let forget_title = title.clone();
                        rsx! {
                            div { class: if is_on { "poster not-installed on" } else { "poster not-installed" }, key: "{fp}", "data-fingerprint": "{fp}",
                                div { class: "poster-art",
                                    onclick: move |_| { if selecting { toggle(picked, &toggle_id); } },
                                    NoCover {}
                                    button {
                                        class: if is_on { "poster-check on" } else { "poster-check" },
                                        title: if is_on { "Deselect" } else { "Select" }, "aria-label": if is_on { "Deselect" } else { "Select" },
                                        onclick: move |e| { e.stop_propagation(); toggle(picked, &tick_id); },
                                        if is_on { Icon { name: "check", size: 14 } }
                                    }
                                    button {
                                        class: if menu_open { "poster-more open" } else { "poster-more" },
                                        title: "More actions", "aria-label": "More actions for {title}",
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
                                        button { class: "menu-item",
                                            onclick: move |_| { menu.set(None); pick_for(&import_id); },
                                            Icon { name: "file-up", size: 15 } "Import"
                                        }
                                        button { class: "menu-item danger",
                                            onclick: move |_| {
                                                menu.set(None);
                                                ctx.ask(Modal {
                                                    title: format!("Forget {forget_title}?"),
                                                    body: "Your place in it and the chapters you read are deleted from this device. They stay in any reading-state file you saved.".into(),
                                                    confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                                    action: ModalAction::ForgetWaiting(forget_id.clone()),
                                                });
                                            },
                                            Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                                        }
                                    }
                                }
                                div { class: "poster-text",
                                    onclick: move |_| { if selecting { toggle(picked, &text_id); } },
                                    strong { class: "poster-title", title: "{title}", "{title}" }
                                    span { class: "muted small poster-status", "Not installed yet" }
                                    span { class: "muted small poster-place", "{place}" }
                                }
                                button {
                                    class: "btn poster-read", id: "import-{fp}", "data-fingerprint": "{fp}",
                                    "aria-label": "Import {title}", title: "Pick the file for {title}",
                                    onclick: move |_| { if selecting { toggle(picked, &button_id); } else { pick_for(&button_id); } },
                                    Icon { name: "file-up", size: 15 } "Import"
                                }
                            }
                        }
                    }
                }
            }
        }

        if selecting {
            div { class: "action-bar", id: "action-bar",
                div { class: "bar-count",
                    strong { id: "bar-count", "{chosen.len()} selected" }
                    span { class: "muted small", "{human_bytes(chosen_bytes)} on this device" }
                }
                button { class: "btn ghost small", id: "bar-select-all",
                    onclick: move |_| {
                        if all_ticked { picked.set(BTreeSet::new()); } else { picked.set(everything.iter().cloned().collect()); }
                    },
                    if all_ticked { "Select none" } else { "Select all" }
                }
                span { class: "grow" }
                if chosen_books.len() == 1 && chosen.len() == 1 {
                    button { class: "btn primary", id: "bar-export", title: "Edit and export the book you ticked",
                        onclick: {
                            let id = chosen_books[0].clone();
                            move |_| { picked.set(BTreeSet::new()); super::export::export_book(ctx, &id); }
                        },
                        Icon { name: "file-down", size: 15 } "Export\u{2026}"
                    }
                }
                div { class: "bar-menu",
                    button { class: "btn", id: "bar-more", title: "More actions", onclick: move |_| more.set(!more()),
                        Icon { name: "ellipsis", size: 18 }
                    }
                    if more() {
                        div { class: "menu-scrim", onclick: move |_| more.set(false) }
                        div { class: "menu up",
                            if !chosen_books.is_empty() {
                                button { class: "menu-item", id: "bar-remove-downloads", title: "Delete the files to free the space, and keep the books and your places",
                                    onclick: {
                                        let ids = chosen_books.clone();
                                        move |_| {
                                            more.set(false);
                                            ctx.ask(Modal {
                                                title: format!("Remove the downloads of {}?", plural(ids.len(), "book")),
                                                body: "Their files are deleted from this device to free the space. The books stay in your library with your places in them: import the files again to carry on.".into(),
                                                confirm: "Remove downloads".into(), cancel: "Keep".into(), danger: true,
                                                action: ModalAction::RemoveDownloads(ids.clone()),
                                            });
                                            picked.set(BTreeSet::new());
                                        }
                                    },
                                    Icon { name: "x", size: 15 } "Remove downloads"
                                }
                            }
                            button { class: "menu-item danger forget-item", id: "bar-forget", title: "Take the books you ticked out of your library",
                                onclick: {
                                    let ids: Vec<String> = chosen.iter().cloned().collect();
                                    move |_| {
                                        more.set(false);
                                        ctx.ask(Modal {
                                            title: format!("Forget {}?", plural(ids.len(), "book")),
                                            body: "The books and your places in them are deleted from this device. This cannot be undone.".into(),
                                            confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                            action: ModalAction::ForgetMany(ids.clone()),
                                        });
                                        picked.set(BTreeSet::new());
                                    }
                                },
                                Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// What Forget does, behind its question mark.
const FORGET_HELP: &str = "Forget takes the book out of your library: its file, your place in it and the chapters you read are deleted from this device. To free the space but keep your place, remove the download instead.";

/// Add the card to the pick, or take it off.
fn toggle(mut picked: Signal<BTreeSet<String>>, id: &str) {
    let mut set = picked.write();
    if !set.remove(id) {
        set.insert(id.to_string());
    }
}

/// Open a book from its poster: read it, or explain what it needs.
/// A card was clicked: its overview, with the chapters and what can be done with the book.
fn open_book(ctx: Ctx, id: &str) {
    if ctx.book(id).is_some() {
        super::book::open_overview(ctx, id);
    }
}

pub fn goto_library(ctx: Ctx) {
    ctx.go(Page::Library);
}
