//! The panel for choosing which sources to search.

use crate::ui::widgets::Chip;
use crate::state::AppState;
use dioxus::prelude::*;
use library::Verdict;
use super::{place_popover, SourceInfo};

/// The panel for choosing the sources to search, opened by the filter button.
#[component]
pub(super) fn SourcePanel(
    sources: Vec<SourceInfo>,
    excluded: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    on_close: EventHandler<()>,
) -> Element {
    let state = use_context::<AppState>();
    let mut find = use_signal(String::new);
    let health = state.health.read().clone();
    let needle = find.read().trim().to_lowercase();
    let shown: Vec<&SourceInfo> = sources
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.lang.to_lowercase().contains(&needle)
        })
        .collect();
    let all_ids: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
    let failing: Vec<String> = sources
        .iter()
        .filter(|s| {
            !health
                .latest(&s.id)
                .is_some_and(|r| r.verdict == Verdict::Pass)
        })
        .map(|s| s.id.clone())
        .collect();

    rsx! {
        div { class: "popover", onmounted: move |_| place_popover(),
            div { class: "popover-quick",
                button { class: "pill small", r#type: "button", onclick: move |_| on_change.call(Vec::new()), "All" }
                button { class: "pill small", r#type: "button", onclick: move |_| on_change.call(all_ids.clone()), "None" }
                button { class: "pill small", r#type: "button", title: "Only the sources whose last test passed",
                    onclick: move |_| on_change.call(failing.clone()),
                    "Tested OK"
                }
            }
            input {
                r#type: "search", placeholder: "Find a source or a language",
                value: "{find}", oninput: move |e| find.set(e.value()),
            }
            div { class: "source-list",
                for source in shown.iter() {
                    {
                        let included = !excluded.contains(&source.id);
                        let verdict = health.latest(&source.id).map(|r| r.verdict);
                        let toggle_id = source.id.clone();
                        let current = excluded.clone();
                        rsx! {
                            label { class: "source-row", key: "{source.id}",
                                input {
                                    r#type: "checkbox", checked: included,
                                    onchange: move |_| {
                                        let mut next = current.clone();
                                        if let Some(at) = next.iter().position(|x| *x == toggle_id) { next.remove(at); } else { next.push(toggle_id.clone()); }
                                        on_change.call(next);
                                    },
                                }
                                span { class: "grow", "{source.name}" }
                                span { class: "muted small", "{source.lang}" }
                                if source.adult { Chip { text: "18+", kind: Some("warn".to_string()) } }
                                {match verdict {
                                    Some(Verdict::Pass) => rsx! { Chip { text: "works", kind: Some("ok".to_string()) } },
                                    Some(Verdict::NeedsBrowser) | Some(Verdict::NeedsFlareSolverr) => rsx! { Chip { text: "needs help", kind: Some("warn".to_string()) } },
                                    Some(Verdict::Failed) => rsx! { Chip { text: "not working", kind: Some("bad".to_string()) } },
                                    None => rsx! {},
                                }}
                            }
                        }
                    }
                }
                if shown.is_empty() { p { class: "muted small", style: "padding: 8px", "No source matches that." } }
            }
            div { class: "popover-foot",
                button { class: "btn primary small", r#type: "button", onclick: move |_| on_close.call(()), "Done" }
            }
        }
    }
}
