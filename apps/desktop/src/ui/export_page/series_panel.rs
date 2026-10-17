//! The Series panel: the series being exported together, and the list to add another from.

use super::Panel;
use crate::logic::{self, plural};
use crate::state::AppState;
use crate::ui::widgets::{Cover, Icon, NoCover};
use crate::ui::work::{activate_export_series, add_export_series, load_saved_cover, remove_export_series, saved_cover_key};
use dioxus::prelude::*;
use library::DownloadedSeries;

#[component]
pub(super) fn SeriesPanel(on_add: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    rsx! {
        Panel {
            title: "Series",
            icon: "book-open",
            id: "series",
            height: 150,
            actions: rsx! {
                button { class: "btn ghost small", title: "Add another series to this export", onclick: move |_| on_add.call(()),
                    Icon { name: "plus", size: 14 } "Add"
                }
            },
            div { class: "px-list",
                for (i, item) in plan.items.iter().enumerate() {
                    if let Some(series) = library.find(&item.series_id) {
                        {
                        let included = series.chapters.iter().filter(|c| !item.excluded.contains(&c.key)).count();
                        let has_cover = series.info.as_ref().is_some_and(|i| i.has_cover);
                        let on = i == plan.active;
                        let off = item.off;
                        rsx! {
                            div {
                                key: "{item.series_id}",
                                class: if on { "px-layer on" } else { "px-layer" },
                                onclick: move |_| { if !on { activate_export_series(state, i); } },
                                input {
                                    r#type: "checkbox", checked: !off, title: "Include this series in the export",
                                    onclick: move |e| e.stop_propagation(),
                                    onchange: move |e| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(i) { item.off = !e.checked(); }
                                    },
                                }
                                div { class: "px-layer-cover",
                                    if has_cover { Cover { key_id: saved_cover_key(&series.id) } } else { NoCover {} }
                                }
                                div { class: "grow px-layer-text",
                                    strong { title: "{series.title}", "{series.title}" }
                                    span { class: "muted small", "{included} of {series.chapters.len()} chapters" }
                                }
                                button {
                                    class: "btn ghost small", title: "Take it out of this export",
                                    onclick: move |e| { e.stop_propagation(); remove_export_series(state, i); },
                                    Icon { name: "x", size: 14 }
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

#[component]
pub(super) fn AddSeries(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    let available: Vec<&DownloadedSeries> = library
        .series
        .iter()
        .filter(|s| !s.chapters.is_empty() && !s.is_sample())
        .filter(|s| !plan.items.iter().any(|i| i.series_id == s.id))
        .collect();
    // Their covers are on disk, small: read them as soon as the list opens.
    let ids: Vec<String> = available.iter().map(|s| s.id.clone()).collect();
    use_effect(move || {
        for id in &ids {
            load_saved_cover(state, id);
        }
    });
    rsx! {
        div { class: "modal-head",
            h2 { "Add a series to this export" }
            button { class: "btn ghost small", title: "Close", onclick: move |_| on_close.call(()), Icon { name: "x", size: 16 } }
        }
        if available.is_empty() {
            p { class: "muted", "Every downloaded series is already in this export." }
        }
        for s in available {
            {
                let id = s.id.clone();
                let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                rsx! {
                    button { key: "{s.id}", class: "px-add-row",
                        onclick: move |_| { add_export_series(state, id.clone()); on_close.call(()); },
                        div { class: "px-layer-cover",
                            if has_cover { Cover { key_id: saved_cover_key(&s.id) } } else { NoCover {} }
                        }
                        div { class: "grow px-layer-text",
                            strong { "{s.title}" }
                            span { class: "muted small", "{plural(s.chapters.len(), \"chapter\")} · {logic::human_bytes(s.bytes())}" }
                        }
                        Icon { name: "plus", size: 16 }
                    }
                }
            }
        }
        div { class: "actions",
            button { class: "btn primary", onclick: move |_| on_close.call(()), "Done" }
        }
    }
}
