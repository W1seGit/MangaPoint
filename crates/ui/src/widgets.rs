//! Small pieces used by several screens of both apps.

use crate::art;
use dioxus::prelude::*;

/// A Lucide icon by name (see `art/icons.rs`). Takes the surrounding text color.
#[component]
pub fn Icon(name: &'static str, size: Option<u32>) -> Element {
    let size = size.unwrap_or(18);
    let body = art::icons::body(name).unwrap_or("");
    rsx! {
        svg {
            class: "icon",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            dangerous_inner_html: "{body}",
        }
    }
}

/// A screen with nothing on it yet: the cat napping, and a friendly line.
#[component]
pub fn EmptyState(
    title: String,
    message: String,
    compact: Option<bool>,
    /// The cat beside the words, in a card, rather than above them.
    side: Option<bool>,
    /// Show the cat asleep on an empty bookshelf instead of on its own.
    shelf: Option<bool>,
    /// Or another scene: "search", "ocean" or "waiting".
    scene: Option<&'static str>,
    children: Element,
) -> Element {
    let class = if side.unwrap_or(false) {
        "empty side"
    } else if compact.unwrap_or(false) {
        "empty compact"
    } else {
        "empty"
    };
    rsx! {
        div { class: "{class}",
            if scene == Some("ocean") {
                art::ocean::OceanCat {}
            } else if scene == Some("search") {
                art::scenes::SearchCat {}
            } else if scene == Some("waiting") {
                art::hang::HangingCat {}
            } else if shelf.unwrap_or(false) {
                art::shelf::ShelfCat {}
            } else {
                art::cat::RestingCat {}
            }
            div { class: "empty-words",
                h2 { "{title}" }
                p { "{message}" }
                {children}
            }
        }
    }
}

/// How far something is, or a bar that runs back and forth when that is not known.
#[component]
pub fn ProgressBar(fraction: Option<f32>) -> Element {
    match fraction {
        Some(f) => {
            let pct = (f.clamp(0.0, 1.0) * 100.0).round();
            rsx! { div { class: "progress", div { style: "width: {pct}%" } } }
        }
        None => rsx! { div { class: "progress indeterminate", div {} } },
    }
}

/// A switch that slides between off and on. `label` names it for a screen reader when no text
/// beside it does.
#[component]
pub fn Toggle(on: bool, onchange: EventHandler<bool>, label: Option<String>) -> Element {
    let checked = if on { "true" } else { "false" };
    rsx! {
        button {
            r#type: "button", role: "switch", "aria-checked": "{checked}", "aria-label": label,
            class: if on { "toggle on" } else { "toggle" },
            onclick: move |_| onchange.call(!on),
            span { class: "knob" }
        }
    }
}

/// A small question mark that explains a setting when the pointer is on it.
#[component]
pub fn Help(text: String) -> Element {
    rsx! {
        span { class: "help", tabindex: "0", "data-tip": "{text}", "aria-label": "{text}",
            Icon { name: "circle-help", size: 15 }
        }
    }
}

#[component]
pub fn Chip(text: String, kind: Option<String>) -> Element {
    let class = format!("chip {}", kind.unwrap_or_default());
    rsx! { span { class: "{class}", "{text}" } }
}

/// Something is loading: the cat runs in circles in the middle of the place
/// where it will show up.
#[component]
pub fn Loading(label: String) -> Element {
    rsx! {
        div { class: "loading",
            art::cat::CirclingCat {}
            span { "{label}" }
        }
    }
}

/// A small working marker with a few words, to sit in a line of text.
#[component]
pub fn Busy(label: String, size: Option<u32>) -> Element {
    rsx! {
        span { class: "busy",
            art::cat::CirclingCat { size: size.unwrap_or(44) }
            "{label}"
        }
    }
}

/// The picture for a cover that is missing or has not loaded yet.
#[component]
pub fn NoCover() -> Element {
    rsx! { div { class: "cover empty art", dangerous_inner_html: art::placeholder::svg() } }
}

/// One setting: what it is on the left (a question mark explains it, a note says what is true
/// right now), how to change it on the right.
#[component]
pub fn SettingRow(
    label: String,
    /// Explains the setting, behind a question mark.
    help: Option<String>,
    /// What is true right now, shown under the label.
    note: Option<String>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label",
                    "{label}"
                    if let Some(text) = help { Help { text } }
                }
                if let Some(text) = note.filter(|t| !t.is_empty()) { div { class: "sx-desc", "{text}" } }
            }
            div { class: "sx-control", {children} }
        }
    }
}
