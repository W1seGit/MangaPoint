//! The reader's chrome, the same in both apps: the bars over the page (the way out, the chapter
//! and its arrows, the settings button, the title and how far along), the reading settings, the
//! zoom menu and the card at the end of a chapter. They only show what they are given and say what
//! was asked; each app's reader decides what that does.

use crate::logic::reader::{percent_finished, zoom_choice, ReaderLook, ZoomChoice};
use crate::widgets::{Icon, SettingRow};
use dioxus::prelude::*;

/// Where the reader is in the chapter: the page at the top of the screen, how many pages the
/// chapter has, and how far down it is (0 to 1).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ReadPos {
    pub page: usize,
    pub pages: usize,
    pub frac: f32,
}

/// How far along the chapter is, and its title, at the bottom of the screen. Apart from the rest
/// of the bar so scrolling redraws only this.
#[component]
pub fn ReaderCaption(title: String, pos: ReadSignal<ReadPos>) -> Element {
    let p = pos();
    let percent = percent_finished(p.frac);
    let shown = p.page + 1;
    rsx! {
        footer { class: "reader-bar reader-bottom",
            div { class: "reader-caption",
                strong { id: "reader-title", title: "{title}", "{title}" }
                span { class: "reader-percent", id: "reader-percent", "{percent}%" }
                span { class: "sr-only", id: "reader-page-counter", "aria-label": "Page", "aria-live": "polite",
                    if p.pages == 0 { "–" } else { "{shown} / {p.pages}" }
                }
            }
        }
    }
}

/// The class of a button of a group: lit when it is the one chosen.
fn on(yes: bool) -> &'static str {
    if yes {
        "on"
    } else {
        ""
    }
}

/// Chrome over the page: the way out, arrows and the chapter across the top, the settings button,
/// and the title and how far along you are at the bottom. Both bars fade into the page and slide
/// away while reading.
#[component]
pub fn ReaderBar(
    title: String,
    chapter_label: String,
    items: Vec<(usize, String, bool)>,
    current: usize,
    look: ReaderLook,
    pos: ReadSignal<ReadPos>,
    always: bool,
    /// How else the zoom is changed here (keys, the wheel, pinching), for its question mark.
    zoom_help: String,
    on_back: EventHandler<()>,
    on_prev: EventHandler<()>,
    on_next: EventHandler<()>,
    on_chapter: EventHandler<usize>,
    on_zoom: EventHandler<ZoomChoice>,
    on_always: EventHandler<bool>,
) -> Element {
    let mut menu = use_signal(|| false);
    let mut settings = use_signal(|| false);
    rsx! {
        header { class: "reader-bar reader-top",
            button { class: "reader-round", id: "reader-back", "aria-label": "Back to library", title: "Back to library (Esc)",
                onclick: move |_| on_back.call(()),
                Icon { name: "book-open", size: 20 }
            }
            div { class: "reader-chapter-nav",
                button { class: "reader-arrow", id: "reader-arrow-left", "aria-label": "Previous chapter", title: "Previous chapter",
                    onclick: move |_| on_prev.call(()),
                    Icon { name: "chevron-left", size: 26 }
                }
                div { class: "menu-anchor reader-chapter-anchor",
                    button { class: "reader-chapter-link", id: "reader-chapter-picker", "aria-label": "Chapter", title: "Choose a chapter ( [ and ] )",
                        onclick: move |_| menu.set(!menu()),
                        span { class: "reader-chapter-text", "{chapter_label}" } Icon { name: "chevron-down", size: 14 }
                    }
                    if menu() {
                        div { class: "menu-scrim", onclick: move |_| menu.set(false) }
                        div { class: "menu down reader-menu", id: "reader-chapter-menu", role: "listbox",
                            for (index, name, read) in items.iter().cloned() {
                                button {
                                    key: "{index}", class: if index == current { "menu-item reader-chapter-item on" } else { "menu-item reader-chapter-item" },
                                    "data-index": "{index}", role: "option", "aria-selected": "{index == current}",
                                    onclick: move |_| { menu.set(false); on_chapter.call(index); },
                                    span { class: "grow", "{name}" }
                                    if read { span { class: "reader-read", title: "Read", Icon { name: "check", size: 14 } } }
                                }
                            }
                        }
                    }
                }
                button { class: "reader-arrow", id: "reader-arrow-right", "aria-label": "Next chapter", title: "Next chapter",
                    onclick: move |_| on_next.call(()),
                    Icon { name: "chevron-right", size: 26 }
                }
            }
            button { class: "reader-round", id: "reader-view-button", "aria-label": "Reading settings", title: "Reading settings",
                onclick: move |_| settings.set(true),
                Icon { name: "settings", size: 20 }
            }
        }
        ReaderCaption { title: title.clone(), pos }
        if settings() {
            ReaderSettings { look, always, zoom_help: zoom_help.clone(), on_close: move |_| settings.set(false), on_zoom, on_always }
        }
    }
}

