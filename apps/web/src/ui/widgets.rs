//! Small pieces used by several screens: the shared ones (`mangapoint_ui::widgets`) and the
//! website's own.

use dioxus::prelude::*;
pub use mangapoint_ui::widgets::*;

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

/// A zoom as a small menu, as in a document editor: a box showing the zoom (type a percentage
/// in it) and a list of the ready-made ones. The desktop app's `ZoomDropdown`, without the drag
/// handle.
#[component]
pub fn ZoomDropdown(selected: ZoomPick, percents: Vec<u16>, min: u16, max: u16, onpick: EventHandler<ZoomPick>) -> Element {
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
                span { class: "zoom-icon", Icon { name: "move-horizontal", size: 15 } }
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
