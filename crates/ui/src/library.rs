//! The Library's cards and the bar for the cards that are ticked, the same in both apps. They show
//! what they are given and say what was clicked; each app fills in its own menu items and actions.

use crate::widgets::Icon;
use dioxus::prelude::*;

/// The button under a card: read, continue, or import the file a book is waiting for.
#[derive(Clone, PartialEq, Debug)]
pub struct PosterButton {
    pub id: String,
    pub label: String,
    pub icon: &'static str,
    pub title: String,
    pub aria: String,
    /// How much has been read, for the bar under the button.
    pub fraction: Option<f32>,
}

/// What a card shows.
#[derive(Clone, PartialEq, Debug)]
pub struct PosterFacts {
    pub id: String,
    pub title: String,
    /// How many chapters, and the space they take.
    pub stats: Option<(String, String)>,
    /// Short lines under the title, by class (`poster-about`, `poster-status`, `poster-place`).
    pub lines: Vec<(&'static str, String)>,
    pub button: Option<PosterButton>,
    /// A place kept for a book whose file is not on this device yet: it cannot be opened.
    pub not_installed: bool,
}

/// One card of the Library: the cover with its tick box and menu, the title and what it holds,
/// and a button. While cards are ticked, a click anywhere on a card ticks or unticks it.
#[component]
pub fn Poster(
    facts: PosterFacts,
    selected: bool,
    selecting: bool,
    menu_open: bool,
    /// The cover, and any badge over it.
    art: Element,
    /// The items of the card's menu. A click on one closes the menu.
    menu: Element,
    on_open: EventHandler<()>,
    on_toggle: EventHandler<()>,
    on_menu: EventHandler<bool>,
    on_button: EventHandler<()>,
) -> Element {
    let f = facts;
    let class = match (f.not_installed, selected) {
        (false, false) => "poster",
        (false, true) => "poster on",
        (true, false) => "poster not-installed",
        (true, true) => "poster not-installed on",
    };
    let open_title = (!f.not_installed).then(|| format!("Open {}", f.title));
    let tick = if selected { "Deselect" } else { "Select" };
    let click = move |_| {
        if selecting {
            on_toggle.call(());
        } else {
            on_open.call(());
        }
    };
    rsx! {
        div { class: "{class}", "data-id": "{f.id}",
            div { class: "poster-art", title: open_title.clone(), onclick: click,
                {art}
                button {
                    class: if selected { "poster-check on" } else { "poster-check" },
                    title: tick, "aria-label": tick,
                    onclick: move |e| { e.stop_propagation(); on_toggle.call(()); },
                    if selected { Icon { name: "check", size: 14 } }
                }
                button {
                    class: if menu_open { "poster-more open" } else { "poster-more" },
                    title: "More actions", "aria-label": "More actions for {f.title}",
                    onclick: move |e| { e.stop_propagation(); on_menu.call(!menu_open); },
                    Icon { name: "ellipsis", size: 16 }
                }
            }
            if menu_open {
                div { class: "menu-scrim", onclick: move |_| on_menu.call(false) }
                div { class: "menu down poster-menu", onclick: move |_| on_menu.call(false), {menu} }
            }
            // The text opens the book too, and lights up with the cover.
            div { class: "poster-text", title: open_title, onclick: click,
                strong { class: "poster-title", title: "{f.title}", "{f.title}" }
                if let Some((have, stored)) = f.stats.clone() {
                    div { class: "poster-stats",
                        div { span { class: "poster-stat-label", "Chapters" } strong { "{have}" } }
                        div { span { class: "poster-stat-label", "Storage" } strong { "{stored}" } }
                    }
                }
                for (kind, text) in f.lines.iter().cloned() {
                    span { class: "muted small {kind}", title: "{text}", "{text}" }
                }
            }
            if let Some(b) = f.button.clone() {
                button {
                    class: "btn poster-read", id: "{b.id}", "data-id": "{f.id}",
                    "aria-label": "{b.aria}", title: "{b.title}",
                    onclick: move |_| { if selecting { on_toggle.call(()); } else { on_button.call(()); } },
                    Icon { name: b.icon, size: 15 } "{b.label}"
                }
                if let Some(done) = b.fraction {
                    div { class: "progress poster-read-progress", "aria-label": "Read so far",
                        title: "{(done * 100.0).round()}% read",
                        div { style: "width: {(done * 100.0).round()}%" }
                    }
                }
            }
        }
    }
}

/// The bar at the bottom while cards are ticked: how many and how big, select all or none, the
/// app's buttons, and a menu of the rest. A click on a menu item closes the menu.
#[component]
pub fn ActionBar(
    count: usize,
    /// What the ticked cards take, as words ("1.2 GB on this computer").
    size: String,
    all_ticked: bool,
    on_all: EventHandler<bool>,
    buttons: Element,
    more: Element,
) -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        div { class: "action-bar", id: "action-bar",
            div { class: "bar-count",
                strong { id: "bar-count", "{count} selected" }
                span { class: "muted small", "{size}" }
            }
            button { class: "btn ghost small", id: "bar-select-all",
                onclick: move |_| on_all.call(!all_ticked),
                if all_ticked { "Select none" } else { "Select all" }
            }
            span { class: "grow" }
            {buttons}
            div { class: "bar-menu",
                button { class: "btn", id: "bar-more", title: "More actions", "aria-label": "More actions",
                    onclick: move |_| open.set(!open()),
                    Icon { name: "ellipsis", size: 18 }
                }
                if open() {
                    div { class: "menu-scrim", onclick: move |_| open.set(false) }
                    div { class: "menu up", onclick: move |_| open.set(false), {more} }
                }
            }
        }
    }
}
