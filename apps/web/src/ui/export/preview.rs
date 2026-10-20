//! The page editor of the Export screen: every page of a chapter in one continuous scroll, like a
//! PDF viewer, with the original beside each converted page. The desktop app's own screen
//! (`apps/desktop/src/ui/workspace/preview.rs`): the same top bar (page jump and zoom), the column
//! of thumbnails with Overview, the Compare tab, the seams of a strip and the Original / Reader
//! switch of a narrow window. Pages are shown as the converter made them; there is no crop editor.

use super::{edit, Look, PageSource, Zoom};
use crate::app::Ctx;
use crate::bridge;
use crate::model::Book;
use crate::ui::widgets::{EmptyState, Icon, ZoomDropdown, ZoomPick};
use dioxus::prelude::*;
use serde_json::json;

/// What the page after this one repeats of it: how many rows, and where those
/// rows sit on the page (as shares of the screen height).
#[derive(Clone, PartialEq)]
struct Seam {
    rows: u32,
    band_top: f32,
    band_height: f32,
}

/// The seam after each page of a strip chapter; `None` for the last page and
/// for chapters that are not strips.
fn seams(sources: &[PageSource], screen_height: u32) -> Vec<Option<Seam>> {
    let rows_of = |s: &PageSource| (s.part == "strip").then_some((s.top, s.height));
    (0..sources.len())
        .map(|i| {
            let (top, height) = rows_of(&sources[i])?;
            let (next_top, _) = rows_of(sources.get(i + 1)?)?;
            let rows = (top + height).saturating_sub(next_top);
            let screen = screen_height.max(1) as f32;
            Some(Seam {
                rows,
                band_top: height.saturating_sub(rows) as f32 / screen * 100.0,
                band_height: rows as f32 / screen * 100.0,
            })
        })
        .collect()
}

/// How tall a page is at this zoom, as a CSS length.
fn row_height(zoom: Zoom, width: u32, height: u32) -> String {
    match zoom {
        Zoom::Fit => format!("min(var(--reader-page-height),calc(var(--reader-page-width) * {height} / {width}))"),
        Zoom::Width => format!("calc(var(--reader-page-width) * {height} / {width})"),
        Zoom::Custom(percent) => format!("{}px", height * percent as u32 / 100),
    }
}

fn reader_style(zoom: Zoom, width: u32, height: u32) -> String {
    let size = match zoom {
        Zoom::Fit => format!("height:{};width:auto;", row_height(zoom, width, height)),
        Zoom::Width => "height:auto;width:100%;".to_string(),
        Zoom::Custom(percent) => format!("height:{}px;width:{}px;", height * percent as u32 / 100, width * percent as u32 / 100),
    };
    format!("{size}aspect-ratio:{width}/{height};")
}

