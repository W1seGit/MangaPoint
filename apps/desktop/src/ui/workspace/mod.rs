//! One series: its overview, and its chapters (with what is downloaded).

mod chapters;
pub(super) mod crop_editor;
mod downloads;
mod edit_info;
mod overview;
pub(super) mod preview;

use super::widgets::{cover_key, Chip, Cover, EmptyState, Icon, NoCover};
use super::work::{add_open_series_to_library, load_saved_cover, saved_cover_key};
use crate::state::{AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;

#[component]
pub fn WorkspacePage() -> Element {
    let state = use_context::<AppState>();
    let ws = state.workspace.read().clone();
    let Some(ws) = ws else {
        return rsx! {
            EmptyState {
                title: "No series open",
                message: "Pick one from the search results, or open one from your Library.",
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.go(Page::Search), Icon { name: "arrow-left" } "Back to search" }
            }
        };
    };

    let downloaded = state
        .library
        .read()
        .find(&ws.series_id)
        .map_or(0, |s| s.chapters.len());
    let library_bytes = state
        .library
        .read()
        .find(&ws.series_id)
        .map_or(0, |s| s.bytes());
    let in_library = state.library.read().find(&ws.series_id).is_some();

    let back_label = if ws.origin == Page::Library {
        "Library"
    } else {
        "Results"
    };
    let origin = ws.origin;
    // The cover from the search that found it, or the one saved with the series.
    let saved_cover = state
        .library
        .read()
        .find(&ws.series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let cover = ws
        .ranked
        .as_ref()
        .map(|r| cover_key(&r.best().candidate.source_id, r.best().candidate.manga_id))
        .or_else(|| saved_cover.then(|| saved_cover_key(&ws.series_id)));
    {
        let id = ws.series_id.clone();
        use_effect(move || load_saved_cover(state, &id));
    }
    // What the sources say about it: the first to answer with a description is used.
    let found = state
        .open_details
        .read()
        .clone()
        .filter(|d| d.series_id == ws.series_id);
    let source_chips: Vec<String> = ws
        .ranked
        .as_ref()
        .map(|r| {
            crate::logic::one_per_source(&r.members, 4)
                .iter()
                .map(|m| m.candidate.source_name.clone())
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        div { class: "rise-in",
        button { class: "btn ghost small", id: "series-back", onclick: move |_| state.close_series(origin),
            Icon { name: "arrow-left", size: 16 }
            "{back_label}"
        }
        div { class: "spacer" }
        if !in_library { div { class: "overview found",
            div { class: "overview-cover",
                if let Some(src) = found.as_ref().and_then(|d| d.cover.clone()) {
                    img { class: "cover", src: "{src}", decoding: "async" }
                } else if let Some(key) = cover { Cover { key_id: key } } else { NoCover {} }
            }
            div { class: "overview-body",
                div { class: "overview-title",
                    h1 { "{ws.title}" }
                    SeriesActions { series_id: ws.series_id.clone(), title: ws.title.clone() }
                }
                div { class: "row wrap",
                    for (i, name) in source_chips.iter().enumerate() {
                        Chip {
                            key: "{i}",
                            text: if i == 0 { format!("{name} · best") } else { name.clone() },
                            kind: if i == 0 { Some("accent".to_string()) } else { None },
                        }
                    }
                    if downloaded > 0 {
                        Chip { text: format!("{downloaded} downloaded · {}", crate::logic::human_bytes(library_bytes)), kind: Some("ok".to_string()) }
                    }
                }
                if let Some(d) = &found {
                    if !d.info.author.is_empty() || !d.info.status.is_empty() {
                        p { class: "muted", style: "margin: 10px 0 0",
                            if !d.info.author.is_empty() { "By {d.info.author}" }
                            if !d.info.author.is_empty() && !d.info.status.is_empty() { " · " }
                            "{d.info.status}"
                        }
                    }
                    if !d.info.genres.is_empty() {
                        div { class: "row wrap", style: "margin-top: 10px",
                            for g in d.info.genres.iter() { Chip { key: "{g}", text: g.clone() } }
                        }
                    }
                    if !d.info.description.is_empty() {
                        p { class: "description clamp", style: "margin-top: 12px", "{d.info.description}" }
                    }
                }
            }
        } }

        // The overview of a series in your library, and below it every chapter.
        if in_library { overview::OverviewTab {} }
        chapters::ChaptersTab {}
        }
    }
}

/// Add to the library, take out of it, and export: the buttons of a series.
#[component]
fn SeriesActions(series_id: String, title: String) -> Element {
    let state = use_context::<AppState>();
    let (downloaded, bytes) = state
        .library
        .read()
        .find(&series_id)
        .map_or((0, 0), |s| (s.chapters.len(), s.bytes()));
    let in_library = state.library.read().find(&series_id).is_some();
    let body = if downloaded > 0 {
        format!(
            "{title} and its {downloaded} downloaded chapters ({}) will be deleted from this computer. Files you already exported are kept. To keep it in the library, delete just the chapters from the list.",
            crate::logic::human_bytes(bytes)
        )
    } else {
        format!("{title} will be taken off your list. Nothing is stored on this computer for it.")
    };
    let remove_modal = Modal {
        title: format!("Remove {title} from your library?"),
        body,
        confirm: "Remove".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::RemoveSeries {
            series_ids: vec![series_id.clone()],
        },
    };
    rsx! {
        div { class: "row wrap series-actions",
            if in_library {
                button { class: "btn", title: "Remove it from your library",
                    onclick: move |_| state.ask(remove_modal.clone()),
                    Icon { name: "bookmark-check", size: 16 } "In your library"
                }
            } else {
                button { class: "btn primary", title: "Keep it in your library without downloading anything",
                    onclick: move |_| add_open_series_to_library(state),
                    Icon { name: "bookmark-plus", size: 16 } "Add to library"
                }
            }
        }
    }
}
