//! Searching every installed source at once.

mod enrich;
mod filter_panel;
mod run;
mod series_card;
mod source_panel;

pub use run::{clear_search, run_search};

use filter_panel::{fetch_tags, FilterPanel, TAG_LOOKUPS};
use series_card::SeriesCard;
use source_panel::SourcePanel;

use crate::ui::widgets::{Busy, EmptyState, EngineStarting, Icon, Loading};
use mangapoint_ui::art::design::Figure;
use crate::logic::{self, SearchFilters, SearchSort};
use crate::state::{AppState, Page};
use dioxus::prelude::*;
use sources::SourceManager;

/// A source as the search page shows it.
#[derive(Clone, PartialEq)]
struct SourceInfo {
    id: String,
    name: String,
    lang: String,
    adult: bool,
}

#[component]
pub fn SearchPage() -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(|| state.search.peek().query.clone());
    let mut open = use_signal(|| false);
    let mut filters_open = use_signal(|| false);
    let changed = use_signal(|| false);
    let view = state.search.read().clone();
    let failed_text = view.failed.join(", ");
    let sort = *state.search_sort.read();
    let order = logic::sorted_order(&view.ranked, sort);

    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.ui.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });
    let excluded = state.ui.settings.read().search_excluded_sources.clone();
    let mut sources: Vec<SourceInfo> = match &*installed.read() {
        Some(Some(items)) => items
            .iter()
            .map(|i| SourceInfo {
                id: i.source.id.clone(),
                name: i.source.name.clone(),
                lang: i.source.lang.clone(),
                adult: i.source.is_nsfw,
            })
            .collect(),
        _ => Vec::new(),
    };
    sources.sort_by_key(|a| a.name.to_lowercase());
    let total = sources.len();
    // Narrow the results by what is known of each.
    let filters = state.search_filters.read().clone();
    let all_count = order.len();
    let (order, pending) = {
        let adult: std::collections::HashSet<&str> = sources
            .iter()
            .filter(|s| s.adult)
            .map(|s| s.id.as_str())
            .collect();
        let tags = state.search_tags.read();
        let key = |i: usize| {
            let best = &view.ranked[i].best().candidate;
            (best.source_id.clone(), best.manga_id)
        };
        let kept: Vec<usize> = order
            .into_iter()
            .filter(|&i| {
                let k = key(i);
                filters.allows(tags.get(&k).map(Vec::as_slice), adult.contains(k.0.as_str()))
            })
            .collect();
        let pending = if filters.needs_tags() {
            (0..view.ranked.len().min(TAG_LOOKUPS))
                .filter(|&i| !tags.contains_key(&key(i)))
                .count()
        } else {
            0
        };
        (kept, pending)
    };
    // Types and genres are read from the details of the results. They are
    // looked up in the background as soon as a search is done, so switching
    // the filters afterwards only shows what is already loaded.
    use_effect(move || {
        if state.search.read().searching {
            return;
        }
        let wanted: Vec<(String, i64)> = state
            .search
            .read()
            .ranked
            .iter()
            .take(TAG_LOOKUPS)
            .map(|s| (s.best().candidate.source_id.clone(), s.best().candidate.manga_id))
            .filter(|k| !state.search_tags.peek().contains_key(k))
            .collect();
        if !wanted.is_empty() {
            fetch_tags(state, wanted);
        }
    });
    let filter_pills = filters.pills();
    let chosen = filters.labels().len();
    let filter_button = if chosen == 0 {
        "Filters".to_string()
    } else {
        format!("Filters · {chosen}")
    };
    let included: Vec<SourceInfo> = sources
        .iter()
        .filter(|s| !excluded.contains(&s.id))
        .cloned()
        .collect();
    let filtered = included.len() != total;
    let sources_label = if total == 0 {
        "Sources".to_string()
    } else if filtered {
        format!("{} of {total} Sources", included.len())
    } else {
        format!("{total} Sources")
    };

    // Searching again with the sources as they are now, if there is a search.
    let search_again = move || {
        let query = state.search.peek().query.clone();
        if !query.trim().is_empty() {
            run_search(state, query.trim().to_string());
        }
    };
    // Change which sources are searched.
    let set_excluded = move |list: Vec<String>| {
        let mut settings = state.ui.settings;
        settings.write().search_excluded_sources = list;
        state.save_settings();
    };
    // Closing the panel searches again if the choice changed.
    let close_panel = move || {
        let mut open = open;
        let mut changed = changed;
        open.set(false);
        if changed() {
            search_again();
        }
        changed.set(false);
    };

    // Before anything is searched the box sits in the middle of the window, under
    // the mascot. Searching moves it up and widens it (see css/desktop.css).
    let idle = view.query.is_empty() && !view.searching;
    // Until the catalog engine is up there is nothing to search with: show how
    // far it is instead of the box, which then arrives.
    if state.services.read().is_none() {
        return rsx! { EngineStarting {} };
    }
    rsx! {
      div { class: if idle { "search-stage idle arrive" } else { "search-stage arrive" },
        div { class: "search-hero",
            h1 { class: "hero-title",
                for (i, letter) in "MangaPoint".chars().enumerate() {
                    span { key: "{i}", style: "--i: {i}", "{letter}" }
                }
            }
        }
        // The search box is all there is at the top. It is a grid of two things
        // in one place: an invisible copy of the full-size box, which holds the
        // space (so nothing below moves and the pointer is never left outside
        // while the box shrinks), and the visible box, which is as small or as
        // big as it needs to be.
        div { class: if open() || filters_open() { "searchwrap open" } else { "searchwrap" },
            div { class: "searchbox searchbox-ghost", "aria-hidden": "true",
                div { class: "searchbox-input", "\u{a0}" }
                div { class: "searchbox-row",
                    div { class: "pills-flow",
                        span { class: "ghost-sort", "Sort: {sort.label()}" }
                        for label in filter_pills.iter() {
                            span { class: "ghost-pill", key: "{label}", "{label}" }
                        }
                        for source in included.iter() {
                            span { class: "ghost-pill", key: "{source.id}", "{source.name}" }
                        }
                    }
                    span { class: "search-circle" }
                }
            }
            div { class: "cardwrap",
                form {
                    class: "searchbox",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let q = text.read().trim().to_string();
                        if q.is_empty() {
                            // Searching for nothing goes back to the start.
                            clear_search(state);
                        } else {
                            run_search(state, q);
                        }
                    },
                    input {
                        class: "searchbox-input",
                        r#type: "search",
                        placeholder: "Type a Manga, Manhua, or Manwha here...",
                        value: "{text}",
                        autofocus: true,
                        oninput: move |e| text.set(e.value()),
                    }
                    div { class: "searchbox-row",
                        // The sort and the sources flow and wrap together, so a
                        // second row starts under the sort pill. Pointing anywhere
                        // at the box, typing in it, or opening the panel folds the
                        // source pills into one dropdown (see css/desktop.css).
                        div { class: "pills-flow",
                            div { class: "sort-pill",
                                select {
                                    value: "{sort.key()}",
                                    title: "How the results are ordered",
                                    onchange: move |e| { let mut s = state.search_sort; s.set(SearchSort::from_key(&e.value())); },
                                    for option_sort in SearchSort::ALL {
                                        option { key: "{option_sort.key()}", value: "{option_sort.key()}", selected: option_sort == sort, "Sort: {option_sort.label()}" }
                                    }
                                }
                                span { class: "chev", Figure { name: "chevron" } }
                            }
                            button {
                                class: if filters_open() { "pill-btn fil-btn is-open" } else { "pill-btn fil-btn" }, r#type: "button",
                                title: "Narrow the results by type, explicit content and genre",
                                onclick: move |_| {
                                    if open() { close_panel() }
                                    filters_open.set(!filters_open());
                                },
                                "{filter_button}"
                                Figure { name: "chevron" }
                            }
                            for label in filter_pills.iter() {
                                span { class: "src-pill fil-pill", key: "{label}", "{label}" }
                            }
                            button {
                                class: if open() { "pill-btn src-btn is-open" } else { "pill-btn src-btn" }, r#type: "button",
                                title: "Choose which sources to search",
                                disabled: total == 0,
                                onclick: move |_| { if open() { close_panel() } else { filters_open.set(false); open.set(true) } },
                                "{sources_label}"
                                Figure { name: "chevron" }
                            }
                            for source in included.iter() {
                                span { class: "src-pill", key: "{source.id}", "{source.name}" }
                            }
                            if total > 0 && included.is_empty() {
                                span { class: "muted small", style: "padding: 5px 4px", "No source is chosen. Point at the box and open the sources list." }
                            }
                        }
                        // Submitting the form (this button or Enter) starts the search.
                        button { class: "search-circle", r#type: "submit", title: "Search", "aria-label": "Search",
                            Figure { name: "search-button" }
                        }
                    }
                }
                if filters_open() {
                    div { class: "backdrop", onclick: move |_| filters_open.set(false) }
                    FilterPanel { on_close: move |()| filters_open.set(false) }
                }
                if open() {
                    div { class: "backdrop", onclick: move |_| close_panel() }
                    SourcePanel {
                        sources: sources.clone(),
                        excluded: excluded.clone(),
                        on_change: move |list: Vec<String>| { set_excluded(list); let mut changed = changed; changed.set(true); },
                        on_close: move |()| close_panel(),
                    }
                }
            }
        }

        if view.no_sources {
            div { class: "banner warn",
                "No sources are installed yet. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), Icon { name: "plus", size: 15 } "Add sources" }
            }
        }
        if view.none_selected {
            div { class: "banner warn",
                "Every source is turned off for searching. Use the filter button and tick the ones you want."
            }
        }
        if view.searching {
            Loading { label: format!("Searching… {} of {} sources answered", view.answered, view.total) }
        } else if view.enriching {
            p { class: "muted small", style: "margin: 6px 2px 0", "Checking chapter lists to pick the best source for each…" }
        } else if !view.query.is_empty() && view.ranked.is_empty() && !view.no_sources && !view.none_selected {
            EmptyState {
                title: "No results",
                message: format!("It found nothing for “{}”. Try another spelling, or add more sources.", view.query),
                scene: "search",
            }
        }
        if !filters.is_empty() && !view.ranked.is_empty() && !view.searching {
            div { class: "row wrap filter-note",
                span { class: "muted small", "Showing {order.len()} of {all_count} results" }
                if pending > 0 { Busy { label: "Checking the genres of {pending} more…", size: 36 } }
                button { class: "btn small ghost", onclick: move |_| { let mut f = state.search_filters; f.set(SearchFilters::default()); },
                    "Clear filters"
                }
            }
        }
        if !view.failed.is_empty() && !view.searching {
            p { class: "muted small",
                "These sources did not answer: {failed_text}. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), Icon { name: "refresh-cw", size: 15 } "Check sources" }
            }
        }

        div { class: "grid",
            for i in order.iter().copied() {
                SeriesCard { key: "{view.ranked[i].title}-{i}", index: i }
            }
        }
      }
    }
}

/// Put a dropdown just under the pill row of the search box and keep it inside
/// the window (it scrolls inside itself), so it never makes the page longer.
const PLACE_POPOVER: &str = r#"
(() => {
  const p = document.querySelector('.searchwrap .popover');
  const box = document.querySelector('.searchwrap .searchbox:not(.searchbox-ghost)');
  const row = box && box.querySelector('.sort-pill');
  if (!p || !box || !row) return;
  const top = row.getBoundingClientRect().bottom + 18;
  p.style.position = 'fixed';
  p.style.top = top + 'px';
  p.style.left = (box.getBoundingClientRect().left + 14) + 'px';
  p.style.maxHeight = Math.max(160, window.innerHeight - top - 16) + 'px';
  p.style.overflow = 'hidden';
})();
"#;

fn place_popover() {
    spawn(async move {
        let _ = document::eval(PLACE_POPOVER).await;
    });
}
