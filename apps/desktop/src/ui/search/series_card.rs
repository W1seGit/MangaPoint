//! One series in the results.

use crate::ui::widgets::{cover_key, Chip, Cover, Icon};
use crate::state::AppState;
use dioxus::prelude::*;

#[component]
pub(super) fn SeriesCard(index: usize) -> Element {
    let state = use_context::<AppState>();
    let view = state.search.read();
    let Some(series) = view.ranked.get(index).cloned() else {
        return rsx! {};
    };
    drop(view);
    let best = series.best().clone();
    let cover = cover_key(&best.candidate.source_id, best.candidate.manga_id);
    let others = series.members.len() - 1;
    let chapters = best.enrichment.as_ref().and_then(|e| e.chapter_count);
    let for_click = series.clone();
    let for_save = series.clone();
    let in_library = state
        .library
        .read()
        .find(&library::series_id(&series.title))
        .is_some();
    let save_class = if in_library {
        "btn small ghost add-btn on"
    } else {
        "btn small add-btn"
    };
    let save_title = if in_library {
        "It is in your library"
    } else {
        "Keep it in your library without opening it"
    };
    let save_icon = if in_library {
        "bookmark-check"
    } else {
        "bookmark-plus"
    };
    let save_text = if in_library {
        "In library"
    } else {
        "Add to library"
    };
    rsx! {
        div { class: "series-wrap",
        button {
            class: "series-card",
            onclick: move |_| {
                crate::ui::work::open_series_from_search(state, for_click.clone());
            },
            Cover { key_id: cover }
            div { class: "meta",
                div { class: "series-title", "{series.title}" }
                div { class: "muted small", "{best.candidate.source_name}" }
                div { class: "row wrap", style: "margin-top: 6px",
                    if others > 0 {
                        Chip { text: format!("+{others} more source{}", if others == 1 { "" } else { "s" }) }
                    }
                    if let Some(n) = chapters {
                        Chip { text: format!("{n} ch.") }
                    }
                }
            }
        }
        button {
            class: save_class,
            title: save_title,
            onclick: move |_| crate::ui::work::add_ranked_to_library(state, &for_save),
            Icon { name: save_icon, size: 15 }
            "{save_text}"
        }
        }
    }
}
