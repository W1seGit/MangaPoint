//! The middle of the Export screen: the pages of a chapter as they are and as they will be in the
//! files. On a wide screen the two can be shown side by side (Compare); on a phone one of the two
//! tabs at the top picks which.

use super::{edit, Look, View};
use crate::app::Ctx;
use crate::model::Book;
use crate::ui::widgets::{EmptyState, Icon, Loading};
use dioxus::prelude::*;

#[component]
pub fn Preview(book: Book, narrow: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let device_name = super::device_of(&e.opts).map(|d| d.name).unwrap_or_default();
    let chapter = book.chapters.get(e.shown).cloned();
    let Some(chapter) = chapter else {
        return rsx! { EmptyState { title: "Nothing to show", message: "This book has no chapters.", compact: true } };
    };
    // On a phone one picture at a time; on a computer the converted pages, or both.
    let (show_original, show_reader) = if narrow {
        (e.view == View::Original, e.view == View::Reader)
    } else {
        (e.compare, true)
    };
    let columns = usize::from(show_original) + usize::from(show_reader);
    rsx! {
        div { class: "original-preview export-preview", id: "export-preview",
            div { class: "row px-preview-head",
                Icon { name: "image", size: 16 }
                strong { class: "grow px-preview-title", "{book.title}" }
                select { id: "export-preview-chapter", "aria-label": "Chapter to look at",
                    value: "{e.shown}",
                    onchange: move |ev| { let i = ev.value().parse::<usize>().unwrap_or(0); edit(ctx, |e| e.shown = i); },
                    for (i, c) in book.chapters.iter().enumerate() {
                        option { key: "{i}", value: "{i}", selected: i == e.shown, "{c.label(i)}" }
                    }
                }
                if !narrow {
                    button { class: if e.compare { "btn small primary" } else { "btn small" }, id: "export-compare",
                        title: "Show the pages as they are beside the pages as they will be",
                        onclick: move |_| edit(ctx, |e| e.compare = !e.compare),
                        Icon { name: "arrow-down-up", size: 14 } "Compare"
                    }
                }
            }
            {match &e.look {
                Look::Idle | Look::Loading => rsx! { Loading { label: "Opening the pages\u{2026}" } },
                Look::Failed(message) => rsx! {
                    EmptyState { title: "These pages could not be shown", message: "{message}", compact: true }
                },
                Look::Ready { originals, converted, total, shown } => rsx! {
                    div { class: if columns == 2 { "px-pages two" } else { "px-pages" }, id: "export-pages",
                        if show_original {
                            div { class: "px-col", id: "export-col-original",
                                if columns == 2 { div { class: "px-col-title", "Original" } }
                                for (n, url) in originals.iter().enumerate() {
                                    Page { key: "o{n}", n, url: url.clone(), kind: "original" }
                                }
                            }
                        }
                        if show_reader {
                            div { class: "px-col", id: "export-col-reader",
                                if columns == 2 { div { class: "px-col-title", "{device_name}" } }
                                if converted.is_empty() {
                                    div { class: "px-making", id: "export-making", Loading { label: "Making the pages\u{2026}" } }
                                }
                                for (n, url) in converted.iter().enumerate() {
                                    Page { key: "c{n}", n, url: url.clone(), kind: "converted" }
                                }
                            }
                        }
                    }
                    if total > shown {
                        p { class: "muted small px-more", id: "export-more", "Showing the first {shown} of {total} pages of {chapter.label(e.shown)}." }
                    }
                },
            }}
        }
    }
}

#[component]
fn Page(n: usize, url: String, kind: &'static str) -> Element {
    let mut failed = use_signal(|| false);
    rsx! {
        figure { class: "px-page {kind}", "data-page": "{n}",
            if url.is_empty() || failed() {
                figcaption { class: "muted small", "Page {n + 1} could not be shown." }
            } else {
                img { src: "{url}", alt: "Page {n + 1}", decoding: "async", onerror: move |_| failed.set(true) }
            }
        }
    }
}
