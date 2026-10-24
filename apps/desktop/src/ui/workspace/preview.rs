//! The page editor: every page of a chapter in one continuous scroll, like a
//! PDF viewer, with the original beside each converted page.
use super::crop_editor::SourceCanvas;
use crate::state::AppState;
use crate::ui::cat::CirclingCat;
use crate::ui::widgets::EmptyState;
use crate::ui::work::{prepare_preview, schedule_preview, select_preview_page};
use converter::plan::Source;
use dioxus::prelude::*;
use library::CropRect;
use mangapoint_ui::logic::preview::{groups, part_note, reader_style, row_height, seams, PageSlot, SlotPart};
use mangapoint_ui::page_editor as editor;

/// Where a page sits in its source page, for the layout shared with the website.
fn slot(page: &crate::state::PreviewPage) -> PageSlot {
    use converter::plan::Part;
    let (part, top, height) = match page.source.as_ref().map(|p| p.source) {
        Some(Source::Strip { top, height }) => (SlotPart::Strip, top, height),
        Some(Source::Image { part: Part::Half(0), .. }) => (SlotPart::Left, 0, 0),
        Some(Source::Image { part: Part::Half(_), .. }) => (SlotPart::Right, 0, 0),
        Some(Source::Image { part: Part::Whole | Part::Rotated, .. }) | None => (SlotPart::Whole, 0, 0),
        Some(_) => (SlotPart::Piece, 0, 0),
    };
    PageSlot { image: page.image, part, top, height }
}


/// Starts the page editor's script (`crates/ui/js/preview-editor.js`): it sizes the viewer to the
/// window and reports which pages are on screen (as `[first, last, current]`, `-1` when there are
/// none) over the eval's channel, so it stays alive.
pub(crate) fn editor_script() -> String {
    let source = mangapoint_ui::scripts::source("preview_editor").unwrap_or_default();
    format!("{source}\n;window.MP.preview_editor.start((m) => dioxus.send(m));\nawait new Promise(() => {{}});")
}

/// The source artwork beside a group of reader pages. It stays in view while
/// any of the group's pages is, and marks which part of the picture the page
/// being looked at shows. It is the picture as downloaded, never the split.
#[component]
fn SourceCell(
    first: usize,
    pages: Vec<std::rc::Rc<crate::state::PreviewPage>>,
    /// The page of the group in view, as an index into the chapter.
    selected: Option<usize>,
    chapter: String,
    editing: bool,
    guide: bool,
    draft: Signal<CropRect>,
) -> Element {
    let current = selected.and_then(|i| pages.get(i.checked_sub(first)?));
    let lead = current.or(pages.first());
    // While a crop is edited it is measured on the part of the picture the
    // page shows, so that part is what is drawn.
    let partial = current.is_some_and(|p| {
        matches!(
            p.source.as_ref().map(|s| s.source),
            Some(Source::Image { part, .. }) if part != converter::plan::Part::Whole
        )
    });
    let editing_piece = editing && partial;
    let source = if editing_piece {
        current.and_then(|p| p.piece.clone())
    } else {
        lead.and_then(|p| p.original.clone())
            .or_else(|| pages.iter().find_map(|p| p.original.clone()))
    };
    let marks: Vec<(usize, [f32; 4], bool)> = if editing_piece {
        Vec::new()
    } else {
        pages
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let region = p.region.filter(|r| !r.is_whole())?;
                Some((first + k, region.page, Some(first + k) == selected))
            })
            .collect()
    };
    // Where the crop box sits: on the picture, inside the piece it is measured on.
    let frame = match (editing_piece, current.and_then(|p| p.region)) {
        (true, _) => Some([0.0, 0.0, 1.0, 1.0]),
        (false, Some(region)) => Some(region.piece),
        (false, None) if partial => None,
        (false, None) => Some([0.0, 0.0, 1.0, 1.0]),
    };
    rsx! {
        editor::SourceCell { count: pages.len(),
            if let Some(source) = source {
                if let (Some(current), Some(frame)) = (current, frame) {
                    SourceCanvas {
                        key: "{chapter}/{first}",
                        source,
                        draft,
                        crop_key: current.crop_key.clone(),
                        editing,
                        guide,
                        frame,
                        marks,
                    }
                } else {
                    editor::SourceArt { url: source.url.clone(), width: source.width, height: source.height, index: first,
                        editor::SourceMarks { marks }
                    }
                }
            } else {
                editor::SourceLoading {}
            }
        }
    }
}

/// Preview another downloaded chapter, from its first page.
pub(crate) fn switch_chapter(state: AppState, key: String) {
    let mut pv = state.preview;
    let mut w = pv.write();
    if w.chapter_key.as_deref() == Some(key.as_str()) {
        return;
    }
    w.chapter_key = Some(key);
    w.selected = 0;
    w.thumb_start = 0;
    w.window = (0, 0);
    w.pages.clear();
    w.draft_crop = None;
    w.shading_samples.clear();
    drop(w);
    spawn(async move {
        let _ = document::eval(&mangapoint_ui::scripts::call_method("preview_editor", "top", "").unwrap_or_default()).await;
    });
    schedule_preview(state, 0);
}

