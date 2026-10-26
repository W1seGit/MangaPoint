//! The Chapters panel: which chapters of the series on screen are exported.

use super::{active_series, Panel};
use mangapoint_ui::export::ChapterRangeBar;
use crate::state::AppState;
use crate::logic;
use crate::ui::workspace::preview::switch_chapter;
use dioxus::prelude::*;

#[component]
pub(super) fn ChaptersPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let active = state.export_plan.read().active;
    let excluded = state
        .export_plan
        .read()
        .items
        .get(active)
        .map(|i| i.excluded.clone())
        .unwrap_or_default();
    let included = series
        .chapters
        .iter()
        .filter(|c| !excluded.contains(&c.key))
        .count();
    let all_on = included == series.chapters.len();
    let all_keys: Vec<String> = series.chapters.iter().map(|c| c.key.clone()).collect();
    let numbers: Vec<Option<f64>> = series.chapters.iter().map(|c| c.number).collect();
    let shown = state.preview.read().chapter_key.clone();
    // Chapters of an earlier export are marked.
    let exported = state.exports.read().exported_keys(&series.id);

    rsx! {
        Panel {
            title: "Chapters",
            icon: "list-checks",
            id: "chapters",
            actions: rsx! { span { class: "muted small", "{included} of {series.chapters.len()}" } },
            // Stays at the top while the chapters scroll under it.
            ChapterRangeBar { all_on, numbers, item_name: "series",
                on_all: { let keys = all_keys.clone(); move |yes| {
                    let mut plan = state.export_plan;
                    let mut guard = plan.write();
                    if let Some(item) = guard.items.get_mut(active) {
                        item.excluded = if yes { Default::default() } else { keys.iter().cloned().collect() };
                    }
                } },
                on_range: { let keys = all_keys.clone(); move |found: Vec<usize>| {
                    let mut plan = state.export_plan;
                    let mut guard = plan.write();
                    if let Some(item) = guard.items.get_mut(active) {
                        item.excluded = keys.iter().enumerate().filter(|(i, _)| !found.contains(i)).map(|(_, key)| key.clone()).collect();
                    }
                } },
            }
            div { class: "px-chapters",
                for c in series.chapters.iter() {
                    {
                        let key = c.key.clone();
                        let show_key = c.key.clone();
                        let on = !excluded.contains(&c.key);
                        let showing = shown.as_deref() == Some(c.key.as_str());
                        rsx! {
                            div { key: "{c.key}",
                                class: match (on, showing) {
                                    (true, true) => "px-chapter on showing",
                                    (true, false) => "px-chapter on",
                                    (false, true) => "px-chapter showing",
                                    _ => "px-chapter",
                                },
                                input {
                                    r#type: "checkbox", checked: on, title: "Include this chapter in the export",
                                    onchange: move |_| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(active) {
                                            if !item.excluded.remove(&key) { item.excluded.insert(key.clone()); }
                                        }
                                    },
                                }
                                button {
                                    class: "px-chapter-open", title: "Show this chapter in the preview",
                                    onclick: move |_| switch_chapter(state, show_key.clone()),
                                    span { class: "px-chapter-no", { c.number.map(logic::format_number).unwrap_or_else(|| "–".into()) } }
                                    span { class: "grow px-chapter-name", title: "{c.name}", "{c.name}" }
                                }
                                if exported.contains(&c.key) {
                                    span { class: "chip tiny", title: "Already in an export", "exported" }
                                }
                                span { class: "muted small", "{logic::human_bytes(c.bytes)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