/// Which pages share a picture: a double page split in two, or a page zoomed into pieces, is several
/// reader pages from one source page. Every other page is a group of its own. As
/// `(first, one past the last)`.
fn groups(sources: &[PageSource]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, page) in sources.iter().enumerate() {
        match out.last_mut() {
            Some(last) if page.image.is_some() && sources[last.0].image == page.image => {
                last.1 = i + 1;
            }
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// What a page is of its source page, when the source page makes several
/// ("left half of source page 12"): which one of them this is.
fn part_note(page: &PageSource, k: usize, of: usize) -> Option<String> {
    let image = page.image?;
    if of < 2 {
        return None;
    }
    let what = match page.part.as_str() {
        "left" => "left half".to_string(),
        "right" => "right half".to_string(),
        _ => format!("part {} of {of}", k + 1),
    };
    Some(format!("{what} of source page {}", image + 1))
}

/// One page of the chapter in the scrolling list: the page as the reader will show it.
#[component]
fn EditorRow(index: usize, selected: bool, url: String, label: String, row_style: String, frame_style: String, seam: Option<Seam>) -> Element {
    let ctx = use_context::<Ctx>();
    let mut failed = use_signal(|| false);
    rsx! {
        article {
            class: if selected { "editor-row selected" } else { "editor-row" },
            "data-row": "{index}",
            style: "{row_style}",
            onclick: move |_| { if !selected { edit(ctx, |e| e.selected = index); } },
            div { class: "editor-row-label", "{label}" }
            div { class: "editor-row-panes",
                div { class: "row-pane result-pane",
                    div { class: "editor-reader-frame", style: "{frame_style}",
                        if url.is_empty() || failed() {
                            div { class: "editor-image-loading", "Page {index + 1} could not be shown." }
                        } else {
                            img { src: "{url}", alt: "Page {index + 1} exactly as exported to the reader", draggable: "false", onerror: move |_| failed.set(true) }
                        }
                        if let Some(seam) = seam.as_ref().filter(|s| s.rows > 0) {
                            div {
                                class: "editor-seam-band",
                                style: "top: {seam.band_top}%; height: {seam.band_height}%;",
                                title: "The next page starts with these rows again",
                                span { "Repeated on the next page" }
                            }
                        }
                    }
                }
            }
            if let Some(seam) = seam {
                div { class: if seam.rows > 0 { "editor-seam-note repeats" } else { "editor-seam-note clean" },
                    if seam.rows > 0 {
                        Icon { name: "layers", size: 13 }
                        "Next page repeats these last {seam.rows} rows"
                    } else {
                        Icon { name: "check", size: 13 }
                        "Clean cut: the next page starts right below"
                    }
                }
            }
        }
    }
}

/// The source artwork beside a group of reader pages. It stays in view while any of the group's
/// pages is. It is the picture as it was imported, never the split.
#[component]
fn SourceCell(first: usize, count: usize, original: Option<(String, u32, u32)>) -> Element {
    rsx! {
        div { class: "source-cell source-pane", style: "grid-row: 1 / span {count};",
            div { class: "source-sticky",
                if let Some((url, width, height)) = original {
                    div { class: "editor-source-scroll",
                        div {
                            class: "editor-source-art",
                            style: "aspect-ratio: {width} / {height}; --source-aspect: {width as f64 / height.max(1) as f64};",
                            img { src: "{url}", alt: "Source artwork of page {first + 1}", draggable: "false" }
                        }
                    }
                } else {
                    div { class: "editor-image-loading source-loading", "Preparing source…" }
                }
            }
        }
    }
}

/// The zoom of the page preview, in the shared zoom menu.
#[component]
fn ZoomMenu() -> Element {
    let ctx = use_context::<Ctx>();
    let selected = match ctx.export.read().zoom {
        Zoom::Fit => ZoomPick::Fit,
        Zoom::Width => ZoomPick::Width,
        Zoom::Custom(percent) => ZoomPick::Percent(percent),
    };
    rsx! {
        ZoomDropdown {
            selected,
            percents: vec![50u16, 75, 90, 100, 125, 150, 200],
            min: Zoom::MIN_PERCENT,
            max: Zoom::MAX_PERCENT,
            onpick: move |pick| {
                edit(ctx, |e| e.zoom = match pick {
                    ZoomPick::Fit => Zoom::Fit,
                    ZoomPick::Width => Zoom::Width,
                    ZoomPick::Percent(percent) => Zoom::Custom(percent),
                });
            },
        }
    }
}

/// Show page `index`: scroll the list and the thumbnails to it.
fn select_page(ctx: Ctx, index: usize) {
    edit(ctx, |e| {
        e.selected = index;
        e.overview = false;
    });
    bridge::send("editor_scroll", json!({ "index": index }));
}

#[component]
pub fn Preview(book: Book) -> Element {
    let ctx = use_context::<Ctx>();
    use_effect(move || bridge::send("editor_start", json!({})));
    let e = ctx.export.read().clone();
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
    let seams = seams(&sources, device.height);
    let chapter = e.shown;
    let device_name = device.name.clone();
    rsx! {
        div { id: "page-editor", class: "{classes}", tabindex: "0",
            div { class: "editor-toolbar",
                label { class: "editor-page-jump",
                    span { "Page" }
                    input {
                        id: "export-page-jump", r#type: "number", min: "1", max: "{total}", step: "1",
                        value: "{number}", disabled: total == 0, "aria-label": "Go to page",
                        onchange: move |ev| {
                            if let Ok(n) = ev.value().trim().parse::<usize>() {
                                if (1..=total).contains(&n) { select_page(ctx, n - 1); }
                            }
                        },
                    }
                    span { class: "muted", "/ {total}" }
                }
                div { class: "editor-toolbar-actions",
                    ZoomMenu {}
                }
            }
            if let Look::Failed(message) = &e.look { div { class: "banner bad small", "{message}" } }
            div { class: "editor-body",
                nav { class: "editor-thumbnails", id: "export-thumbnails", "aria-label": "Reader pages",
                    for (i, url) in converted.iter().enumerate() {
                        button {
                            key: "{i}",
                            class: if i == selected { "editor-thumb selected" } else { "editor-thumb" },
                            "data-editor-page": "{i}",
                            "aria-label": "Open page {i + 1}",
                            "aria-current": if i == selected { "page" } else { "false" },
                            onclick: move |_| select_page(ctx, i),
                            div { class: "editor-thumb-art",
                                if url.is_empty() { Icon { name: "file-image", size: 23 } } else { img { src: "{url}", alt: "Reader page {i + 1}" } }
                            }
                            span { "{i + 1}" }
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
                        for (start, end) in groups(&sources) {
                            div {
                                key: "{chapter}/g{start}",
                                class: match (compare, end - start) {
                                    (false, _) => "editor-group",
                                    (true, 1) => "editor-group comparing single",
                                    (true, _) => "editor-group comparing",
                                },
                                style: "{row_style}",
                                if compare {
                                    SourceCell {
                                        first: start,
                                        count: end - start,
                                        original: sources[start].image.and_then(|img| {
                                            let url = originals.get(img)?.clone();
                                            let (w, h) = sizes.get(img).copied().filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((600, 900));
                                            (!url.is_empty()).then_some((url, w, h))
                                        }),
                                    }
                                }
                                for i in start..end {
                                    EditorRow {
                                        key: "{chapter}/{i}",
                                        index: i,
                                        selected: i == selected,
                                        url: converted.get(i).cloned().unwrap_or_default(),
                                        label: match part_note(&sources[i], i - start, end - start) {
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
                div { class: "editor-overview-backdrop",
                    section { class: "editor-overview", role: "dialog", "aria-modal": "true", "aria-label": "Chapter page overview",
                        header {
                            strong { "Chapter overview · {total} reader pages" }
                            button { id: "editor-close-overview", class: "btn small", onclick: move |_| edit(ctx, |e| e.overview = false), Icon { name: "x", size: 16 } }
                        }
                        div { class: "editor-overview-grid",
                            for (i, url) in converted.iter().enumerate() {
                                button {
                                    key: "{i}",
                                    class: if i == selected { "selected" } else { "" },
                                    onclick: move |_| select_page(ctx, i),
                                    div { class: "editor-overview-art",
                                        if url.is_empty() { Icon { name: "file-image", size: 24 } } else { img { src: "{url}", alt: "Reader page {i + 1}" } }
                                    }
                                    span { "Page {i + 1}" }
                                }
                            }
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
