//! The page editor: every page of a chapter in one continuous scroll, like a
//! PDF viewer, with the original beside each converted page.
use super::crop_editor::SourceCanvas;
use crate::state::{AppState, Shot, Zoom};
use crate::ui::cat::CirclingCat;
use crate::ui::widgets::{EmptyState, Icon, ZoomDropdown, ZoomPick};
use crate::ui::work::{prepare_preview, schedule_preview, select_preview_page};
use converter::plan::Source;
use dioxus::prelude::*;
use library::CropRect;

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
fn seams(
    pages: &[std::rc::Rc<crate::state::PreviewPage>],
    screen_height: u32,
) -> Vec<Option<Seam>> {
    let rows_of = |page: &crate::state::PreviewPage| match page.source.as_ref().map(|p| p.source) {
        Some(Source::Strip { top, height }) => Some((top, height)),
        _ => None,
    };
    (0..pages.len())
        .map(|i| {
            let (top, height) = rows_of(&pages[i])?;
            let (next_top, _) = rows_of(pages.get(i + 1)?)?;
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

/// Sizes the viewer to the window, reports which pages are on screen (as
/// `[first, last, current]`, `-1` when there are none), and wires the keys.
const EDITOR_SCRIPT: &str = include_str!("../../js/preview-editor.js");

/// How tall a page is at this zoom, as a CSS length.
fn row_height(zoom: Zoom, width: u32, height: u32) -> String {
    match zoom {
        Zoom::Fit => format!(
            "min(var(--reader-page-height),calc(var(--reader-page-width) * {height} / {width}))"
        ),
        Zoom::Width => format!("calc(var(--reader-page-width) * {height} / {width})"),
        Zoom::Custom(percent) => format!("{}px", height * percent as u32 / 100),
    }
}

fn reader_style(zoom: Zoom, width: u32, height: u32) -> String {
    let size = match zoom {
        Zoom::Fit => format!("height:{};width:auto;", row_height(zoom, width, height)),
        Zoom::Width => "height:auto;width:100%;".to_string(),
        Zoom::Custom(percent) => format!(
            "height:{}px;width:{}px;",
            height * percent as u32 / 100,
            width * percent as u32 / 100
        ),
    };
    format!("{size}aspect-ratio:{width}/{height};")
}

/// Which pages share a picture: a double page split in two, or a page zoomed
/// into pieces, is several reader pages from one source page. Every other page
/// is a group of its own. As `(first, one past the last)`.
fn groups(pages: &[std::rc::Rc<crate::state::PreviewPage>]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        match out.last_mut() {
            Some(last) if page.image.is_some() && pages[last.0].image == page.image => {
                last.1 = i + 1;
            }
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// What a page is of its source page, when the source page makes several
/// ("left half of source page 12"): which one of them this is.
fn part_note(page: &crate::state::PreviewPage, k: usize, of: usize) -> Option<String> {
    use converter::plan::Part;
    let image = page.image?;
    if of < 2 {
        return None;
    }
    let what = match page.source.as_ref().map(|p| p.source) {
        Some(Source::Image {
            part: Part::Half(0),
            ..
        }) => "left half".to_string(),
        Some(Source::Image {
            part: Part::Half(_),
            ..
        }) => "right half".to_string(),
        _ => format!("part {} of {of}", k + 1),
    };
    Some(format!("{what} of source page {}", image + 1))
}

/// One page of the chapter in the scrolling list: the page as the reader will
/// show it. The original sits beside it, in the group's source cell.
#[component]
fn EditorRow(
    index: usize,
    selected: bool,
    result: Option<Shot>,
    /// "Page 13", and when the source page makes several, which part this is.
    label: String,
    row_style: String,
    frame_style: String,
    seam: Option<Seam>,
    /// The page is being drawn again: it is darkened, with the cat on it.
    busy: bool,
) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        article {
            class: if selected { "editor-row selected" } else { "editor-row" },
            "data-row": "{index}",
            style: "{row_style}",
            onclick: move |_| {
                if !selected {
                    let mut preview = state.preview;
                    let mut p = preview.write();
                    p.selected = index;
                    p.draft_crop = None;
                }
            },
            div { class: "editor-row-label", "{label}" }
            div { class: "editor-row-panes",
                div { class: "row-pane result-pane",
                    div { class: "editor-reader-frame", style: "{frame_style}",
                        if let Some(shot) = result {
                            img { src: "{shot.url}", alt: "Page {index + 1} exactly as exported to the reader", draggable: "false" }
                        } else {
                            div { class: "editor-image-loading" }
                        }
                        if busy {
                            div { class: "render-veil", CirclingCat { size: 96 } }
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
        div { class: "source-cell source-pane", style: "grid-row: 1 / span {pages.len()};",
            div { class: "source-sticky",
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
                        SourcePicture { source, index: first, marks }
                    }
                } else {
                    div { class: "editor-image-loading source-loading", "Preparing source…" }
                }
            }
        }
    }
}

/// The picture of a page that is not being cropped, with the parts that
/// reader pages show marked on it.
#[component]
fn SourcePicture(source: Shot, index: usize, marks: Vec<(usize, [f32; 4], bool)>) -> Element {
    rsx! {
        div { class: "editor-source-scroll",
            div {
                class: "editor-source-art",
                style: "aspect-ratio: {source.width} / {source.height}; --source-aspect: {source.width as f64 / source.height.max(1) as f64};",
                img { src: "{source.url}", alt: "Source artwork of page {index + 1}", draggable: "false" }
                SourceMarks { marks }
            }
        }
    }
}

/// The rectangles of a picture that reader pages show: the page in view is
/// lit, the others outlined and numbered.
#[component]
pub(super) fn SourceMarks(marks: Vec<(usize, [f32; 4], bool)>) -> Element {
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
        let _ = document::eval("document.querySelector('.editor-scroll')?.scrollTo(0,0)").await;
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
            let js = format!("document.querySelector('[data-editor-page=\"{selected}\"]')?.scrollIntoView({{block:'nearest'}})");
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
    let selected = match state.preview.read().view.zoom {
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
                state.preview.write().view.zoom = match pick {
                    ZoomPick::Fit => Zoom::Fit,
                    ZoomPick::Width => Zoom::Width,
                    ZoomPick::Percent(percent) => Zoom::Custom(percent),
                };
            },
        }
    }
}

#[component]
pub fn PreviewTab() -> Element {
    let mut state = use_context::<AppState>();
    let mut draft = state.crop_draft;
    let mut identity = use_signal(String::new);
    use_effect(move || prepare_preview(state));
    use_future(move || async move {
        let mut eval = document::eval(EDITOR_SCRIPT);
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
    let device = state.settings.read().device_profile();
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
    let seams = seams(&p.pages, device.height);
    rsx! {
        div { id: "page-editor", class: "{classes}", tabindex: "0",
            div { class: "editor-toolbar",
                label { class: "editor-page-jump",
                    span { "Page" }
                    input {
                        r#type: "number", min: "1", max: "{total}", step: "1",
                        value: "{number}", disabled: total == 0, "aria-label": "Go to page",
                        onchange: move |e| {
                            if let Ok(n) = e.value().trim().parse::<usize>() {
                                if (1..=total).contains(&n) { select_preview_page(state, n - 1); }
                            }
                        },
                    }
                    span { class: "muted", "/ {total}" }
                }
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
                        button {
                            key: "{i}",
                            class: if i == selected { "editor-thumb selected" } else { "editor-thumb" },
                            "data-editor-page": "{i}",
                            "aria-label": "Open page {i + 1}",
                            "aria-current": if i == selected { "page" } else { "false" },
                            onclick: move |_| select_preview_page(state, i),
                            div { class: "editor-thumb-art",
                                if let Some(shot) = page.thumbnail.as_ref().or(page.shot.as_ref()) {
                                    img { src: "{shot.url}", alt: "Reader page {i + 1}" }
                                } else {
                                    Icon { name: "file-image", size: 23 }
                                }
                            }
                            span { "{i + 1}" }
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
                        for (start, end) in groups(&p.pages) {
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
                                    EditorRow {
                                        key: "{chapter}/{i}",
                                        index: i,
                                        selected: i == selected,
                                        result: p.pages[i].shot.clone(),
                                        label: match part_note(&p.pages[i], i - start, end - start) {
                                            Some(note) => format!("Page {} · {note}", i + 1),
                                            None => format!("Page {}", i + 1),
                                        },
                                        row_style: row_style.clone(),
                                        frame_style: frame_style.clone(),
                                        seam: seams.get(i).cloned().flatten(),
                                        // Darkened only while this page is being drawn again: a page drawn the
                                        // way it should look is left alone, whatever else is loading.
                                        busy: (i + 1 >= p.window.0 && i <= p.window.1 + 1)
                                            && (p.pages[i].shot.is_none() || (p.busy && p.pages[i].rendered_for != p.signature)),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if p.overview {
                div { class: "editor-overview-backdrop",
                    section { class: "editor-overview", role: "dialog", "aria-modal": "true", "aria-label": "Chapter page overview",
                        header {
                            strong { "Chapter overview · {total} reader pages" }
                            button { id: "editor-close-overview", class: "btn small", onclick: move |_| { state.preview.write().overview = false; }, Icon { name: "x", size: 16 } }
                        }
                        div { class: "editor-overview-grid",
                            for (i, page) in p.pages.iter().enumerate() {
                                button {
                                    key: "{i}",
                                    class: if i == selected { "selected" } else { "" },
                                    onclick: move |_| select_preview_page(state, i),
                                    div { class: "editor-overview-art",
                                        if let Some(shot) = page.thumbnail.as_ref().or(page.shot.as_ref()) {
                                            img { src: "{shot.url}", alt: "Reader page {i + 1}" }
                                        } else {
                                            Icon { name: "file-image", size: 24 }
                                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        image: Option<usize>,
        part: converter::plan::Part,
    ) -> std::rc::Rc<crate::state::PreviewPage> {
        std::rc::Rc::new(crate::state::PreviewPage {
            image,
            source: image.map(|index| converter::PlannedPage {
                source: Source::Image { index, part },
                unit_top: 0,
                unit_height: 0,
                landscape: false,
            }),
            ..Default::default()
        })
    }

    #[test]
    fn the_pages_of_one_source_page_share_a_group() {
        use converter::plan::Part;
        let pages = vec![
            page(Some(0), Part::Whole),
            page(Some(1), Part::Half(1)),
            page(Some(1), Part::Half(0)),
            page(Some(2), Part::Whole),
            page(None, Part::Whole),
            page(None, Part::Whole),
        ];
        assert_eq!(groups(&pages), vec![(0, 1), (1, 3), (3, 4), (4, 5), (5, 6)]);
        assert_eq!(part_note(&pages[0], 0, 1), None);
        assert_eq!(
            part_note(&pages[1], 0, 2).as_deref(),
            Some("right half of source page 2")
        );
        assert_eq!(
            part_note(&pages[2], 1, 2).as_deref(),
            Some("left half of source page 2")
        );
        let tile = page(
            Some(4),
            Part::Tile {
                of: converter::plan::Piece::Whole,
                col: 1,
                row: 0,
            },
        );
        assert_eq!(
            part_note(&tile, 1, 4).as_deref(),
            Some("part 2 of 4 of source page 5")
        );
    }

    #[test]
    fn a_page_is_sized_by_its_zoom() {
        assert_eq!(
            row_height(Zoom::Custom(50), 480, 800),
            "400px",
            "half of the screen's own height"
        );
        assert!(row_height(Zoom::Fit, 480, 800).starts_with("min("));
        assert_eq!(
            row_height(Zoom::Width, 480, 800),
            "calc(var(--reader-page-width) * 800 / 480)"
        );
    }

    #[test]
    fn the_reader_style_keeps_the_page_shape() {
        let fit = reader_style(Zoom::Fit, 480, 800);
        assert!(fit.contains("width:auto;") && fit.ends_with("aspect-ratio:480/800;"));
        assert_eq!(
            reader_style(Zoom::Custom(100), 480, 800),
            "height:800px;width:480px;aspect-ratio:480/800;"
        );
        assert!(reader_style(Zoom::Width, 480, 800).starts_with("height:auto;width:100%;"));
    }
}
