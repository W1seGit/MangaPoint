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