/// The pages now on screen changed: draw them, and follow the page at the top.
fn track_scroll(state: AppState, first: i64, last: i64, current: i64) {
    if first < 0 || last < 0 {
        return;
    }
    let (window, selected) = ((first as usize, last as usize), current.max(0) as usize);
    let (window_changed, page_changed) = {
        let mut preview = state.preview;
        let mut p = preview.write();
        let total = p.pages.len();
        if total == 0 {
            return;
        }
        let window = (window.0.min(total - 1), window.1.min(total - 1));
        let window_changed = p.window != window;
        p.window = window;
        // While a crop is being dragged, the page stays where it is.
        let selected = selected.min(total - 1);
        let page_changed = !p.view.editing_crop && p.selected != selected;
        if page_changed {
            p.selected = selected;
            p.draft_crop = None;
        }
        (window_changed, page_changed)
    };
    if page_changed {
        let selected = state.preview.peek().selected;
        spawn(async move {
            let js = mangapoint_ui::scripts::call_method("preview_editor", "thumb", &format!("{{index:{selected}}}")).unwrap_or_default();
            let _ = document::eval(&js).await;
        });
    }
    if window_changed || page_changed {
        schedule_preview(state, 120);
    }
}

/// The zoom of the page preview, in the shared zoom menu.
#[component]
pub(crate) fn ZoomMenu() -> Element {
    let mut state = use_context::<AppState>();
    let zoom = state.preview.read().view.zoom;
    rsx! { mangapoint_ui::export::PreviewZoom { zoom, on_change: move |zoom| { state.preview.write().view.zoom = zoom; } } }
}

