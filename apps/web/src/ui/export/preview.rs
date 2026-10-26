//! The page editor of the Export screen: every page of a chapter in one continuous scroll, like a
//! PDF viewer, with the original beside each converted page. The desktop app's own screen
//! (`apps/desktop/src/ui/workspace/preview.rs`): the same top bar (page jump and zoom), the column
//! of thumbnails with Overview, the Compare tab, the seams of a strip and the Original / Reader
//! switch of a narrow window. Pages are shown as the converter made them; there is no crop editor.

use super::{edit, Look, PageSource, Zoom};
use crate::app::Ctx;
use crate::bridge;
use crate::model::Book;
use crate::ui::widgets::{EmptyState, Loading};
use dioxus::prelude::*;
use mangapoint_ui::logic::preview::{groups, part_note, reader_style, row_height, seams, PageSlot};
use mangapoint_ui::page_editor as editor;
use serde_json::json;

/// One page of the chapter in the scrolling list: the page as the reader will show it.
#[component]
fn BrowserRow(index: usize, selected: bool, url: String, label: String, row_style: String, frame_style: String, seam: Option<mangapoint_ui::logic::preview::Seam>) -> Element {
    let ctx = use_context::<Ctx>();
    let mut failed = use_signal(|| false);
    rsx! {
        editor::PageRow { index, selected, label, row_style, frame_style, seam, on_select: move |_| edit(ctx, |e| e.selected = index),
            if url.is_empty() || failed() {
                div { class: "editor-image-loading", "Page {index + 1} could not be shown." }
            } else {
                img { src: "{url}", alt: "Page {index + 1} exactly as exported to the reader", draggable: "false", onerror: move |_| failed.set(true) }
            }
        }
    }
}

/// The picture a group of pages was made from: an imported page, or for a strip the downloaded
/// rows. `None` while it is not ready.
fn source_of(sources: &[PageSource], originals: &[String], sizes: &[(u32, u32)], start: usize) -> Option<(String, u32, u32)> {
    match sources[start].image {
        Some(img) => {
            let url = originals.get(img)?.clone();
            let (w, h) = sizes.get(img).copied().filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((600, 900));
            (!url.is_empty()).then_some((url, w, h))
        }
        None => {
            let s = &sources[start];
            (!s.strip_url.is_empty() && s.rows_width > 0 && s.rows_height > 0).then(|| (s.strip_url.clone(), s.rows_width, s.rows_height))
        }
    }
}

/// The zoom of the page preview, in the shared zoom menu.
#[component]
fn BrowserZoomMenu() -> Element {
    let ctx = use_context::<Ctx>();
    let zoom = ctx.export.read().zoom;
    rsx! { mangapoint_ui::export::PreviewZoom { zoom, on_change: move |zoom| { edit(ctx, |e| e.zoom = zoom); } } }
}

/// Show page `index`: scroll the list and the thumbnails to it.
fn select_page(ctx: Ctx, index: usize) {
    edit(ctx, |e| {
        e.selected = index;
        e.overview = false;
    });
    bridge::send("editor_scroll", json!({ "index": index }));
}

