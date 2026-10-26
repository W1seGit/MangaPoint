//! The pieces of the page editor (every page of a chapter in one scroll, with the original beside
//! it) that both apps draw the same way. What goes inside them (the converted picture, how a
//! source is cropped, where thumbnails come from) and what a click does is the app's own.

use crate::logic::preview::Seam;
use crate::widgets::Icon;
use dioxus::prelude::*;

/// "Page [n] / total" at the top of the editor.
#[component]
pub fn PageJump(total: usize, number: usize, id: Option<&'static str>, on_go: EventHandler<usize>) -> Element {
    rsx! {
        label { class: "editor-page-jump",
            span { "Page" }
            input {
                id,
                r#type: "number", min: "1", max: "{total}", step: "1",
                value: "{number}", disabled: total == 0, "aria-label": "Go to page",
                onchange: move |ev| {
                    if let Ok(n) = ev.value().trim().parse::<usize>() {
                        if (1..=total).contains(&n) { on_go.call(n - 1); }
                    }
                },
            }
            span { class: "muted", "/ {total}" }
        }
    }
}

/// One page of the chapter in the scrolling list: the page as the reader will show it. The
/// picture (and whatever lies over it) is the children; the seam to the next page is drawn here.
#[component]
pub fn PageRow(
    index: usize,
    selected: bool,
    /// "Page 13", and when the source page makes several, which part this is.
    label: String,
    row_style: String,
    frame_style: String,
    seam: Option<Seam>,
    on_select: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        article {
            class: if selected { "editor-row selected" } else { "editor-row" },
            "data-row": "{index}",
            style: "{row_style}",
            onclick: move |_| { if !selected { on_select.call(()); } },
            div { class: "editor-row-label", "{label}" }
            div { class: "editor-row-panes",
                div { class: "row-pane result-pane",
                    div { class: "editor-reader-frame", style: "{frame_style}",
                        {children}
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

/// The source artwork beside a group of `count` reader pages: it stays in view while any of them
/// is. The children are the picture, or `SourceLoading`.
#[component]
pub fn SourceCell(count: usize, children: Element) -> Element {
    rsx! {
        div { class: "source-cell source-pane", style: "grid-row: 1 / span {count};",
            div { class: "source-sticky", {children} }
        }
    }
}

/// Shown in a `SourceCell` while its picture is not ready.
#[component]
pub fn SourceLoading() -> Element {
    rsx! { div { class: "editor-image-loading source-loading", "Preparing source\u{2026}" } }
}

/// A source picture as downloaded or imported, never split. Marks over it are the children.
#[component]
pub fn SourceArt(url: String, width: u32, height: u32, index: usize, children: Element) -> Element {
    rsx! {
        div { class: "editor-source-scroll",
            div {
                class: "editor-source-art",
                style: "aspect-ratio: {width} / {height}; --source-aspect: {width as f64 / height.max(1) as f64};",
                img { src: "{url}", alt: "Source artwork of page {index + 1}", draggable: "false" }
                {children}
            }
        }
    }
}

/// A page in the column of thumbnails; the picture is the children.
#[component]
pub fn Thumb(index: usize, selected: bool, on_open: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: if selected { "editor-thumb selected" } else { "editor-thumb" },
            "data-editor-page": "{index}",
            "aria-label": "Open page {index + 1}",
            "aria-current": if selected { "page" } else { "false" },
            onclick: move |_| on_open.call(()),
            div { class: "editor-thumb-art", {children} }
            span { "{index + 1}" }
        }
    }
}

/// Every page of the chapter at once, over the editor. The pages are `OverviewPage`s.
#[component]
pub fn Overview(total: usize, on_close: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div { class: "editor-overview-backdrop",
            section { class: "editor-overview", role: "dialog", "aria-modal": "true", "aria-label": "Chapter page overview",
                header {
                    strong { "Chapter overview \u{b7} {total} reader pages" }
                    button { id: "editor-close-overview", class: "btn small", onclick: move |_| on_close.call(()), Icon { name: "x", size: 16 } }
                }
                div { class: "editor-overview-grid", {children} }
            }
        }
    }
}

/// One page in the `Overview`; the picture is the children.
#[component]
pub fn OverviewPage(index: usize, selected: bool, on_open: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: if selected { "selected" } else { "" },
            onclick: move |_| on_open.call(()),
            div { class: "editor-overview-art", {children} }
            span { "Page {index + 1}" }
        }
    }
}

/// The picture of a page in a thumbnail or the overview: the image, or a placeholder when there is
/// none yet.
#[component]
pub fn PageImage(url: Option<String>, index: usize, size: u32) -> Element {
    rsx! {
        match url.filter(|u| !u.is_empty()) {
            Some(url) => rsx! { img { src: "{url}", alt: "Reader page {index + 1}" } },
            None => rsx! { Icon { name: "file-image", size } },
        }
    }
}

/// The rectangles of a picture that reader pages show: the page in view is lit, the others
/// outlined and numbered.
#[component]
pub fn SourceMarks(marks: Vec<(usize, [f32; 4], bool)>) -> Element {
    rsx! {
        for (page, r, on) in marks {
            div {
                key: "{page}",
                class: if on { "source-region on" } else { "source-region" },
                style: "left:{r[0] * 100.0}%;top:{r[1] * 100.0}%;width:{r[2] * 100.0}%;height:{r[3] * 100.0}%;",
                title: "Shown on page {page + 1}",
                span { "{page + 1}" }
            }
        }
    }
}
