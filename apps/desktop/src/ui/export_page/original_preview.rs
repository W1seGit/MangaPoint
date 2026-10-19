//! The Export screen's preview for a phone or tablet: the pages as they will be
//! in the file, which is as they were saved, in color. Nothing is converted.

use crate::state::AppState;
use crate::ui::widgets::{EmptyState, Icon, Loading};
use dioxus::prelude::*;
use std::collections::HashSet;

#[component]
pub(super) fn OriginalPreview() -> Element {
    let state = use_context::<AppState>();
    let mut chosen = use_signal(|| None::<String>);
    let mut failed = use_signal(HashSet::<usize>::new);
    let plan = state.export_plan.read().clone();
    let Some(item) = plan.items.get(plan.active) else {
        return rsx! {};
    };
    let library = state.library.read();
    let Some(series) = library.find(&item.series_id) else {
        return rsx! { EmptyState { title: "Nothing to show", message: "Download a chapter to see its pages here." } };
    };
    // The chapter on show: the one picked here, else the one the screen remembers, else the first.
    let key = chosen()
        .or_else(|| item.chapter.clone())
        .filter(|k| series.chapters.iter().any(|c| &c.key == k))
        .or_else(|| series.chapters.first().map(|c| c.key.clone()));
    let Some(key) = key else {
        return rsx! { EmptyState { title: "Nothing downloaded", message: "Download a chapter to see its pages here." } };
    };
    let series_id = series.id.clone();
    let title = series.title.clone();
    let options: Vec<(String, String)> = series
        .chapters
        .iter()
        .map(|c| (c.key.clone(), crate::logic::chapter_label(c)))
        .collect();
    let chapter = series.chapters.iter().find(|c| c.key == key).cloned();
    drop(library);
    let Some(chapter) = chapter else { return rsx! {} };
    let listing = use_resource(use_reactive!(|(series_id, chapter)| async move {
        tokio::task::spawn_blocking(move || crate::ui::reader::original_pages(&series_id, &chapter))
            .await
            .ok()
    }));
    rsx! {
        div { class: "original-preview", id: "original-preview",
            div { class: "row", style: "padding: 10px 14px; gap: 12px; border-bottom: 1px solid var(--line)",
                Icon { name: "image", size: 16 }
                strong { class: "grow", "{title}" }
                select { id: "original-preview-chapter", "aria-label": "Chapter to look at",
                    value: "{key}",
                    onchange: move |e| { failed.write().clear(); chosen.set(Some(e.value())); },
                    for (k, label) in options {
                        option { key: "{k}", value: "{k}", selected: k == key, "{label}" }
                    }
                }
            }
            div { class: "original-pages",
                {match &*listing.read() {
                    None => rsx! { Loading { label: "Opening the pages…" } },
                    Some(None) => rsx! { EmptyState { title: "These pages could not be read", message: "Their files are missing from this computer.", compact: true } },
                    Some(Some(pages)) => rsx! {
                        for (n, (url, ratio)) in pages.iter().cloned().enumerate() {
                            figure { key: "{n}", class: "original-page", style: "--ar: {ratio};",
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
