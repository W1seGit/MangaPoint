//! The Library's cards and the bar for the cards that are ticked, the same in both apps. They show
//! what they are given and say what was clicked; each app fills in its own menu items and actions.

use crate::art::design::Figure;
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

/// The two counts hanging from the top of a card: what is downloaded, and what has been exported.
#[derive(Clone, PartialEq, Debug)]
pub struct PosterTabs {
    /// "125/225": the chapters on this device, of all there are.
    pub downloaded: String,
    /// "64/124": the chapters exported to a reader. `None` where exports are not kept.
    pub exported: Option<String>,
}

/// What a card shows.
#[derive(Clone, PartialEq, Debug)]
pub struct PosterFacts {
    pub id: String,
    pub title: String,
    pub tabs: Option<PosterTabs>,
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
    // What the card used to spell out (chapters, storage, status) is in the tooltip of the title.
    let mut about: Vec<String> = Vec::new();
    if let Some((have, stored)) = f.stats.clone() {
        about.push(format!("{have} chapters"));
        about.push(stored);
    }
    about.extend(f.lines.iter().map(|(_, text)| text.clone()));
    let tip = if about.is_empty() { f.title.clone() } else { format!("{} \u{b7} {}", f.title, about.join(" \u{b7} ")) };
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
            if let Some(tabs) = f.tabs.clone() {
                div { class: if tabs.exported.is_some() { "poster-tabs" } else { "poster-tabs one" },
                    div { class: "poster-tab down", title: "Chapters downloaded",
                        Figure { name: "download-small" }
                        span { "{tabs.downloaded}" }
                    }
                    if let Some(exported) = tabs.exported.clone() {
                        div { class: "poster-tab up", title: "Chapters exported",
                            span { "{exported}" }
                            Figure { name: "export-small" }
                        }
                    }
                }
            }
            if menu_open {
                div { class: "menu-scrim", onclick: move |_| on_menu.call(false) }
                div { class: "menu down poster-menu", onclick: move |_| on_menu.call(false), {menu} }
            }
            // The text opens the book too.
            div { class: "poster-info", onclick: click,
                strong { class: "poster-title", title: "{tip}", "{f.title}" }
                if f.not_installed {
                    span { class: "poster-note", "Not installed yet" }
                }
                if let Some(b) = f.button.clone() {
                    button {
                        class: "btn poster-read", id: "{b.id}", "data-id": "{f.id}",
                        "aria-label": "{b.aria}", title: "{b.title}",
                        onclick: move |e| {
                            e.stop_propagation();
                            if selecting { on_toggle.call(()); } else { on_button.call(()); }
                        },
                        span { "{b.label}" }
                        Figure { name: "reading" }
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

/// The row above the cards: which part of the library to show, a filter and the sort order. The
/// views and sorts are given as (label, is it the chosen one) so that each app keeps its own list.
#[component]
pub fn LibraryTools(
    views: Vec<(&'static str, bool)>,
    on_view: EventHandler<usize>,
    filter: String,
    on_filter: EventHandler<String>,
    /// (key, label, chosen) of every way to sort.
    sorts: Vec<(&'static str, &'static str, bool)>,
    on_sort: EventHandler<String>,
) -> Element {
    let chosen = sorts.iter().find(|s| s.2).map(|s| s.0).unwrap_or_default();
    rsx! {
        div { class: "lib-tools",
            div { class: "pillrow",
                for (i, (label, on)) in views.into_iter().enumerate() {
                    button {
                        key: "{label}",
                        class: if on { "pill on" } else { "pill" },
                        onclick: move |_| on_view.call(i),
                        "{label}"
                    }
                }
            }
            input {
                class: "lib-filter", r#type: "search", placeholder: "Filter your library", "aria-label": "Filter your library",
                value: "{filter}", oninput: move |e| on_filter.call(e.value()),
            }
            select {
                class: "lib-sort", "aria-label": "Sort", value: "{chosen}",
                onchange: move |e| on_sort.call(e.value()),
                for (key, label, on) in sorts {
                    option { key: "{key}", value: "{key}", selected: on, "{label}" }
                }
            }
        }
    }
}
