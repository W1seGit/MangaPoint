//! The Library: the series you keep (downloaded or saved for later). Exported
//! files are listed on the Downloads page.

use super::widgets::{open_folder, Chip, Cover, EmptyState, Icon, NoCover};
use super::work::{
    ensure_series_info, fill_chapter_totals, load_saved_cover, open_export, open_plan, open_series_from_library,
    saved_cover_key,
};
use crate::logic::{human_bytes, library_order_read, plural, read_button, LibrarySort, LibraryView};
use crate::state::{library_files, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;
use std::collections::HashSet;

#[component]
pub fn LibraryPage() -> Element {
    let state = use_context::<AppState>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    let mut picked = use_signal(HashSet::<String>::new);
    let mut more = use_signal(|| false);
    // The poster whose menu is open.
    let mut menu = use_signal(|| None::<String>);
    // Fetch the details and covers of series that do not have them yet, and
    // show the saved covers. Runs again when the catalog comes up or the
    // library changes.
    use_effect(move || {
        let _catalog_ready = state.services.read().is_some();
        // How many chapters each series has in all, looked up in the background.
        fill_chapter_totals(state);
        let series: Vec<(String, bool, bool)> = state
            .library
            .read()
            .series
            .iter()
            .filter(|s| !s.is_sample())
            .map(|s| {
                (
                    s.id.clone(),
                    s.info.is_some(),
                    s.info.as_ref().is_some_and(|i| i.has_cover),
                )
            })
            .collect();
        for (id, has_info, has_cover) in series {
            if !has_info {
                ensure_series_info(state, &id);
            } else if has_cover {
                load_saved_cover(state, &id);
            }
        }
    });
    // Read in place: copying the whole library for every change to it was
    // costly once it was big.
    let db = state.library.read();
    let series: Vec<&library::DownloadedSeries> =
        db.series.iter().filter(|s| !s.is_sample()).collect();
    let total: u64 = series.iter().map(|s| s.bytes()).sum();
    let exported: HashSet<String> = state
        .exports
        .read()
        .records
        .iter()
        .map(|r| r.series_id.clone())
        .collect();
    let order = library_order_read(&series, view(), sort(), &filter.read(), &exported, &|id| {
        db.reading_of(id).map_or(0, |p| p.last_read_at)
    });
    let downloaded_count = series
        .iter()
        .filter(|s| !s.chapters.is_empty())
        .count();
    let saved_count = series.len() - downloaded_count;
    // Which series have work, and nothing of how far it has got: the posters
    // are not drawn again for every page of a download.
    let working_series = use_memo(move || state.queue.read().series_with_work())();

    // The ticked series that are still shown, in the order shown.
    let chosen: Vec<&library::DownloadedSeries> = order
        .iter()
        .map(|&i| series[i])
        .filter(|s| picked.read().contains(&s.id))
        .collect();
    let chosen_ids: Vec<String> = chosen.iter().map(|s| s.id.clone()).collect();
    let chosen_with_downloads: Vec<String> = chosen
        .iter()
        .filter(|s| !s.chapters.is_empty())
        .map(|s| s.id.clone())
        .collect();
    let chosen_bytes: u64 = chosen.iter().map(|s| s.bytes()).sum();
    let selecting = !chosen.is_empty();
    let all_shown: Vec<String> = order
        .iter()
        .map(|&i| series[i].id.clone())
        .collect();
    let all_ticked = selecting && chosen.len() == all_shown.len();

    let remove_modal = Modal {
        title: format!(
            "Remove {} from your library?",
            plural(chosen.len(), "series")
        ),
        body: if chosen_with_downloads.is_empty() {
            "They will be taken off your list. Nothing is stored on this computer for them."
                .to_string()
        } else {
            format!(
                "Their downloaded chapters ({}) will be deleted from this computer too. Files you already exported are kept. To keep them in the library, delete only the downloads instead.",
                human_bytes(chosen_bytes)
            )
        },
        confirm: "Remove".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::RemoveSeries {
            series_ids: chosen_ids.clone(),
        },
    };
    let delete_modal = Modal {
        title: format!(
            "Delete the downloads of {}?",
            plural(chosen_with_downloads.len(), "series")
        ),
        body: format!(
            "The downloaded chapters ({}) will be deleted from this computer. The series stay in your library, so you can download them again later. Files you already exported are kept.",
            human_bytes(chosen_bytes)
        ),
        confirm: "Delete downloads".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteDownloads {
            series_ids: chosen_with_downloads.clone(),
        },
    };

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Library" }
                p { class: "muted",
                    if series.is_empty() {
                        "Series you keep, to read now or later."
                    } else {
                        "{downloaded_count} downloaded · {human_bytes(total)} on this computer"
                        if saved_count > 0 { " · {saved_count} saved for later" }
                    }
                }
            }
            button {
                class: "btn", title: "Open the folder the downloads are kept in",
                onclick: move |_| {
                    let root = library_files().root().to_path_buf();
                    let _ = std::fs::create_dir_all(&root);
                    open_folder(&root);
                },
                Icon { name: "folder-open", size: 16 } "Open folder"
            }
        }

        if series.is_empty() {
            EmptyState {
                title: "Your library is empty",
                message: "Find a series and add it to your library. You can download its chapters now or whenever you like.",
                shelf: true,
                button { class: "btn primary", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
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
                    class: "lib-filter", r#type: "search", placeholder: "Filter your library",
                    value: "{filter}", oninput: move |e| filter.set(e.value()),
                }
                select {
                    class: "lib-sort", value: "{sort().key()}",
                    onchange: move |e| sort.set(LibrarySort::from_key(&e.value())),
                    for o in LibrarySort::ALL {
                        option { key: "{o.key()}", value: "{o.key()}", selected: sort() == o, "{o.label()}" }
                    }
                }
            }
            if order.is_empty() {
                if !filter.read().trim().is_empty() {
                    EmptyState {
                        title: "Nothing matches",
                        message: "No series in this view has that in its name. Try fewer letters.",
                        compact: true,
                    }
                } else if view() == LibraryView::Exported {
                    EmptyState {
                        title: "Nothing exported yet",
                        message: "Series you export for your reader show up here.",
                        shelf: true,
                    }
                } else if view() == LibraryView::Saved {
                    EmptyState {
                        title: "Nothing saved for later",
                        message: "Everything in your library is already downloaded. Add a series from Search without downloading it and it waits here.",
                        shelf: true,
                    }
                } else {
                    EmptyState {
                        title: "Nothing downloaded yet",
                        message: "Open a series and download chapters; they will be listed here.",
                        shelf: true,
                    }
                }
            }
            div { class: if selecting { "poster-grid selecting" } else { "poster-grid" },
                for &i in order.iter() {
                    {
                        let s = series[i];
                        let open_id = s.id.clone();
                        let tick_id = s.id.clone();
                        let on = picked.read().contains(&s.id);
                        let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                        let cover_key = saved_cover_key(&s.id);
                        let chapters = s.chapters.len();
                        let size = human_bytes(s.bytes());
                        let sub = match (chapters, s.chapter_total) {
                            (0, Some(total)) => format!("{} · not downloaded", plural(total, "chapter")),
                            (0, None) => "Not downloaded yet".to_string(),
                            (done, Some(total)) if total >= done => format!("{done} of {total} chapters · {size}"),
                            _ => format!("{} · {size}", plural(chapters, "chapter")),
                        };
                        let about = s
                            .info
                            .as_ref()
                            .map(|i| {
                                let mut parts: Vec<String> = Vec::new();
                                if !i.status.is_empty() {
                                    parts.push(i.status.clone());
                                }
                                parts.extend(i.genres.iter().take(2).cloned());
                                parts.join(" · ")
                            })
                            .unwrap_or_default();
                        let is_exported = exported.contains(&s.id);
                        // "Read" (or "Continue · Ch. N" with how far) for a series with pages.
                        let reading = read_button(s, db.reading_of(&s.id));
                        let read_id = s.id.clone();
                        let queued = working_series.contains(&s.id);
                        let menu_id = s.id.clone();
                        let text_id = s.id.clone();
                        let menu_open = menu().as_deref() == Some(s.id.as_str());
                        let (export_id, plan_id, view_id, delete_id, remove_id) = (
                            s.id.clone(),
                            s.id.clone(),
                            s.id.clone(),
                            s.id.clone(),
                            s.id.clone(),
                        );
                        let delete_modal = Modal {
                            title: format!("Delete the downloads of {}?", s.title),
                            body: format!(
                                "The downloaded chapters ({size}) will be deleted from this computer. The series stays in your library, so you can download it again later. Files you already exported are kept."
                            ),
                            confirm: "Delete downloads".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::DeleteDownloads { series_ids: vec![delete_id] },
                        };
                        let remove_modal = Modal {
                            title: format!("Remove {} from your library?", s.title),
                            body: if chapters == 0 {
                                "It will be taken off your list. Nothing is stored on this computer for it.".to_string()
                            } else {
                                format!("Its downloaded chapters ({size}) will be deleted from this computer too. Files you already exported are kept.")
                            },
                            confirm: "Remove".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::RemoveSeries { series_ids: vec![remove_id] },
                        };
                        rsx! {
                            div { class: if on { "poster on" } else { "poster" }, key: "{s.id}",
                                div { class: "poster-art", title: "Open {s.title}",
                                    onclick: move |_| {
                                        if picked.read().is_empty() {
                                            open_series_from_library(state, &open_id);
                                        } else {
                                            let mut p = picked.write();
                                            if !p.remove(&open_id) { p.insert(open_id.clone()); }
                                        }
                                    },
                                    if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
                                    if queued {
                                        span { class: "poster-badge queued", title: "Downloading",
                                            Icon { name: "download", size: 12 }
                                        }
                                    }
                                    button {
                                        class: if on { "poster-check on" } else { "poster-check" },
                                        title: if on { "Deselect" } else { "Select" },
                                        onclick: move |e| {
                                            e.stop_propagation();
                                            let mut p = picked.write();
                                            if !p.remove(&tick_id) { p.insert(tick_id.clone()); }
                                        },
                                        if on { Icon { name: "check", size: 14 } }
                                    }
                                    button {
                                        class: if menu_open { "poster-more open" } else { "poster-more" },
                                        title: "More actions",
                                        onclick: move |e| {
                                            e.stop_propagation();
                                            menu.set(if menu_open { None } else { Some(menu_id.clone()) });
                                        },
                                        Icon { name: "ellipsis", size: 16 }
                                    }
                                }
                                if menu_open {
                                    div { class: "menu-scrim", onclick: move |_| menu.set(None) }
                                    div { class: "menu down",
                                        button { class: "menu-item",
                                            onclick: move |_| { menu.set(None); open_series_from_library(state, &view_id); },
                                            Icon { name: "book-open", size: 15 } "Open"
                                        }
                                        if chapters > 0 {
                                            button { class: "menu-item",
                                                onclick: move |_| { menu.set(None); open_export(state, vec![export_id.clone()]); },
                                                Icon { name: "file-down", size: 15 } "Export…"
                                            }
                                        }
                                        button { class: "menu-item",
                                            onclick: move |_| { menu.set(None); open_plan(state, vec![plan_id.clone()]); },
                                            Icon { name: "download", size: 15 } "Download chapters…"
                                        }
                                        if chapters > 0 {
                                            button { class: "menu-item danger",
                                                onclick: move |_| { menu.set(None); state.ask(delete_modal.clone()); },
                                                Icon { name: "trash", size: 15 } "Delete downloads"
                                            }
                                        }
                                        button { class: "menu-item danger",
                                            onclick: move |_| { menu.set(None); state.ask(remove_modal.clone()); },
                                            Icon { name: "x", size: 15 } "Remove from library"
                                        }
                                    }
                                }
                                // The text opens the series too, and lights up with the cover.
                                div { class: "poster-text", title: "Open {s.title}",
                                    onclick: move |_| {
                                        if picked.read().is_empty() {
                                            open_series_from_library(state, &text_id);
                                        } else {
                                            let mut p = picked.write();
                                            if !p.remove(&text_id) { p.insert(text_id.clone()); }
                                        }
                                    },
                                    strong { class: "poster-title", title: "{s.title}", "{s.title}" }
                                    span { class: "muted small", "{sub}" }
                                    if !about.is_empty() {
                                        span { class: "muted small poster-about", title: "{about}", "{about}" }
                                    }
                                    div { class: "poster-tags",
                                        if chapters > 0 {
                                            Chip { text: "Downloaded", kind: Some("ok".to_string()) }
                                        } else {
                                            Chip { text: "Saved for later" }
                                        }
                                        if is_exported {
                                            Chip { text: "Exported", kind: Some("accent".to_string()) }
                                        }
                                    }
                                }
                                if let Some(reading) = reading {
                                    button {
                                        class: "btn poster-read", id: "read-{s.id}", "data-series": "{s.id}",
                                        "aria-label": "{reading.label} {s.title}", title: "Read {s.title} in MangaPoint",
                                        onclick: move |_| state.open_reader(&read_id, None),
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

        if selecting {
            div { class: "action-bar",
                div { class: "bar-count",
                    strong { "{chosen.len()} selected" }
                    span { class: "muted small", "{human_bytes(chosen_bytes)} on this computer" }
                }
                button { class: "btn ghost small", onclick: move |_| {
                        if all_ticked { picked.write().clear(); } else { picked.set(all_shown.iter().cloned().collect()); }
                    },
                    if all_ticked { "Select none" } else { "Select all" }
                }
                span { class: "grow" }
                button { class: "btn primary", onclick: {
                        let ids = chosen_ids.clone();
                        move |_| { picked.write().clear(); open_plan(state, ids.clone()); }
                    },
                    Icon { name: "download", size: 16 } "Download…"
                }
                if !chosen_with_downloads.is_empty() {
                    button { class: "btn", title: "Edit and export the downloaded chapters of the series you ticked",
                        onclick: {
                            let ids = chosen_with_downloads.clone();
                            move |_| { picked.write().clear(); open_export(state, ids.clone()); }
                        },
                        Icon { name: "file-down", size: 15 } "Export…"
                    }
                }
                div { class: "bar-menu",
                    button { class: "btn", title: "More actions", onclick: move |_| more.set(!more()),
                        Icon { name: "ellipsis", size: 18 }
                    }
                    if more() {
                        div { class: "menu-scrim", onclick: move |_| more.set(false) }
                        div { class: "menu up",
                            if !chosen_with_downloads.is_empty() {
                                button { class: "menu-item danger", title: "Delete the downloaded chapters but keep the series",
                                    onclick: move |_| { more.set(false); state.ask(delete_modal.clone()); picked.write().clear(); },
                                    Icon { name: "trash", size: 15 } "Delete downloads"
                                }
                            }
                            button { class: "menu-item danger", title: "Take the series out of your library",
                                onclick: move |_| { more.set(false); state.ask(remove_modal.clone()); picked.write().clear(); },
                                Icon { name: "x", size: 15 } "Remove from library"
                            }
                        }
                    }
                }
            }
        }
    }
}
