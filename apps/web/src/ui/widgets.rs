//! Small pieces used by several screens. Same markup and class names as the
//! desktop app's `widgets.rs`, so `style.css` styles them unchanged.

use crate::art;
use dioxus::prelude::*;

/// A Lucide icon by name. Takes the surrounding text color.
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
    /// Show the cat asleep on an empty bookshelf instead of on its own.
    shelf: Option<bool>,
    /// Or another scene: "search", "ocean" or "waiting".
    scene: Option<&'static str>,
    children: Element,
) -> Element {
    let class = if compact.unwrap_or(false) { "empty compact" } else { "empty" };
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

/// A switch that slides between off and on.
#[component]
pub fn Toggle(on: bool, onchange: EventHandler<bool>, label: Option<String>) -> Element {
    let checked = if on { "true" } else { "false" };
    rsx! {
        button {
            r#type: "button", role: "switch", "aria-checked": "{checked}", "aria-label": label.unwrap_or_default(),
            class: if on { "toggle on" } else { "toggle" },
            onclick: move |_| onchange.call(!on),
            span { class: "knob" }
        }
    }
}

#[component]
pub fn Chip(text: String, kind: Option<String>) -> Element {
    let class = format!("chip {}", kind.unwrap_or_default());
    rsx! { span { class: "{class}", "{text}" } }
}

/// Something is loading: the cat runs in circles.
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

/// The cover of a book: the browser fills the picture from the thumbnail it
/// kept; a book without one shows the placeholder.
#[component]
pub fn Cover(id: String, has_cover: bool) -> Element {
    if !has_cover {
        return rsx! { NoCover {} };
    }
    rsx! {
        span { class: "cover-box",
            img { class: "cover", "data-cover": "{id}", alt: "", decoding: "async" }
        }
    }
}

#[component]
pub fn NoCover() -> Element {
    rsx! { div { class: "cover empty art", dangerous_inner_html: art::placeholder::svg() } }
}

/// A line of help for a setting.
#[component]
pub fn Help(text: String) -> Element {
    rsx! {
        span { class: "help", tabindex: "0", "data-tip": "{text}", "aria-label": "{text}",
            Icon { name: "circle-help", size: 15 }
        }
    }
}

/// One setting: what it is on the left (a question mark explains it, a note says what is true
/// right now), how to change it on the right. The same row as the desktop app's settings.
#[component]
pub fn SettingRow(label: String, help: Option<String>, note: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "{label}" if let Some(text) = help { Help { text } } }
                if let Some(text) = note.filter(|t| !t.is_empty()) { div { class: "sx-desc", "{text}" } }
            }
            div { class: "sx-control", {children} }
        }
    }
}