#[component]
pub fn PreviewTab() -> Element {
    let mut state = use_context::<AppState>();
    let mut draft = state.crop_draft;
    let mut identity = use_signal(String::new);
    use_effect(move || prepare_preview(state));
    use_future(move || async move {
        let mut eval = document::eval(&editor_script());
        while let Ok(message) = eval.recv::<Vec<i64>>().await {
            if let [first, last, current] = message.as_slice() {
                track_scroll(state, *first, *last, *current);
            }
        }
    });
    use_effect(move || {
        let p = state.preview.read();
        let ws = state.workspace.read();
        let lib = state.library.read();
        if let (Some(ws), Some(chapter), Some(page)) =
            (ws.as_ref(), p.chapter_key.as_ref(), p.pages.get(p.selected))
        {
            if let Some(series) = lib.find(&ws.series_id) {
                let crop = series
                    .crops
                    .effective(chapter, &page.crop_key)
                    .unwrap_or_default();
                let next = format!("{}/{}/{}:{crop:?}", ws.series_id, chapter, page.crop_key);
                if *identity.peek() != next {
                    identity.set(next);
                    draft.set(crop);
                }
            }
        }
    });
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    if state.library.read().find(&ws.series_id).is_none() {
        return rsx! {EmptyState{title:"Nothing to edit yet",message:"Download a chapter to open it in the editor."}};
    }
    let p = state.preview.read().clone();
    let view = p.view;
    let chapter = p.chapter_key.clone().unwrap_or_default();
    let total = p.pages.len();
    let selected = p.selected.min(total.saturating_sub(1));
    let number = if total == 0 { 0 } else { selected + 1 };
    let device = state.ui.settings.read().device_profile();
    let frame_style = reader_style(view.zoom, device.width, device.height);
    let row_style = format!(
        "--row-h:{};",
        row_height(view.zoom, device.width, device.height)
    );
    let classes = format!(
        "page-editor{}{}",
        "",
        if view.source_focus {
            " source-focused"
        } else {
            ""
        }
    );
    let compare = view.show_original;
    let slots: Vec<PageSlot> = p.pages.iter().map(|page| slot(page)).collect();
    let seams = seams(&slots, device.height);
    rsx! {
        div { id: "page-editor", class: "{classes}", tabindex: "0",
            div { class: "editor-toolbar",
                editor::PageJump { total, number, on_go: move |i| select_preview_page(state, i) }
                div { class: "editor-toolbar-actions",
                    ZoomMenu {}
                }
            }
            if let Some(error) = &p.error { div { class: "banner bad small", "{error}" } }
            div { class: "editor-body",
                nav {
                    class: "editor-thumbnails", "aria-label": "Reader pages",
                    onscroll: move |e| {
                        let start = (e.data.scroll_top() / 132.0).floor().max(0.0) as usize;
                        let needs = {
                            let mut pv = state.preview;
                            let mut w = pv.write();
                            if w.thumb_start != start { w.thumb_start = start; true } else { false }
                        };
                        if needs && !state.preview.peek().overview { schedule_preview(state, 80); }
                    },
                    for (i, page) in p.pages.iter().enumerate() {
                        editor::Thumb { key: "{i}", index: i, selected: i == selected, on_open: move |_| select_preview_page(state, i),
                            editor::PageImage { url: page.thumbnail.as_ref().or(page.shot.as_ref()).map(|s| s.url.clone()), index: i, size: 23 }
                        }
                    }
                    button {
                        class: "btn small editor-overview-button",
                        title: "See every page of the chapter at once",
                        onclick: move |_| {
                            state.preview.write().overview = true;
                            schedule_preview(state, 0);
                        },
                        "Overview"
                    }
                }
                main { class: "editor-main",
                    div { class: "editor-mobile-switch",
                        button {
                            class: if !view.source_focus { "selected" } else { "" },
                            onclick: move |_| { state.preview.write().view.source_focus = false; },
                            "Reader result"
                        }
                        button {
                            class: if view.source_focus { "selected" } else { "" },
                            onclick: move |_| {
                                let mut pv = state.preview;
                                let mut w = pv.write();
                                w.view.source_focus = true;
                                w.view.show_original = true;
                            },
                            "Source"
                        }
                    }
                    div { class: "editor-scroll",
                        div { class: if compare { "editor-columns comparing" } else { "editor-columns solo" },
                            button {
                                class: if compare { "compare-tab on" } else { "compare-tab" },
                                title: "Show the downloaded artwork beside each reader page",
                                "aria-pressed": "{compare}",
                                onclick: move |_| {
                                    state.preview.write().view.show_original = !compare;
                                    schedule_preview(state, 0);
                                },
                                "Compare"
                            }
                            if compare {
                                span { class: "source-pane",
                                    if p.strip { "Source section · joined strip" } else { "Source artwork" }
                                    if view.editing_crop { " · drag the edges to crop" }
                                }
                            }
                            span { class: "result-pane", title: "{device.name}", "Reader result · {device.width} × {device.height}" }
                        }
                        if total == 0 && p.error.is_none() {
                            div { class: "editor-image-loading", "Preparing the chapter…" }
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
                                    SourceCell {
                                        first: start,
                                        pages: p.pages[start..end].to_vec(),
                                        selected: (start..end).contains(&selected).then_some(selected),
                                        chapter: chapter.clone(),
                                        editing: view.editing_crop,
                                        guide: view.show_crop,
                                        draft,
                                    }
                                }
                                for i in start..end {
                                    {
                                        let page = &p.pages[i];
                                        // Darkened only while this page is being drawn again: a page drawn the
                                        // way it should look is left alone, whatever else is loading.
                                        let busy = (i + 1 >= p.window.0 && i <= p.window.1 + 1)
                                            && (page.shot.is_none() || (p.busy && page.rendered_for != p.signature));
                                        rsx! {
                                            editor::PageRow {
                                                key: "{chapter}/{i}",
                                                index: i,
                                                selected: i == selected,
                                                label: match part_note(&slots[i], i - start, end - start) {
                                                    Some(note) => format!("Page {} · {note}", i + 1),
                                                    None => format!("Page {}", i + 1),
                                                },
                                                row_style: row_style.clone(),
                                                frame_style: frame_style.clone(),
                                                seam: seams.get(i).cloned().flatten(),
                                                on_select: move |_| {
                                                    let mut preview = state.preview;
                                                    let mut p = preview.write();
                                                    p.selected = i;
                                                    p.draft_crop = None;
                                                },
                                                if let Some(shot) = page.shot.as_ref() {
                                                    img { src: "{shot.url}", alt: "Page {i + 1} exactly as exported to the reader", draggable: "false" }
                                                } else {
                                                    div { class: "editor-image-loading" }
                                                }
                                                if busy {
                                                    div { class: "render-veil", CirclingCat { size: 96 } }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if p.overview {
                editor::Overview { total, on_close: move |_| { state.preview.write().overview = false; },
                    for (i, page) in p.pages.iter().enumerate() {
                        editor::OverviewPage { key: "{i}", index: i, selected: i == selected, on_open: move |_| select_preview_page(state, i),
                            editor::PageImage { url: page.thumbnail.as_ref().or(page.shot.as_ref()).map(|s| s.url.clone()), index: i, size: 24 }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::plan::{Part, Piece};

    fn page(image: Option<usize>, part: Part) -> crate::state::PreviewPage {
        crate::state::PreviewPage {
            image,
            source: image.map(|index| converter::PlannedPage {
                source: Source::Image { index, part },
                unit_top: 0,
                unit_height: 0,
                landscape: false,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_planned_part_becomes_the_shared_slot() {
        assert_eq!(slot(&page(Some(1), Part::Half(0))).part, SlotPart::Left);
        assert_eq!(slot(&page(Some(1), Part::Half(1))).part, SlotPart::Right);
        assert_eq!(slot(&page(Some(2), Part::Whole)).part, SlotPart::Whole);
        let tile = page(Some(4), Part::Tile { of: Piece::Whole, col: 1, row: 0 });
        assert_eq!(slot(&tile), PageSlot { image: Some(4), part: SlotPart::Piece, top: 0, height: 0 });
        assert_eq!(slot(&page(None, Part::Whole)).part, SlotPart::Whole);
    }
}
