//! The Chapters panel: which chapters of the series on screen are exported.

use super::{active_series, Panel};
use crate::logic;
use crate::state::AppState;
use crate::ui::workspace::preview::switch_chapter;
use dioxus::prelude::*;

/// The bar at the top of the Chapters panel: Every chapter, and a range to
/// export. Its own component, so the panel's function stays small enough for the
/// optimiser (a very large one crashed the compiler in release builds).
#[component]
fn ChapterRangeBar(active: usize, all_on: bool, keys: Vec<String>, numbers: Vec<Option<f64>>) -> Element {
    let state = use_context::<AppState>();
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    let all_keys = keys.clone();
    let range_keys = keys;
    rsx! {
    div { class: "px-sticky",
            div { class: "px-all-row",
                label { class: "px-all",
                    input {
                        r#type: "checkbox", checked: all_on,
                        onchange: move |_| {
                            let mut plan = state.export_plan;
                            let mut guard = plan.write();
                            if let Some(item) = guard.items.get_mut(active) {
                                if all_on { item.excluded = all_keys.iter().cloned().collect(); } else { item.excluded.clear(); }
                            }
                            range_error.set(None);
                        },
                    }
                    span { "Every chapter" }
                }
                form {
                    class: "px-range",
                    onsubmit: move |e| {
                        e.prevent_default();
                        match logic::select_by_text(&range_text.read(), &numbers) {
                            Ok(found) if found.is_empty() => {
                                // Nothing changes: leaving the export empty by accident is worse.
                                range_error.set(Some("no chapter of this series has a number in that range".into()));
                            }
                            Ok(found) => {
                                range_error.set(None);
                                let mut plan = state.export_plan;
                                let mut guard = plan.write();
                                if let Some(item) = guard.items.get_mut(active) {
                                    item.excluded = range_keys
                                        .iter()
                                        .enumerate()
                                        .filter(|(i, _)| !found.contains(i))
                                        .map(|(_, k)| k.clone())
                                        .collect();
                                }
                            }
                            Err(msg) => range_error.set(Some(msg)),
                        }
                    },
                    input {
                        r#type: "text", placeholder: "Range: 1-20, 25, 30-",
                        title: "Export only these chapters. Press Enter or Select.",
                        value: "{range_text}",
                        oninput: move |e| range_text.set(e.value()),
                    }
                    button { class: "btn small", r#type: "submit", "Select" }
                }
            }
            if let Some(msg) = range_error() {
                p { class: "px-range-error", "{msg}" }
            }
        }
    }
}

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
            ChapterRangeBar { active, all_on, keys: all_keys.clone(), numbers }
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
