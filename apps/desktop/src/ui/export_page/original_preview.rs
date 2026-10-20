//! The Export screen's preview for a phone or tablet: the pages as they will be
//! in the file, which is as they were saved, in color. Nothing is converted, so
//! this is the page editor without the conversion: the same top bar (page jump
//! and zoom) and the column of thumbnails, and the pages in one long scroll.

use crate::state::{AppState, Zoom};
use crate::ui::widgets::{EmptyState, Loading};
use crate::ui::workspace::preview::ZoomMenu;
use dioxus::prelude::*;
use std::collections::HashSet;

/// Sizes the list to the window and reports the page at the top as `[first, last, current]`.
const EDITOR_SCRIPT: &str = include_str!("../../js/preview-editor.js");

/// Scroll the thumbnails to page `index`.
fn show_thumb(index: usize) {
    spawn(async move {
        let js = format!("document.querySelector('[data-editor-page=\"{index}\"]')?.scrollIntoView({{block:'nearest'}})");
        let _ = document::eval(&js).await;
    });
}

/// Show page `index`: scroll the list and the thumbnails to it.
fn show_page(index: usize) {
    spawn(async move {
        let js = format!("document.querySelector('.original-page[data-row=\"{index}\"]')?.scrollIntoView({{block:'start'}})");
        let _ = document::eval(&js).await;
    });
    show_thumb(index);
}

#[component]
pub(super) fn OriginalPreview() -> Element {
    let state = use_context::<AppState>();
    let mut failed = use_signal(HashSet::<usize>::new);
    let mut current = use_signal(|| 0usize);
    use_future(move || async move {
        let mut eval = document::eval(EDITOR_SCRIPT);
        while let Ok(message) = eval.recv::<Vec<i64>>().await {
            if let [_, _, at] = message.as_slice() {
                if *at >= 0 && *current.peek() != *at as usize {
                    current.set(*at as usize);
                    show_thumb(*at as usize);
                }
            }
        }
    });
    let plan = state.export_plan.read().clone();
    let Some(item) = plan.items.get(plan.active) else {
        return rsx! {};
    };
    let library = state.library.read();
    let Some(series) = library.find(&item.series_id) else {
        return rsx! { EmptyState { title: "Nothing to show", message: "Download a chapter to see its pages here." } };
    };
    // The chapter on show: the one picked in the list of chapters, else the one the screen remembers, else the first.
    let key = state
        .preview
        .read()
        .chapter_key
        .clone()
        .or_else(|| item.chapter.clone())
        .filter(|k| series.chapters.iter().any(|c| &c.key == k))
        .or_else(|| series.chapters.first().map(|c| c.key.clone()));
    let Some(key) = key else {
        return rsx! { EmptyState { title: "Nothing downloaded", message: "Download a chapter to see its pages here." } };
    };
    let series_id = series.id.clone();
    let chapter = series.chapters.iter().find(|c| c.key == key).cloned();
    drop(library);
    let Some(chapter) = chapter else { return rsx! {} };
    let listing = use_resource(use_reactive!(|(series_id, chapter)| async move {
        tokio::task::spawn_blocking(move || crate::ui::reader::original_pages(&series_id, &chapter))
            .await
            .ok()
    }));
    // Fit and Width both fill the column; a percentage is that much of it.
    let width = match state.preview.read().view.zoom {
        Zoom::Custom(percent) => format!("width: {percent}%;"),
        _ => "width: 100%;".to_string(),
    };
    let pages = listing.read().clone();
    let total = pages.as_ref().and_then(|p| p.as_ref()).map_or(0, |p| p.len());
    let at = current().min(total.saturating_sub(1));
    let number = if total == 0 { 0 } else { at + 1 };
    rsx! {
        div { id: "page-editor", class: "page-editor original-editor", tabindex: "0",
            div { class: "editor-toolbar",
                label { class: "editor-page-jump",
                    span { "Page" }
                    input {
                        r#type: "number", min: "1", max: "{total}", step: "1",
                        value: "{number}", disabled: total == 0, "aria-label": "Go to page",
                        onchange: move |e| {
                            if let Ok(n) = e.value().trim().parse::<usize>() {
                                if (1..=total).contains(&n) { current.set(n - 1); show_page(n - 1); }
                            }
                        },
                    }
                    span { class: "muted", "/ {total}" }
                }
                div { class: "editor-toolbar-actions",
                    ZoomMenu {}
                }
            }
            div { class: "editor-body",
                nav { class: "editor-thumbnails", "aria-label": "Pages",
                    if let Some(Some(pages)) = &pages {
                        for (n, (url, _)) in pages.iter().cloned().enumerate() {
                            button {
                                key: "{n}",
                                class: if n == at { "editor-thumb selected" } else { "editor-thumb" },
                                "data-editor-page": "{n}",
                                "aria-label": "Open page {n + 1}",
                                "aria-current": if n == at { "page" } else { "false" },
                                onclick: move |_| { current.set(n); show_page(n); },
                                div { class: "editor-thumb-art",
                                    img { src: "{url}", alt: "Page {n + 1}", loading: "lazy", decoding: "async" }
                                }
                                span { "{n + 1}" }
                            }
                        }
                    }
                }
                main { class: "editor-main",
                    div { class: "editor-scroll",
                        div { class: "original-pages", id: "original-preview",
                            {match &pages {
                                None => rsx! { Loading { label: "Opening the pages…" } },
                                Some(None) => rsx! { EmptyState { title: "These pages could not be read", message: "Their files are missing from this computer.", compact: true } },
                                Some(Some(pages)) => rsx! {
                                    for (n, (url, ratio)) in pages.iter().cloned().enumerate() {
                                        figure { key: "{n}", class: "original-page", "data-row": "{n}", style: "--ar: {ratio}; {width}",
                                            if failed.read().contains(&n) {
                                                figcaption { class: "muted small", "Page {n + 1} could not be shown." }
                                            } else {
                                                img { src: "{url}", alt: "Page {n + 1}", loading: "lazy", decoding: "async",
                                                    onerror: move |_| { failed.write().insert(n); } }
                                            }
                                        }
                                    }
                                },
                            }}
                        }
                    }
                }
            }
        }
    }
}
