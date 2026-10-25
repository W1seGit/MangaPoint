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

/// The tick box beside a chapter range: pick only the whole-numbered chapters.
#[component]
pub fn WholeOnly(on: bool, on_change: EventHandler<bool>) -> Element {
    rsx! {
        label { class: "whole-only", title: "Leave out chapters numbered 12.1, 12.5 and the like (extras and notices)",
            input { r#type: "checkbox", checked: on, onchange: move |e| on_change.call(e.checked()) }
            span { "Whole numbers only" }
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

/// What the zoom box can hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomPick {
    /// The whole page.
    Fit,
    /// As wide as the space allows.
    Width,
    Percent(u16),
}

impl ZoomPick {
    /// What the box shows.
    pub fn label(self) -> String {
        match self {
            ZoomPick::Fit => "Fit page".to_string(),
            ZoomPick::Width => "Fit width".to_string(),
            ZoomPick::Percent(percent) => format!("{percent}%"),
        }
    }

    /// What was typed in the box: "125", "125%", "fit", "fit width", held to
    /// `min..=max` percent. `None` when it is not a zoom.
    pub fn parse(text: &str, min: u16, max: u16) -> Option<ZoomPick> {
        let text = text.trim().to_lowercase();
        if text.contains("width") {
            return Some(ZoomPick::Width);
        }
        if text.starts_with("fit") {
            return Some(ZoomPick::Fit);
        }
        let number: u32 = text.trim_end_matches('%').trim().parse().ok()?;
        Some(ZoomPick::Percent(number.clamp(u32::from(min), u32::from(max)) as u16))
    }
}

/// A zoom as a small menu, as in a document editor: a box showing the zoom
/// (type a percentage in it) and a list of the ready-made ones. Used for the
/// zoom of the page preview and for the zoom pages are converted at.
#[component]
pub fn ZoomDropdown(
    selected: ZoomPick,
    percents: Vec<u16>,
    min: u16,
    max: u16,
    onpick: EventHandler<ZoomPick>,
) -> Element {
    let ui = crate::host::use_ui();
    let mut open = use_signal(|| false);
    let label = selected.label();
    let mut choose = move |pick: ZoomPick| {
        onpick.call(pick);
        open.set(false);
    };
    let class = if open() { "zoom-menu is-open" } else { "zoom-menu" };
    rsx! {
        div { class: "{class}",
            div { class: "zoom-field",
                span {
                    class: "zoom-icon scrub", title: "Drag left or right to change the zoom",
                    "data-scrub": "1", "data-min": "{min}", "data-max": "{max}",
                    // The drag handle: moving sideways over it changes the zoom (`js/zoom-scrub.js`).
                    onmounted: move |_| ui.host().run_script("zoom_scrub", String::new()),
                    Icon { name: "move-horizontal", size: 15 }
                }
                input {
                    r#type: "text", value: "{label}", "aria-label": "Zoom", spellcheck: "false",
                    onfocus: move |_| open.set(true),
                    onchange: move |e| {
                        if let Some(parsed) = ZoomPick::parse(&e.value(), min, max) {
                            choose(parsed);
                        }
                    },
                }
                button { class: "zoom-chevron", title: "Zoom choices", onclick: move |_| open.set(!open()),
                    Icon { name: "chevron-down", size: 15 }
                }
            }
            if open() {
                div { class: "menu-scrim", onclick: move |_| open.set(false) }
                div { class: "menu down zoom-list",
                    button { class: if selected == ZoomPick::Fit { "menu-item on" } else { "menu-item" }, onclick: move |_| choose(ZoomPick::Fit), "Fit page" }
                    button { class: if selected == ZoomPick::Width { "menu-item on" } else { "menu-item" }, onclick: move |_| choose(ZoomPick::Width), "Fit width" }
                    div { class: "menu-sep" }
                    for percent in percents {
                        button {
                            key: "{percent}",
                            class: if selected == ZoomPick::Percent(percent) { "menu-item on" } else { "menu-item" },
                            onclick: move |_| choose(ZoomPick::Percent(percent)),
                            "{percent}%"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_zooms_are_understood() {
        assert_eq!(ZoomPick::parse("125", 25, 400), Some(ZoomPick::Percent(125)));
        assert_eq!(ZoomPick::parse(" 90 % ", 25, 400), Some(ZoomPick::Percent(90)));
        assert_eq!(ZoomPick::parse("Fit page", 25, 400), Some(ZoomPick::Fit));
        assert_eq!(ZoomPick::parse("fit width", 25, 400), Some(ZoomPick::Width));
        assert_eq!(ZoomPick::parse("1", 25, 400), Some(ZoomPick::Percent(25)));
        assert_eq!(ZoomPick::parse("99999", 25, 400), Some(ZoomPick::Percent(400)));
        assert_eq!(ZoomPick::parse("big", 25, 400), None);
        assert_eq!(ZoomPick::parse("150", 100, 300), Some(ZoomPick::Percent(150)));
        assert_eq!(ZoomPick::parse("20", 100, 300), Some(ZoomPick::Percent(100)));
        assert_eq!(ZoomPick::Width.label(), "Fit width");
        assert_eq!(ZoomPick::Percent(150).label(), "150%");
    }
}
