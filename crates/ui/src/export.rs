//! Pieces of the Export screen both apps draw the same way: the folding panels of the side
//! columns and the bars between them (dragged by `js/export-split.js`).

use crate::widgets::Icon;
use dioxus::prelude::*;

/// A panel of the Export screen's side columns, with a title that folds it away.
#[component]
pub fn Panel(
    title: String,
    icon: &'static str,
    /// What the splitters (and the tests) call this panel.
    id: &'static str,
    /// Is it open to begin with?
    open: Option<bool>,
    /// A starting height in pixels. Without one the panel takes what is left.
    height: Option<u32>,
    /// Buttons for the title bar, which do not fold the panel.
    actions: Option<Element>,
    children: Element,
) -> Element {
    let mut open = use_signal(move || open.unwrap_or(true));
    let class = match (open(), height.is_some()) {
        (true, true) => "px-panel open fixed",
        (true, false) => "px-panel open",
        _ => "px-panel collapsed",
    };
    let style = height.map(|h| format!("--h: {h}px;")).unwrap_or_default();
    let chevron = if open() { "chevron-down" } else { "chevron-right" };
    rsx! {
        section { class: "{class}", style: "{style}", "data-panel": "{id}", id: "panel-{id}",
            header { class: "px-panel-head", onclick: move |_| open.set(!open()),
                Icon { name: chevron, size: 15 }
                Icon { name: icon, size: 15 }
                span { class: "grow", "{title}" }
                if let Some(actions) = actions {
                    div { class: "px-panel-actions", onclick: move |e| e.stop_propagation(), {actions} }
                }
            }
            div { class: "px-panel-body", {children} }
        }
    }
}

/// The bar between two panels, which can be dragged to resize them. `target` is the panel that
/// changes size; `dir` is -1 when dragging down shrinks it.
#[component]
pub fn Split(target: &'static str, dir: i32) -> Element {
    rsx! {
        div { class: "px-split", "data-split": "panel", "data-target": "{target}", "data-dir": "{dir}",
            title: "Drag to resize the panels" }
    }
}

/// Chapter selection with a validated range, leaving host storage to callbacks.
#[component]
pub fn ChapterRangeBar(all_on: bool, numbers: Vec<Option<f64>>, item_name: String, on_all: EventHandler<bool>, on_range: EventHandler<Vec<usize>>) -> Element {
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    rsx! {
        div { class: "px-sticky",
            div { class: "px-all-row",
                label { class: "px-all",
                    input {
                        r#type: "checkbox", id: "export-all", checked: all_on,
                        onchange: move |_| {
                            on_all.call(!all_on);
                            range_error.set(None);
                        },
                    }
                    span { "Every chapter" }
                }
                form {
                    class: "px-range",
                    onsubmit: move |e| {
                        e.prevent_default();
                        match crate::logic::chapters::select_by_text(&range_text.read(), &numbers) {
                            Ok(found) if found.is_empty() => {
                                // Nothing changes: leaving the export empty by accident is worse.
                                range_error.set(Some(format!("no chapter of this {item_name} has a number in that range")));
                            }
                            Ok(found) => {
                                range_error.set(None);
                                on_range.call(found);
                            }
                            Err(msg) => range_error.set(Some(msg)),
                        }
                    },
                    input {
                        r#type: "text", id: "export-range", placeholder: "Range: 1-20, 25, 30-",
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


/// Preview zoom choices, independent of the app's preview state.
#[component]
pub fn PreviewZoom(zoom: crate::logic::preview::Zoom, on_change: EventHandler<crate::logic::preview::Zoom>) -> Element {
    use crate::logic::preview::Zoom;
    use crate::widgets::{ZoomDropdown, ZoomPick};
    let selected = match zoom { Zoom::Fit => ZoomPick::Fit, Zoom::Width => ZoomPick::Width, Zoom::Custom(p) => ZoomPick::Percent(p) };
    rsx! { ZoomDropdown {
        selected, percents: vec![50u16, 75, 90, 100, 125, 150, 200], min: Zoom::MIN_PERCENT, max: Zoom::MAX_PERCENT,
        onpick: move |pick| on_change.call(match pick { ZoomPick::Fit => Zoom::Fit, ZoomPick::Width => Zoom::Width, ZoomPick::Percent(p) => Zoom::Custom(p) }),
    } }
}