/// The reading settings, full screen: how the book is shown and how it is read.
#[component]
pub fn ReaderSettings(
    look: ReaderLook,
    always: bool,
    zoom_help: String,
    on_close: EventHandler<()>,
    on_zoom: EventHandler<ZoomChoice>,
    on_always: EventHandler<bool>,
) -> Element {
    rsx! {
        div { class: "reader-settings", id: "reader-settings", role: "dialog", "aria-label": "Reading settings",
            div { class: "reader-settings-inner",
                header { class: "reader-settings-head",
                    button { class: "reader-round", id: "reader-settings-close", "aria-label": "Back to the book", title: "Back to the book",
                        onclick: move |_| on_close.call(()), Icon { name: "arrow-left", size: 20 }
                    }
                    h2 { "Reading settings" }
                }
                div { class: "reader-settings-body",
                    SettingRow { label: "Zoom", help: format!("Fit page suits the window: the full width of a phone, a comfortable column on a big screen. Pick a percentage to make it smaller or larger. {zoom_help}"),
                        ZoomMenu { look, on_zoom }
                    }
                    SettingRow { label: "Controls", help: "The bars with the chapter, the settings and your place can hide while you read, or stay on the screen.",
                        div { class: "seg", role: "group", "aria-label": "Controls",
                            button { id: "reader-bars-auto", class: on(!always), "aria-pressed": "{!always}",
                                onclick: move |_| on_always.call(false), "Hide while reading" }
                            button { id: "reader-bars-always", class: on(always), "aria-pressed": "{always}",
                                onclick: move |_| on_always.call(true), "Always show" }
                        }
                    }
                }
            }
        }
    }
}

/// The page zoom: a box showing it (type a percentage in it) and a list of ready-made ones.
#[component]
pub fn ZoomMenu(look: ReaderLook, on_zoom: EventHandler<ZoomChoice>) -> Element {
    let mut open = use_signal(|| false);
    let selected = zoom_choice(look.zoom);
    let mut choose = move |choice: ZoomChoice| {
        on_zoom.call(choice);
        open.set(false);
    };
    let item = |c: ZoomChoice| if selected == c { "menu-item on" } else { "menu-item" };
    rsx! {
        div { class: if open() { "zoom-menu is-open" } else { "zoom-menu" },
            div { class: "zoom-field",
                span { class: "zoom-icon", Icon { name: "zoom-in", size: 15 } }
                input {
                    id: "reader-zoom-box", r#type: "text", value: "{selected.label()}", "aria-label": "Zoom", spellcheck: "false",
                    onfocus: move |_| open.set(true),
                    onchange: move |e| {
                        if let Some(choice) = ZoomChoice::parse(&e.value()) { choose(choice); }
                    },
                }
                button { class: "zoom-chevron", id: "reader-zoom-toggle", "aria-label": "Zoom choices", title: "Zoom choices", onclick: move |_| open.set(!open()),
                    Icon { name: "chevron-down", size: 15 }
                }
            }
            if open() {
                div { class: "menu-scrim", onclick: move |_| open.set(false) }
                div { class: "menu down zoom-list", id: "reader-zoom-list",
                    button { class: item(ZoomChoice::Fit), id: "reader-fit-height", onclick: move |_| choose(ZoomChoice::Fit), "Fit page" }
                    div { class: "menu-sep" }
                    for percent in [25u16, 50, 75, 90, 125, 150, 200, 300, 400, 500] {
                        button {
                            key: "{percent}", class: item(ZoomChoice::Percent(percent)),
                            onclick: move |_| choose(ZoomChoice::Percent(percent)),
                            "{percent}%"
                        }
                    }
                }
            }
        }
    }
}

/// The note at the end of a chapter, with the way on.
#[component]
pub fn EndCard(
    title: String,
    note: String,
    prev: Option<String>,
    next: Option<String>,
    on_prev: EventHandler<()>,
    on_next: EventHandler<()>,
    on_back: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "reader-end",
            h2 { "{title}" }
            if !note.is_empty() { p { class: "muted", "{note}" } }
            div { class: "reader-end-buttons",
                if let Some(prev) = prev {
                    button { class: "btn", id: "reader-prev-chapter", "aria-label": "Previous chapter",
                        onclick: move |_| on_prev.call(()),
                        Icon { name: "arrow-left", size: 16 } "Previous · {prev}"
                    }
                }
                if let Some(next) = next {
                    button { class: "btn primary", id: "reader-next-chapter", "aria-label": "Next chapter",
                        onclick: move |_| on_next.call(()),
                        "Next · {next}" Icon { name: "arrow-right", size: 16 }
                    }
                } else {
                    button { class: "btn", id: "reader-end-back", onclick: move |_| on_back.call(()),
                        Icon { name: "arrow-left", size: 16 } "Back to your library"
                    }
                }
            }
        }
    }
}