/// The pages of a chapter for a phone or tablet: the same top bar as the other readers (page jump
/// and zoom), and the pages as they were imported, one after the other like a long page, with
/// nothing between them. Nothing is converted, so there is nothing to compare.
#[component]
fn OriginalPages(book: Book) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let chapter = book.chapters.get(e.shown).cloned();
    let from_library = book.id != super::SAMPLE;
    let total = match (&chapter, &e.look) {
        (Some(c), _) if from_library => c.count,
        (_, Look::Ready { originals, .. }) => originals.len(),
        _ => 0,
    };
    let selected = e.selected.min(total.saturating_sub(1));
    let number = if total == 0 { 0 } else { selected + 1 };
    // Fit shows a whole page (as tall as the window, never wider than the usual page), Width fills
    // the column, and a percentage is that much of the page's own size. The style sheet does the rest.
    let (mode, width) = match e.zoom {
        Zoom::Fit => ("fit", String::new()),
        Zoom::Width => ("width", String::new()),
        Zoom::Custom(percent) => ("custom", format!("--zoom:{};", percent as f32 / 100.0)),
    };
    rsx! {
        div { id: "page-editor", class: "page-editor original-editor", tabindex: "0",
            div { class: "editor-toolbar",
                editor::PageJump { id: "export-page-jump", total, number, on_go: move |i| select_page(ctx, i) }
                div { class: "editor-toolbar-actions",
                    BrowserZoomMenu {}
                }
            }
            div { class: "editor-body",
                nav { class: "editor-thumbnails", id: "export-thumbnails", "aria-label": "Pages",
                    if let (Some(chapter), true) = (&chapter, from_library) {
                        for n in 0..chapter.count {
                            editor::Thumb { key: "{book.id}-{chapter.first + n}", index: n, selected: n == selected, on_open: move |_| select_page(ctx, n),
                                div { class: "reader-tile original-thumb", "data-book": "{book.id}", "data-page": "{chapter.first + n}" }
                            }
                        }
                    } else if let Look::Ready { originals, .. } = &e.look {
                        for (n, url) in originals.iter().enumerate() {
                            editor::Thumb { key: "{n}", index: n, selected: n == selected, on_open: move |_| select_page(ctx, n),
                                editor::PageImage { url: Some(url.clone()), index: n, size: 23 }
                            }
                        }
                    }
                }
                main { class: "editor-main",
                    div { class: "editor-scroll", id: "export-pages",
                        div { class: "original-preview continuous", id: "export-preview", "data-zoom": "{mode}", style: "{width}",
                            // A book of the library: every page of the chapter is a tile that the page glue fills as
                            // it comes near the screen (the reader's own tiles), so a long chapter is not loaded at once.
                            if let (Some(chapter), true) = (&chapter, from_library) {
                                for n in 0..chapter.count {
                                    div {
                                        class: "reader-tile original-tile", key: "{book.id}-{chapter.first + n}",
                                        "data-row": "{n}", "data-book": "{book.id}", "data-page": "{chapter.first + n}",
                                    }
                                }
                            } else {
                                {match &e.look {
                                    Look::Idle => rsx! { Loading { label: "Opening the pages\u{2026}" } },
                                    Look::Failed(message) => rsx! { EmptyState { title: "These pages could not be shown", message: "{message}", compact: true } },
                                    Look::Ready { originals, total, shown, .. } => rsx! {
                                        for (n, url) in originals.iter().enumerate() {
                                            if !url.is_empty() {
                                                img { key: "{n}", class: "original-img original-tile", "data-row": "{n}", src: "{url}", alt: "Page {n + 1}", decoding: "async" }
                                            }
                                        }
                                        if total > shown {
                                            p { class: "muted small px-more", id: "export-more", "Showing the first {shown} of {total} pages of this chapter." }
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
}

#[component]
pub fn Preview(book: Book) -> Element {
    let ctx = use_context::<Ctx>();
    // The page list is a new element when the original pages and the converted ones swap, so it is sized again.
    let original = use_memo(move || device_profiles::is_original(&ctx.export.read().opts.device));
    use_effect(move || {
        let _ = original();
        bridge::send("editor_start", json!({}));
    });
    let e = ctx.export.read().clone();
    if device_profiles::is_original(&e.opts.device) {
        return rsx! { OriginalPages { book: book.clone() } };
    }
    let Some(device) = super::device_of(&e.opts) else {
        return rsx! { EmptyState { title: "Nothing to show", message: "Choose a reader first.", compact: true } };
    };
    if book.chapters.get(e.shown).is_none() {
        return rsx! { EmptyState { title: "Nothing to show", message: "This book has no chapters.", compact: true } };
    }
    let (converted, sources, originals, sizes, total_source, shown_source) = match &e.look {
        Look::Ready { originals, converted, sources, sizes, total, shown } => (converted.clone(), sources.clone(), originals.clone(), sizes.clone(), *total, *shown),
        _ => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0, 0),
    };
    let total = converted.len();
    let selected = e.selected.min(total.saturating_sub(1));
    let number = if total == 0 { 0 } else { selected + 1 };
    let frame_style = reader_style(e.zoom, device.width, device.height);
    let row_style = format!("--row-h:{};", row_height(e.zoom, device.width, device.height));
    let classes = if e.source_focus { "page-editor source-focused" } else { "page-editor" };
    let compare = e.compare;
    let strip = sources.iter().any(|s| s.part == "strip");
    let slots: Vec<PageSlot> = sources.iter().map(PageSource::slot).collect();
    let seams = seams(&slots, device.height);
    let chapter = e.shown;
    let device_name = device.name.clone();
    rsx! {
        div { id: "page-editor", class: "{classes}", tabindex: "0",
            div { class: "editor-toolbar",
                editor::PageJump { id: "export-page-jump", total, number, on_go: move |i| select_page(ctx, i) }
                div { class: "editor-toolbar-actions",
                    BrowserZoomMenu {}
                }
            }
            if let Look::Failed(message) = &e.look { div { class: "banner bad small", "{message}" } }
            div { class: "editor-body",
                nav { class: "editor-thumbnails", id: "export-thumbnails", "aria-label": "Reader pages",
                    for (i, url) in converted.iter().enumerate() {
                        editor::Thumb { key: "{i}", index: i, selected: i == selected, on_open: move |_| select_page(ctx, i),
                            editor::PageImage { url: Some(url.clone()), index: i, size: 23 }
                        }
                    }
                    button {
                        class: "btn small editor-overview-button", id: "export-overview",
                        title: "See every page of the chapter at once",
                        onclick: move |_| edit(ctx, |e| e.overview = true),
                        "Overview"
                    }
                }
                main { class: "editor-main",
                    div { class: "editor-mobile-switch",
                        button { id: "export-tab-reader", class: if !e.source_focus { "selected" } else { "" },
                            onclick: move |_| edit(ctx, |e| e.source_focus = false), "Reader" }
                        button { id: "export-tab-original", class: if e.source_focus { "selected" } else { "" },
                            onclick: move |_| edit(ctx, |e| { e.source_focus = true; e.compare = true; }), "Original" }
                    }
                    div { class: "editor-scroll", id: "export-pages",
                        div { class: if compare { "editor-columns comparing" } else { "editor-columns solo" },
                            button {
                                class: if compare { "compare-tab on" } else { "compare-tab" }, id: "export-compare",
                                title: "Show the imported artwork beside each reader page",
                                "aria-pressed": "{compare}",
                                onclick: move |_| edit(ctx, |e| e.compare = !compare),
                                "Compare"
                            }
                            if compare {
                                span { class: "source-pane",
                                    if strip { "Source section · joined strip" } else { "Source artwork" }
                                }
                            }
                            span { class: "result-pane", title: "{device_name}", "Reader result · {device.width} × {device.height}" }
                        }
                        if total == 0 && !matches!(e.look, Look::Failed(_)) {
                            div { class: "editor-image-loading", id: "export-making", "Preparing the chapter…" }
                        }
                        for (start, end) in groups(&slots) {
                            div {
                                key: "{chapter}/g{start}",
                                class: match (compare, end - start) {
                                    (false, _) => "editor-group",
                                    (true, 1) => "editor-group comparing single",
                                    (true, _) => "editor-group comparing",
                                },
                                style: "{row_style}",
                                if compare {
                                    editor::SourceCell { count: end - start,
                                        match source_of(&sources, &originals, &sizes, start) {
                                            Some((url, w, h)) => rsx! { editor::SourceArt { url, width: w, height: h, index: start } },
                                            None => rsx! { editor::SourceLoading {} },
                                        }
                                    }
                                }
                                for i in start..end {
                                    BrowserRow {
                                        key: "{chapter}/{i}",
                                        index: i,
                                        selected: i == selected,
                                        url: converted.get(i).cloned().unwrap_or_default(),
                                        label: match part_note(&slots[i], i - start, end - start) {
                                            Some(note) => format!("Page {} · {note}", i + 1),
                                            None => format!("Page {}", i + 1),
                                        },
                                        row_style: row_style.clone(),
                                        frame_style: frame_style.clone(),
                                        seam: seams.get(i).cloned().flatten(),
                                    }
                                }
                            }
                        }
                        if total_source > shown_source {
                            p { class: "muted small px-more", id: "export-more", "Showing the first {shown_source} of {total_source} pages of this chapter." }
                        }
                    }
                }
            }
            if e.overview {
                editor::Overview { total, on_close: move |_| edit(ctx, |e| e.overview = false),
                    for (i, url) in converted.iter().enumerate() {
                        editor::OverviewPage { key: "{i}", index: i, selected: i == selected, on_open: move |_| select_page(ctx, i),
                            editor::PageImage { url: Some(url.clone()), index: i, size: 24 }
                        }
                    }
                }
            }
        }
    }
}

/// The page at the top of the list changed because the list was scrolled.
pub fn scrolled(ctx: Ctx, current: i64) {
    if current < 0 {
        return;
    }
    let current = current as usize;
    let differs = ctx.export.peek().selected != current;
    if differs {
        edit(ctx, |e| e.selected = current);
        bridge::send("editor_thumb", json!({ "index": current }));
    }
}
