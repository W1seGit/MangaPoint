//! First run: choose which sources to install.

use super::ask_enable_browser;
use mangapoint_ui::art::mascot::BrowsingScene;
use super::widgets::{Chip, Icon, Loading};
use crate::backend;
use crate::state::spawn_forever;
use crate::state::{AppState, Page};
use dioxus::prelude::*;
use library::Choice;
use sources::{Requires, SourceManager};

/// The sources step is done: whatever was not answered counts as skipped.
/// Going Back and Next again changes nothing the second time, so nothing is
/// saved or logged then.
pub fn finish_sources(state: AppState) {
    let before = state.sources.read().clone();
    let mut sources_state = before.clone();
    if let Some(client) = state.client() {
        SourceManager::new(&*client).finish_onboarding(&mut sources_state);
    }
    if sources_state == before {
        return;
    }
    let installed = sources_state
        .choices
        .values()
        .filter(|c| **c == Choice::Install)
        .count();
    let mut s = state.sources;
    s.set(sources_state);
    state.save_sources();
    tracing::info!("Sources chosen in the first-run setup: {installed} to install");
}

#[component]
pub fn Setup(#[props(default = false)] onboarding: bool) -> Element {
    let state = use_context::<AppState>();

    let offers = use_resource(move || async move {
        let _ = state.sources_version.read();
        let client = state.client()?;
        let snapshot = state.sources.read().clone();
        SourceManager::new(&*client).offers(&snapshot).await.ok()
    });

    // The recommended ones still to install: not installed, and not turned down. (A source chosen once
    // but not in the catalog engine now, after its data was reset, is installed again.)
    let to_install = move || -> Vec<&'static str> {
        match &*offers.peek() {
            Some(Some(list)) => list
                .iter()
                .filter(|o| o.default.requires == Requires::Nothing && o.available && !o.installed && o.choice != Some(Choice::Skip))
                .map(|o| o.default.id)
                .collect(),
            _ => Vec::new(),
        }
    };
    let install_recommended = move |_| {
        let mut installing = state.installing;
        installing.set(true);
        let ids = to_install();
        spawn_forever(async move {
            for id in ids {
                backend::install_default(state, id).await;
            }
            installing.set(false);
            state.bump_sources();
        });
    };
    let busy = *state.installing.read();
    // When none is left (all installed, or turned down), there is nothing for the button to do.
    let pending = match &*offers.read() {
        Some(Some(_)) => to_install().len(),
        _ => 1,
    };
    let any_installed = match &*offers.read() {
        Some(Some(list)) => list.iter().any(|o| o.installed),
        _ => state.sources.read().choices.values().any(|c| *c == Choice::Install),
    };

    let finish = move |_| {
        finish_sources(state);
        state.go(Page::Search);
    };

    let rows = match &*offers.read() {
        Some(Some(list)) => list
            .iter()
            .map(|o| {
                let id = o.default.id;
                let needs_browser = o.default.requires == Requires::Browser;
                let installed = o.installed;
                let skipped = o.choice == Some(Choice::Skip) && !installed;
                rsx! {
                    div { class: "setup-row", key: "{id}",
                        div { class: "src-main",
                            div { class: "row wrap",
                                strong { class: "src-name", "{o.default.name}" }
                                if needs_browser { Chip { text: "needs browser support", kind: Some("warn".to_string()) } }
                                if installed { Chip { text: "installed", kind: Some("ok".to_string()) } }
                                if skipped { Chip { text: "skipped" } }
                                if !o.available { Chip { text: "not offered right now", kind: Some("bad".to_string()) } }
                            }
                            div { class: "muted small", "{o.default.description}" }
                        }
                        if o.available && !installed {
                            div { class: "src-actions",
                                button {
                                    class: "btn primary small",
                                    onclick: move |_| {
                                        let browser_off = !state.ui.settings.read().browser_support;
                                        if needs_browser && browser_off {
                                            ask_enable_browser(state, Some(id.to_string()));
                                        } else {
                                            spawn_forever(async move {
                                                backend::install_default(state, id).await;
                                                state.bump_sources();
                                            });
                                        }
                                    },
                                    Icon { name: "download", size: 15 }
                                    "Install"
                                }
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let loaded = matches!(&*offers.read(), Some(Some(_)));
    let installed_any = state
        .sources
        .read()
        .choices
        .values()
        .any(|c| *c == Choice::Install);

    rsx! {
        div { class: "onboard-head",
            if onboarding {
                div { class: "step-hero", BrowsingScene {} }
            }
            h1 { "Choose your sources" }
            p { class: "muted",
                "Sources are the websites MangaPoint searches. Pick the ones you want; you can change this any time in Sources. "
                "These all passed our full test (search, chapters and page images) when we last checked."
            }
        }
        div { class: "setup-tools",
            button {
                class: "btn setup-all",
                disabled: busy || pending == 0,
                onclick: install_recommended,
                if pending == 0 && !busy {
                    Icon { name: "check", size: 15 }
                    if any_installed { "All recommended sources are installed" } else { "Nothing left to install" }
                } else {
                    Icon { name: "download", size: 15 }
                    if busy { "Installing\u{2026}" } else { "Install all recommended" }
                }
            }
            if !onboarding {
                button {
                    class: "btn primary small",
                    disabled: !installed_any,
                    onclick: finish,
                    Icon { name: "check", size: 15 }
                    "Done"
                }
            }
        }
        if !loaded {
            Loading { label: "Loading sources\u{2026}" }
        }
        div { class: "setup-list", {rows.into_iter()} }
    }
}
