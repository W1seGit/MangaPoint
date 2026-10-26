//! The Library: the series you keep (downloaded or saved for later). Exported
//! files are listed on the Downloads page.

use super::widgets::{open_folder, Cover, EmptyState, Icon, NoCover};
use super::work::{
    start_import, ensure_series_info, fill_chapter_totals, load_saved_cover, open_export, open_plan, open_series_from_library,
    saved_cover_key,
};
use crate::logic::{human_bytes, library_order_read, plural, read_button, LibrarySort, LibraryView};
use crate::state::{library_files, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;
use mangapoint_ui::library::{ActionBar, use_fading, InfoDialog, InfoFile, InfoSheet, LibraryTools, Poster, PosterButton, PosterFacts, PosterTabs};
use std::collections::HashSet;

#[component]
pub fn LibraryPage() -> Element {
    let state = use_context::<AppState>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    let mut picked = use_signal(HashSet::<String>::new);
    // The menu of the Import button.
    let mut import_menu = use_signal(|| false);
    // The poster whose menu is open.
    let mut menu = use_signal(|| None::<String>);
    // The card whose Information is open.
    let mut info = use_signal(|| None::<InfoSheet>);
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
    // The chapters of each series that went into an export (an old record that did not keep which
    // ones counts as the chapters it holds).
    let exported_chapters: std::collections::HashMap<String, usize> = {
        let mut by_series: std::collections::HashMap<String, (HashSet<String>, usize)> = std::collections::HashMap::new();
        for r in state.exports.read().records.iter() {
            let entry = by_series.entry(r.series_id.clone()).or_default();
            entry.0.extend(r.chapter_keys.iter().cloned());
            if r.chapter_keys.is_empty() {
                entry.1 = entry.1.max(r.chapters);
            }
        }
        by_series.into_iter().map(|(id, (keys, old))| (id, keys.len().max(old))).collect()
    };
    let order = library_order_read(&series, view(), sort(), &filter.read(), &exported, &|id| {
        db.reading_of(id).map_or(0, |p| p.last_read_at)
    });
    // The cards in view; one that drops out (another view, a filter) fades before it goes.
    let (fading_rows, fading) = use_fading(&order.iter().map(|&i| series[i].id.clone()).collect::<Vec<_>>());
    let rows: Vec<(usize, bool)> =
        fading_rows.iter().filter_map(|(id, l)| series.iter().position(|s| &s.id == id).map(|i| (i, *l))).collect();
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
    // Imported series have nothing to download, and their pages could not be downloaded again.
    let chosen_downloadable: Vec<String> = chosen.iter().filter(|s| !s.is_imported()).map(|s| s.id.clone()).collect();
    let chosen_downloaded: Vec<&library::DownloadedSeries> =
        chosen.iter().copied().filter(|s| !s.is_imported() && !s.chapters.is_empty()).collect();
    let chosen_downloaded_ids: Vec<String> = chosen_downloaded.iter().map(|s| s.id.clone()).collect();
    let downloaded_bytes: u64 = chosen_downloaded.iter().map(|s| s.bytes()).sum();
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
            plural(chosen_downloaded_ids.len(), "series")
        ),
        body: format!(
            "The downloaded chapters ({}) will be deleted from this computer. The series stay in your library, so you can download them again later. Files you already exported are kept.",
            human_bytes(downloaded_bytes)
        ),
        confirm: "Delete downloads".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteDownloads {
            series_ids: chosen_downloaded_ids.clone(),
        },
    };

    rsx! {
        // The title and the views stay at the top while the cards scroll under them.
        div { class: "lib-top",
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
                div { class: "menu-anchor import-anchor",
                    button {
                        class: "btn", title: "Bring in comics, books and pictures you already have",
                        onclick: move |_| import_menu.set(!import_menu()),
                        Icon { name: "file-up", size: 16 } "Import"
                    }
                    if import_menu() {
                        div { class: "menu-scrim", onclick: move |_| import_menu.set(false) }
                        div { class: "menu down",
                            button { class: "menu-item",
                                onclick: move |_| { import_menu.set(false); start_import(state, None, false); },
                                Icon { name: "file-up", size: 15 } "Choose files…"
                            }
                            button { class: "menu-item",
                                onclick: move |_| { import_menu.set(false); start_import(state, None, true); },
                                Icon { name: "folder-open", size: 15 } "Choose a folder…"
                            }
                            div { class: "menu-note muted small", "Or drop files and folders anywhere on this page." }
                        }
                    }
                }
            }

            if !series.is_empty() {
                LibraryTools {
                    views: LibraryView::ALL.iter().map(|v| (v.label(), *v == view())).collect(),
                    on_view: move |i| view.set(LibraryView::ALL[i]),
                    filter: filter(),
                    on_filter: move |text| filter.set(text),
                    sorts: LibrarySort::ALL.iter().map(|o| (o.key(), o.label(), *o == sort())).collect(),
                    on_sort: move |key: String| sort.set(LibrarySort::from_key(&key)),
                }
            }
        }

        if series.is_empty() {
            EmptyState {
                title: "Your library is empty",
                message: "Find a series and add it to your library. You can download its chapters now or whenever you like.",
                shelf: true,
                button { class: "btn primary", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
                button { class: "btn", onclick: move |_| start_import(state, None, false), Icon { name: "file-up", size: 16 } "Import files" }
            }
        } else {
            if order.is_empty() && rows.is_empty() {
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
            div {
                class: if selecting { "poster-grid selecting" } else { "poster-grid" },
                onmounted: move |_| state.ui.host().run_script("flip_grid", String::new()),
                // Each card is a keyed root of its own: with a block in between the list was matched by place,
                // and a card that was left went on to show the next one's cover for a moment.
                {rows.iter().copied().map(|(i, leaving)| {
                        let s = series[i];
                        let gone_id = s.id.clone();
                        let on_gone = { let f = fading.clone(); move |_| f.gone(&gone_id) };
                        let open_id = s.id.clone();
                        let tick_id = s.id.clone();
                        let on = picked.read().contains(&s.id);
                        let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                        let cover_key = saved_cover_key(&s.id);
                        let chapters = s.chapters.len();
                        let size = human_bytes(s.bytes());
                        // What is downloaded of the series, and what it takes on this computer.
                        let have = match s.chapter_total {
                            Some(total) if total >= chapters => format!("{chapters} of {total}"),
                            _ => chapters.to_string(),
                        };
                        let stored = if chapters == 0 { "-".to_string() } else { size.clone() };
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
                        let has_pages = reading.is_some();
                        let queued = working_series.contains(&s.id);
                        let menu_id = s.id.clone();
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
                        let mut lines = Vec::new();
                        if !about.is_empty() {
                            lines.push(("poster-about", about.clone()));
                        }
                        let status = if s.is_imported() { "Imported" } else if chapters > 0 { "Downloaded" } else { "Saved for later" };
                        lines.push(("poster-status", format!("{status}{}", if is_exported { " · Exported" } else { "" })));
                        let sheet = {
                            let records = state.exports.read();
                            let mut facts = vec![
                                ("Downloaded".to_string(), if chapters == 0 { "Nothing yet".to_string() } else { format!("{} · {size}", plural(chapters, "chapter")) }),
                                ("Exported".to_string(), format!("{} of {chapters}", exported_chapters.get(&s.id).copied().unwrap_or(0).min(chapters))),
                            ];
                            if let Some(total) = s.chapter_total {
                                facts.insert(0, ("Chapters in the series".to_string(), total.to_string()));
                            }
                            let mut files = Vec::new();
                            for r in records.records.iter().rev().filter(|r| r.series_id == s.id) {
                                for f in &r.files {
                                    let path = std::path::Path::new(&f.path);
                                    let name = path.file_name().map_or_else(|| f.path.clone(), |n| n.to_string_lossy().into_owned());
                                    let place = path.parent().map_or_else(|| r.target.clone(), |p| p.display().to_string());
                                    files.push(InfoFile {
                                        name,
                                        note: format!("{} · {} · {}", r.format.to_uppercase(), plural(r.chapters, "chapter"), human_bytes(f.bytes)),
                                        place,
                                    });
                                }
                            }
                            InfoSheet { title: s.title.clone(), facts, files: Some(files) }
                        };
                        let facts = PosterFacts {
                            id: s.id.clone(),
                            title: s.title.clone(),
                            tabs: Some(PosterTabs {
                                downloaded: format!("{chapters}/{}", s.chapter_total.filter(|t| *t >= chapters).unwrap_or(chapters)),
                                exported: Some(exported_chapters.get(&s.id).copied().unwrap_or(0).min(chapters))
                                    .filter(|n| *n > 0)
                                    .map(|n| format!("{n}/{chapters}")),
                            }),
                            stats: Some((have.clone(), stored.clone())),
                            lines,
                            button: if reading.is_none() && !s.is_imported() {
                                // Nothing downloaded yet: the button downloads.
                                Some(PosterButton {
                                    id: format!("get-{}", s.id),
                                    aria: format!("Download chapters of {}", s.title),
                                    label: "Download".to_string(),
                                    icon: "download",
                                    title: format!("Choose chapters of {} to download", s.title),
                                    fraction: None,
                                })
                            } else { reading.map(|r| PosterButton {
                                id: format!("read-{}", s.id),
                                aria: format!("{} {}", r.label, s.title),
                                label: r.label,
                                icon: "book-open",
                                title: format!("Read {} in MangaPoint", s.title),
                                fraction: r.fraction,
                            }) },
                            not_installed: false,
                        };
                        rsx! {
                            Poster { key: "{s.id}", facts, selected: on, selecting, menu_open, leaving, on_gone,
                                art: rsx! {
                                    if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
                                    if queued {
                                        span { class: "poster-badge queued", title: "Downloading",
                                            Icon { name: "download", size: 12 }
                                        }
                                    }
                                },
                                menu: rsx! {
                                    button { class: "menu-item",
                                        onclick: move |_| open_series_from_library(state, &view_id),
                                        Icon { name: "book-open", size: 15 } "Open"
                                    }
                                    button { class: "menu-item",
                                        onclick: move |_| info.set(Some(sheet.clone())),
                                        Icon { name: "info", size: 15 } "Information"
                                    }
                                    if chapters > 0 {
                                        button { class: "menu-item",
                                            onclick: move |_| open_export(state, vec![export_id.clone()]),
                                            Icon { name: "file-down", size: 15 } "Export…"
                                        }
                                    }
                                    if !s.is_imported() {
                                        button { class: "menu-item",
                                            onclick: move |_| open_plan(state, vec![plan_id.clone()]),
                                            Icon { name: "download", size: 15 } "Download chapters…"
                                        }
                                    }
                                    if chapters > 0 && !s.is_imported() {
                                        button { class: "menu-item danger",
                                            onclick: move |_| state.ask(delete_modal.clone()),
                                            Icon { name: "trash", size: 15 } "Delete downloads"
                                        }
                                    }
                                    button { class: "menu-item danger",
                                        onclick: move |_| state.ask(remove_modal.clone()),
                                        Icon { name: "x", size: 15 } "Remove from library"
                                    }
                                },
                                on_open: move |_| open_series_from_library(state, &open_id),
                                on_toggle: move |_| {
                                    let mut p = picked.write();
                                    if !p.remove(&tick_id) { p.insert(tick_id.clone()); }
                                },
                                on_menu: move |open: bool| menu.set(open.then(|| menu_id.clone())),
                                on_button: move |_| {
                                    if has_pages { state.open_reader(&read_id, None) } else { open_plan(state, vec![read_id.clone()]) }
                                },
                            }
                        }
                })}
            }
        }

        if let Some(sheet) = info() {
            InfoDialog { sheet, on_close: move |_| info.set(None) }
        }
        if selecting {
            ActionBar {
                count: chosen.len(),
                size: format!("{} on this computer", human_bytes(chosen_bytes)),
                all_ticked,
                on_all: move |all: bool| {
                    if all { picked.set(all_shown.iter().cloned().collect()); } else { picked.write().clear(); }
                },
                buttons: rsx! {
                    if !chosen_downloadable.is_empty() {
                        button { class: "btn primary", onclick: {
                                let ids = chosen_downloadable.clone();
                                move |_| { picked.write().clear(); open_plan(state, ids.clone()); }
                            },
                            Icon { name: "download", size: 16 } "Download…"
                        }
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
                },
                more: rsx! {
                    if !chosen_downloaded_ids.is_empty() {
                        button { class: "menu-item danger", title: "Delete the downloaded chapters but keep the series",
                            onclick: move |_| { state.ask(delete_modal.clone()); picked.write().clear(); },
                            Icon { name: "trash", size: 15 } "Delete downloads"
                        }
                    }
                    button { class: "menu-item danger", title: "Take the series out of your library",
                        onclick: move |_| { state.ask(remove_modal.clone()); picked.write().clear(); },
                        Icon { name: "x", size: 15 } "Remove from library"
                    }
                },
            }
        }
    }
}
