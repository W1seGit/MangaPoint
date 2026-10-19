//! Narrowing the results by type, explicit content and genre, and looking up the genres.

use crate::logic::{ExplicitFilter, SearchFilters, COMMON_GENRES, SERIES_TYPES};
use crate::state::spawn_forever;
use crate::state::AppState;
use dioxus::prelude::*;
use futures_util::StreamExt;
use std::time::Duration;
use super::place_popover;

/// How many results get their genres looked up.
pub(super) const TAG_LOOKUPS: usize = 150;

/// Look up the genres of some search results (a few at a time), so the type
/// and genre filters can tell them apart. A series whose details cannot be
/// read is kept with none, so it is not asked for again.
pub(super) fn fetch_tags(state: AppState, wanted: Vec<(String, i64)>) {
    static ASKED: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());
    let Some(client) = state.client() else { return };
    let todo: Vec<(String, i64)> = {
        let Ok(mut asked) = ASKED.lock() else { return };
        wanted
            .into_iter()
            .filter(|k| {
                if asked.contains(k) {
                    false
                } else {
                    asked.push(k.clone());
                    true
                }
            })
            .collect()
    };
    if todo.is_empty() {
        return;
    }
    spawn_forever(async move {
        futures_util::stream::iter(todo)
            .for_each_concurrent(6, |key| {
                let client = client.clone();
                async move {
                    let tags = match tokio::time::timeout(
                        Duration::from_secs(20),
                        client.manga_details(key.1),
                    )
                    .await
                    {
                        Ok(Ok(details)) => details.genre,
                        _ => Vec::new(),
                    };
                    let mut all = state.search_tags;
                    all.write().insert(key, tags);
                }
            })
            .await;
    });
}

fn set_filters(state: AppState, filters: SearchFilters) {
    let mut f = state.search_filters;
    f.set(filters);
}

/// Add a word to a list, or take it out if it is there.
fn toggled(list: &[String], word: &str) -> Vec<String> {
    let mut next: Vec<String> = list.to_vec();
    match next.iter().position(|w| w.eq_ignore_ascii_case(word)) {
        Some(at) => {
            next.remove(at);
        }
        None => next.push(word.to_string()),
    }
    next
}

/// The panel for narrowing the results: type, explicit content and genres.
#[component]
pub(super) fn FilterPanel(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let filters = state.search_filters.read().clone();
    let (for_all_types, for_all_genres) = (filters.clone(), filters.clone());
    let (clear_types, clear_explicit, clear_genres) = (filters.clone(), filters.clone(), filters.clone());
    rsx! {
        div { class: "popover filter-panel", onmounted: move |_| place_popover(),
            div { class: "filter-body",
            div { class: "filter-group first",
                div { class: "filter-head",
                    span { class: "filter-label", "Type" }
                    button { class: "filter-clear", r#type: "button", disabled: filters.types.is_empty(),
                        onclick: move |_| set_filters(state, SearchFilters { types: Vec::new(), ..clear_types.clone() }),
                        "Clear"
                    }
                }
                div { class: "filter-pills",
                    button { r#type: "button", class: if filters.types.is_empty() { "pill small on" } else { "pill small" },
                        onclick: move |_| set_filters(state, SearchFilters { types: Vec::new(), ..for_all_types.clone() }),
                        "All"
                    }
                    for t in SERIES_TYPES {
                        {
                            let next = SearchFilters { types: toggled(&filters.types, t), ..filters.clone() };
                            let on = filters.types.iter().any(|x| x.eq_ignore_ascii_case(t));
                            rsx! {
                                button { key: "{t}", r#type: "button", class: if on { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{t}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "filter-group",
                div { class: "filter-head",
                    span { class: "filter-label", "Explicit content" }
                    button { class: "filter-clear", r#type: "button", disabled: filters.explicit == ExplicitFilter::Show,
                        onclick: move |_| set_filters(state, SearchFilters { explicit: ExplicitFilter::Show, ..clear_explicit.clone() }),
                        "Clear"
                    }
                }
                div { class: "filter-pills",
                    for (choice, label) in [(ExplicitFilter::Show, "Show"), (ExplicitFilter::Hide, "Hide"), (ExplicitFilter::Only, "Only")] {
                        {
                            let next = SearchFilters { explicit: choice, ..filters.clone() };
                            rsx! {
                                button { key: "{label}", r#type: "button", class: if filters.explicit == choice { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "filter-group",
                div { class: "filter-head",
                    span { class: "filter-label", "Genres" }
                    button { class: "filter-clear", r#type: "button", disabled: filters.genres.is_empty(),
                        onclick: move |_| set_filters(state, SearchFilters { genres: Vec::new(), ..clear_genres.clone() }),
                        "Clear"
                    }
                }
                div { class: "filter-pills",
                    button { r#type: "button", class: if filters.genres.is_empty() { "pill small on" } else { "pill small" },
                        onclick: move |_| set_filters(state, SearchFilters { genres: Vec::new(), ..for_all_genres.clone() }),
                        "All"
                    }
                    for g in COMMON_GENRES {
                        {
                            let next = SearchFilters { genres: toggled(&filters.genres, g), ..filters.clone() };
                            let on = filters.genres.iter().any(|x| x.eq_ignore_ascii_case(g));
                            rsx! {
                                button { key: "{g}", r#type: "button", class: if on { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{g}"
                                }
                            }
                        }
                    }
                }
            }
            }
            div { class: "popover-foot",
                button { class: "btn primary small", r#type: "button", onclick: move |_| on_close.call(()), "Done" }
            }
        }
    }
}
