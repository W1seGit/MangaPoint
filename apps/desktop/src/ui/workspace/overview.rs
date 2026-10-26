//! Tab 0: what the series is, and what you have of it.

use crate::state::AppState;
use crate::ui::widgets::{Busy, Chip, Cover, EmptyState, Icon, NoCover};
use crate::ui::work::{ensure_series_info, load_saved_cover, saved_cover_key};
use dioxus::prelude::*;

#[component]
pub fn OverviewTab() -> Element {
    let state = use_context::<AppState>();
    let mut expanded = use_signal(|| false);
    // Whether the description is cut short, measured on the page.
    let mut cut_short = use_signal(|| false);
    // Editing the details by hand (an imported series).
    let mut editing = use_signal(|| false);

    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    // Make sure the details and cover are there; both are no-ops once they are.
    {
        let id = ws.series_id.clone();
        use_effect(move || {
            ensure_series_info(state, &id);
            load_saved_cover(state, &id);
        });
    }
    let Some(series) = state.library.read().find(&ws.series_id).cloned() else {
        return rsx! {
            EmptyState {
                title: "Nothing downloaded yet",
                message: "Download a chapter and the details of this series will show up here.",
            }
        };
    };

    if editing() && series.is_imported() {
        return rsx! {
            div { class: "overview",
                div { class: "overview-body",
                    div { class: "overview-title", h1 { "Edit details" } }
                    super::edit_info::EditInfo { series_id: series.id.clone(), on_done: move |_| editing.set(false) }
                }
            }
        };
    }
    let imported = series.is_imported();
    let info = series.info.clone();
    let loading = state.info_busy.read().contains(&ws.series_id);
    let cover_key = saved_cover_key(&series.id);
    let has_cover = info.as_ref().is_some_and(|i| i.has_cover);
    let description = info
        .as_ref()
        .map(|i| i.description.clone())
        .unwrap_or_default();
    let desc_id = format!("desc-{}", series.id);
    let measure_id = desc_id.clone();
    let by_line = info
        .as_ref()
        .map(|i| match (i.author.is_empty(), i.artist.is_empty()) {
            (false, false) if i.author != i.artist => {
                format!("By {} · Art by {}", i.author, i.artist)
            }
            (false, _) => format!("By {}", i.author),
            (true, false) => format!("Art by {}", i.artist),
            (true, true) => String::new(),
        });

    rsx! {
        div { class: "overview",
            div { class: "overview-cover",
                if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
            }
            div { class: "overview-body",
                div { class: "overview-title",
                    h1 { "{series.title}" }
                    super::SeriesActions { series_id: series.id.clone(), title: series.title.clone() }
                    if imported {
                        button { class: "btn", title: "Change its name, description, author and cover",
                            onclick: move |_| editing.set(true),
                            Icon { name: "pencil", size: 16 } "Edit details"
                        }
                    }
                }
                div { class: "row wrap",
                    if imported { Chip { text: "Imported", kind: Some("accent".to_string()) } }
                    if let Some(i) = &info {
                        if !i.status.is_empty() { Chip { text: i.status.clone(), kind: Some("accent".to_string()) } }
                    }
                }
                if let Some(line) = by_line.filter(|l| !l.is_empty()) {
                    p { class: "muted", style: "margin: 10px 0 0", "{line}" }
                }
                if let Some(i) = &info {
                    if !i.genres.is_empty() {
                        div { class: "row wrap", style: "margin-top: 10px",
                            for g in i.genres.iter() { Chip { key: "{g}", text: g.clone() } }
                        }
                    }
                }
                if !description.is_empty() {
                    for key in [description.len()] {
                        p {
                            key: "{key}",
                            id: "{desc_id}",
                            class: if expanded() { "description" } else { "description clamp" },
                            onmounted: {
                                let measure_id = measure_id.clone();
                                move |_| {
                                    let measure_id = measure_id.clone();
                                    spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                                        let script = format!(
                                            "const e = document.getElementById('{measure_id}'); dioxus.send(!!e && e.scrollHeight > e.clientHeight + 1);"
                                        );
                                        let mut measure = document::eval(&script);
                                        if let Ok(cut) = measure.recv::<bool>().await {
                                            cut_short.set(cut);
                                        }
                                    });
                                }
                            },
                            "{description}"
                        }
                    }
                    if cut_short() || expanded() {
                        span { class: "read-more", onclick: move |_| expanded.set(!expanded()),
                            if expanded() { "Show less" } else { "Read more" }
                        }
                    }
                } else if imported {
                    p { class: "muted small", style: "margin-top: 12px",
                        "There is no description yet. Use Edit details to write one."
                    }
                } else if loading {
                    p { class: "muted", style: "margin-top: 12px", Busy { label: "Loading the details…" } }
                } else if info.is_none() {
                    p { class: "muted small", style: "margin-top: 12px",
                        "The details of this series haven't been loaded yet. They need the catalog to be running."
                    }
                    button { class: "btn small", onclick: move |_| ensure_series_info(state, &ws.series_id),
                        Icon { name: "refresh-cw", size: 14 } "Load details"
                    }
                }

                if series.chapters.is_empty() {
                    p { class: "muted", style: "margin: 14px 0 0",
                        "Not downloaded yet. Tick chapters below to download them."
                    }
                }
            }
        }
    }
}
