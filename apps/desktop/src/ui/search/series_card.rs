//! One series in the results.

use crate::logic::{ago, plural};
use crate::state::now_secs;
use crate::ui::widgets::{cover_key, Cover, Icon};
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
    let chapters = best.enrichment.as_ref().and_then(|e| e.chapter_count);
    // When its newest chapter came out, if the source says.
    let updated = best.enrichment.as_ref().and_then(|e| e.latest_upload_ms).filter(|ms| *ms > 0).map(|ms| ago(now_secs(), (ms / 1000) as u64));
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
                div { class: "series-facts",
                    if let Some(n) = chapters {
                        span { class: "fact", "{plural(n as usize, \"chapter\")}" }
                    }
                    if let Some(when) = updated {
                        span { class: "fact", "Updated {when}" }
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
