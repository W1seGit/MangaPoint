//! The Library: the books kept in this browser, and the places waiting for
//! their file, as cards in one grid. Cards can be selected for the bar at the bottom.

use super::widgets::{BrowserCover, EmptyState, Help, Icon, NoCover};
use crate::app::{Ctx, Modal, ModalAction};
use crate::bridge;
use crate::format::{ago, human_bytes, plural};
use crate::model::{Book, LibrarySort, LibraryView};
use dioxus::prelude::*;
use mangapoint_ui::library::{ActionBar, InfoDialog, InfoSheet, LibraryTools, Poster, PosterButton, PosterFacts, PosterTabs};
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
pub fn BrowserLibraryPage() -> Element {
    let ctx = use_context::<Ctx>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    let mut import_menu = use_signal(|| false);
    let mut menu = use_signal(|| None::<String>);
    // The card whose Information is open.
    let mut info = use_signal(|| None::<InfoSheet>);
    let mut picked = use_signal(BTreeSet::<String>::new);

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
            LibraryTools {
                views: LibraryView::ALL.iter().map(|v| (v.label(), *v == view())).collect(),
                on_view: move |i| view.set(LibraryView::ALL[i]),
                filter: filter(),
                on_filter: move |text| filter.set(text),
                sorts: LibrarySort::ALL.iter().map(|o| (o.key(), o.label(), *o == sort())).collect(),
                on_sort: move |key: String| sort.set(LibrarySort::from_key(&key)),
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
                        let (open_id, read_id, menu_id, export_id, remove_id, again_id) =
                            (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                        let remove_title = b.title.clone();
                        let undownload_id = id.clone();
                        let needs_file = b.needs_file;
                        let is_on = picked.read().contains(&id);
                        let tick_id = id.clone();
                        let read_pick = id.clone();
                        let mut status = kind_label(&b).to_string();
                        if finished { status.push_str(" \u{b7} Finished"); }
                        if needs_file { status.push_str(" \u{b7} Needs its file"); }
                        let sheet = InfoSheet {
                            title: b.title.clone(),
                            facts: vec![
                                ("Kind".to_string(), kind_label(&b).to_string()),
                                ("Chapters".to_string(), have.clone()),
                                ("Stored on this device".to_string(), if needs_file { "Its file is not kept".to_string() } else { stored.clone() }),
                            ],
                            files: None,
                        };
                        let facts = PosterFacts {
                            id: id.clone(),
                            title: b.title.clone(),
                            tabs: Some(PosterTabs { downloaded: format!("{have}/{have}"), exported: None }),
                            stats: Some((have.clone(), stored.clone())),
                            lines: vec![("poster-status", status)],
                            button: reading.map(|r| PosterButton {
                                id: format!("read-{id}"),
                                aria: format!("{} {}", r.label, b.title),
                                label: r.label,
                                icon: "book-open",
                                title: format!("Read {}", b.title),
                                fraction: r.fraction,
                            }),
                            not_installed: false,
                        };
                        rsx! {
                            Poster { key: "{id}", facts, selected: is_on, selecting, menu_open,
                                art: rsx! {
                                    BrowserCover { id: id.clone(), has_cover: b.has_cover }
                                    if needs_file {
                                        span { class: "poster-badge saved", title: "The browser did not keep this file", "Needs its file" }
                                    }
                                },
                                menu: rsx! {
                                    button { class: "menu-item",
                                        onclick: move |_| info.set(Some(sheet.clone())),
                                        Icon { name: "info", size: 15 } "Information"
                                    }
                                    if !needs_file {
                                        button { class: "menu-item",
                                            onclick: { let id = read_id.clone(); move |_| super::reader::open_reader(ctx, &id, None) },
                                            Icon { name: "book-open", size: 15 } "Read"
                                        }
                                        button { class: "menu-item",
                                            onclick: move |_| super::export::export_book(ctx, &export_id),
                                            Icon { name: "file-down", size: 15 } "Export\u{2026}"
                                        }
                                    } else {
                                        button { class: "menu-item",
                                            onclick: move |_| pick_for(&again_id),
                                            Icon { name: "file-up", size: 15 } "Import"
                                        }
                                    }
                                    if !needs_file {
                                        button { class: "menu-item", id: "remove-download",
                                            onclick: {
                                                let title = remove_title.clone();
                                                let id = undownload_id.clone();
                                                move |_| {
                                                    ctx.ask(Modal {
                                                        title: format!("Remove the download of {title}?"),
                                                        body: "Its file is deleted from this device to free the space. The book stays in your library with your place in it: import the file again to carry on.".into(),
                                                        confirm: "Remove download".into(), cancel: "Keep".into(), danger: true,
                                                        action: ModalAction::RemoveDownloads(vec![id.clone()]),
                                                    });
                                                }
                                            },
                                            Icon { name: "x", size: 15 } "Remove download"
                                        }
                                    }
                                    button { class: "menu-item danger forget-item",
                                        onclick: move |_| {
                                            ctx.ask(Modal {
                                                title: format!("Forget {remove_title}?"),
                                                body: "The book, its file and your place in it are deleted from this device. Import the file again to read it from the start.".into(),
                                                confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                                action: ModalAction::Forget(vec![remove_id.clone()]),
                                            });
                                        },
                                        Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                                    }
                                },
                                on_open: move |_| open_book(ctx, &open_id),
                                on_toggle: move |_| toggle(picked, &tick_id),
                                on_menu: move |open: bool| menu.set(open.then(|| menu_id.clone())),
                                on_button: move |_| super::reader::open_reader(ctx, &read_pick, None),
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
                        let (menu_id, import_id, button_id, forget_id) = (fp.clone(), fp.clone(), fp.clone(), fp.clone());
                        let tick_id = fp.clone();
                        let forget_title = title.clone();
                        let facts = PosterFacts {
                            id: fp.clone(),
                            title: title.clone(),
                            tabs: None,
                            stats: None,
                            lines: vec![("poster-status", "Not installed yet".to_string()), ("poster-place", place.clone())],
                            button: Some(PosterButton {
                                id: format!("import-{fp}"),
                                label: "Import".into(),
                                icon: "file-up",
                                title: format!("Pick the file for {title}"),
                                aria: format!("Import {title}"),
                                fraction: None,
                            }),
                            not_installed: true,
                        };
                        rsx! {
                            Poster { key: "{fp}", facts, selected: is_on, selecting, menu_open,
                                art: rsx! { NoCover {} },
                                menu: rsx! {
                                    button { class: "menu-item",
                                        onclick: move |_| pick_for(&import_id),
                                        Icon { name: "file-up", size: 15 } "Import"
                                    }
                                    button { class: "menu-item danger",
                                        onclick: move |_| {
                                            ctx.ask(Modal {
                                                title: format!("Forget {forget_title}?"),
                                                body: "Your place in it and the chapters you read are deleted from this device. They stay in any reading-state file you saved.".into(),
                                                confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                                action: ModalAction::Forget(vec![forget_id.clone()]),
                                            });
                                        },
                                        Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                                    }
                                },
                                on_open: move |_| {},
                                on_toggle: move |_| toggle(picked, &tick_id),
                                on_menu: move |open: bool| menu.set(open.then(|| menu_id.clone())),
                                on_button: move |_| pick_for(&button_id),
                            }
                        }
                    }
                }
            }
        }

        if let Some(sheet) = info() {
            InfoDialog { sheet, on_close: move |_| info.set(None) }
        }
        if selecting {
            ActionBar {
                count: chosen.len(),
                size: format!("{} on this device", human_bytes(chosen_bytes)),
                all_ticked,
                on_all: move |all: bool| {
                    if all { picked.set(everything.iter().cloned().collect()); } else { picked.set(BTreeSet::new()); }
                },
                buttons: rsx! {
                    if chosen_books.len() == 1 && chosen.len() == 1 {
                        button { class: "btn primary", id: "bar-export", title: "Edit and export the book you ticked",
                            onclick: {
                                let id = chosen_books[0].clone();
                                move |_| { picked.set(BTreeSet::new()); super::export::export_book(ctx, &id); }
                            },
                            Icon { name: "file-down", size: 15 } "Export\u{2026}"
                        }
                    }
                },
                more: rsx! {
                    if !chosen_books.is_empty() {
                        button { class: "menu-item", id: "bar-remove-downloads", title: "Delete the files to free the space, and keep the books and your places",
                            onclick: {
                                let ids = chosen_books.clone();
                                move |_| {
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
                                ctx.ask(Modal {
                                    title: format!("Forget {}?", plural(ids.len(), "book")),
                                    body: "The books and your places in them are deleted from this device. This cannot be undone.".into(),
                                    confirm: "Forget".into(), cancel: "Keep".into(), danger: true,
                                    action: ModalAction::Forget(ids.clone()),
                                });
                                picked.set(BTreeSet::new());
                            }
                        },
                        Icon { name: "trash", size: 15 } span { class: "grow", "Forget" } span { class: "menu-help", onclick: move |e| e.stop_propagation(), Help { text: FORGET_HELP } }
                    }
                },
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

/// A card was clicked: its overview, with the chapters and what can be done with the book.
fn open_book(ctx: Ctx, id: &str) {
    if ctx.book(id).is_some() {
        super::book::open_overview(ctx, id);
    }
}
