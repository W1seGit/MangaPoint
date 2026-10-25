//! What frames every screen of both apps: the dock at the bottom centre (Search, Library, Export),
//! the bar in the bottom right corner (Tasks, Settings) and the pixel scenery behind a screen.
//! Each app says which items it has and what a press does.

use crate::art::design::Figure;
use dioxus::prelude::*;

/// One place to go to, in the dock or the corner bar.
#[derive(Clone, PartialEq, Debug)]
pub struct BarItem {
    /// What the tests and the address bar call it.
    pub id: &'static str,
    pub label: &'static str,
    /// The figure of the design (`art::design`).
    pub figure: &'static str,
    /// Is the screen it leads to the one on show?
    pub on: bool,
    /// A count to show on it (downloads waiting).
    pub badge: Option<String>,
    /// Is something happening behind it (a download running)?
    pub busy: bool,
}

impl BarItem {
    pub fn new(id: &'static str, label: &'static str, figure: &'static str, on: bool) -> Self {
        Self { id, label, figure, on, badge: None, busy: false }
    }
}

/// How wide a figure is drawn, so that an item can grow into a circle and back.
fn figure_width(figure: &str) -> &'static str {
    match figure {
        "nav-library" => "26.2px",
        "nav-export" => "25.6px",
        "gear" => "24px",
        _ => "27px",
    }
}

#[component]
fn BarButton(item: BarItem, tag: bool, on_pick: EventHandler<&'static str>) -> Element {
    let id = item.id;
    rsx! {
        button {
            class: match (item.on, item.busy) {
                (true, true) => "nav-item dock-item active on busy",
                (true, false) => "nav-item dock-item active on",
                (false, true) => "nav-item dock-item busy",
                (false, false) => "nav-item dock-item",
            },
            id: "nav-{item.label}",
            title: "{item.label}",
            "aria-label": "{item.label}",
            "aria-current": if item.on { "page" } else { "false" },
            style: "--w: {figure_width(item.figure)}",
            onclick: move |_| on_pick.call(id),
            Figure { name: item.figure }
            if let Some(count) = item.badge.as_ref() {
                span { class: "dock-badge", "{count}" }
            }
            if item.on && tag {
                span { class: "dock-tag", "{item.label}" }
            }
        }
    }
}

/// The bar at the bottom centre. The item that is on grows into a circle with its name under it.
#[component]
pub fn Dock(items: Vec<BarItem>, on_pick: EventHandler<&'static str>) -> Element {
    let any = items.iter().any(|i| i.on);
    rsx! {
        nav { class: if any { "nav dock has-pick" } else { "nav dock" }, "aria-label": "Pages",
            for item in items {
                BarButton { key: "{item.id}", item, tag: true, on_pick }
            }
        }
    }
}

/// The bar in the bottom right corner.
#[component]
pub fn CornerBar(items: Vec<BarItem>, on_pick: EventHandler<&'static str>) -> Element {
    rsx! {
        nav { class: "nav corner", "aria-label": "Tasks and settings",
            for item in items {
                BarButton { key: "{item.id}", item, tag: false, on_pick }
            }
        }
    }
}

/// The pixel scenery behind a screen: `"search"` has two clouds above and two rocks below. Anything
/// else has none.
#[component]
pub fn Backdrop(scene: &'static str, #[props(default)] leaving: bool) -> Element {
    match scene {
        "search" => rsx! {
            div { class: if leaving { "scenery scene-search leaving" } else { "scenery scene-search" }, "aria-hidden": "true",
                Figure { name: "search-cloud-1", class: "bd bd-cloud-1" }
                Figure { name: "search-cloud-2", class: "bd bd-cloud-2" }
                Figure { name: "search-rock-1", class: "bd bd-rock-1" }
                Figure { name: "search-rock-2", class: "bd bd-rock-2" }
            }
        },
        _ => rsx! {},
    }
}
