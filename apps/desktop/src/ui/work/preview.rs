//! Previewing: decoding a chapter, and rendering the pages the editor shows.

use super::*;

pub(super) fn data_url(image: &PreviewImage) -> String {
    format!(
        "data:{};base64,{}",
        image.mime,
        STANDARD.encode(&image.bytes)
    )
}

pub(super) fn shot(image: &PreviewImage) -> Shot {
    Shot {
        url: data_url(image),
        width: image.width,
        height: image.height,
    }
}

/// Make sure the preview points at a downloaded chapter, and that it is drawn.
pub fn prepare_preview(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let keys: Vec<String> = state
        .library
        .peek()
        .find(&ws.series_id)
        .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
        .unwrap_or_default();
    let mut preview = state.preview;
    let up_to_date = {
        let mut p = preview.write();
        let valid = p.chapter_key.as_ref().is_some_and(|k| keys.contains(k));
        if !valid {
            p.chapter_key = keys.first().cloned();
            p.pages.clear();
            p.fresh = 0;
        }
        // What the library recorded, not a look at the folder: this runs on the
        // UI thread, and a missing folder is reported when the preview is made.
        let on_disk = p.chapter_key.as_ref().map_or(0, |k| {
            state
                .library
                .peek()
                .find(&ws.series_id)
                .and_then(|s| s.chapters.iter().find(|c| &c.key == k))
                .map_or(0, |c| c.pages)
        });
        valid && on_disk > 0 && !p.pages.is_empty() && p.fresh == p.pages.len() && !p.busy
    };
    if !up_to_date {
        schedule_preview(state, 0);
    }
}

/// The decoded images of the chapter last previewed, so tuning a slider does
/// not read and decode every page again.
type DecodedChapter = (String, Arc<Vec<converter::GrayImage>>);
pub(super) static DECODED: Mutex<Option<DecodedChapter>> = Mutex::new(None);

/// The preview the background thread has finished for a chapter and options,
/// waiting for the screen to take it.
pub(super) static PREPARED_PREVIEW: Mutex<
    Option<(String, converter::ProcessOptions, pipeline::ChapterPreview)>,
> = Mutex::new(None);

/// Counts preview requests. A worker whose number is no longer the newest has
/// been overtaken by a later change and drops its result.
pub(super) static PREVIEW_REQUEST: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

pub(super) fn next_preview_request() -> u64 {
    PREVIEW_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub(super) fn shape_options(opts: &converter::ProcessOptions) -> converter::ProcessOptions {
    let mut key = opts.clone();
    key.contrast = 50;
    key.auto_levels = false;
    key.dither = converter::Dither::Threshold;
    key.depth = converter::Depth::One;
    // How far a page is zoomed, and the arrows on it, change the plan but not
    // the prepared pictures, so dragging the zoom does not prepare them again.
    key.zoom = converter::process::PageZoom::Fit;
    key.zoom_mode = converter::process::ZoomMode::Tile;
    key.guide_arrows = false;
    key
}

/// Most pages drawn full size at once while scrolling.
pub(super) const MAX_FULL_PAGES: usize = 10;

/// The pages to draw full size: the selected one, then those on or near the
/// screen, nearest first.
pub fn full_size_pages(total: usize, selected: usize, window: (usize, usize)) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let selected = selected.min(total - 1);
    let mut pages: Vec<usize> = (window.0.min(total - 1)..=window.1.min(total - 1)).collect();
    pages.sort_by_key(|&i| i.abs_diff(selected));
    pages.truncate(MAX_FULL_PAGES);
    if !pages.contains(&selected) {
        pages.insert(0, selected);
    }
    pages
}

pub fn preview_render_order(
    total: usize,
    selected: usize,
    thumb_start: usize,
    overview: bool,
    window: (usize, usize),
) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let selected = selected.min(total - 1);
    let mut order = vec![selected];
    let range = if overview {
        0..total
    } else {
        thumb_start.min(total)..(thumb_start + 8).min(total)
    };
    for i in range {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    for i in selected.saturating_sub(1)..(selected + 2).min(total) {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    for i in full_size_pages(total, selected, window) {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    order
}

pub fn select_preview_page(state: AppState, selected: usize) {
    let mut preview = state.preview;
    let mut p = preview.write();
    if p.pages.is_empty() {
        return;
    }
    let next = selected.min(p.pages.len() - 1);
    if next != p.selected {
        p.draft_crop = None;
    }
    p.selected = next;
    p.overview = false;
    drop(p);
    schedule_preview(state, 0);
    spawn(async move {
        let js = format!(
            "document.querySelector('[data-editor-page=\"{next}\"]')?.scrollIntoView({{block:'nearest'}});\
             document.querySelector('.editor-row[data-row=\"{next}\"]')?.scrollIntoView({{block:'start'}})"
        );
        let _ = document::eval(&js).await;
    });
}

/// What identifies one saved page without reading it: where it is, how big,
/// and when it was last written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp {
    pub path: std::path::PathBuf,
    pub len: u64,
    pub modified_nanos: u128,
}

pub(super) fn stamp_files(files: &[std::path::PathBuf]) -> Result<Vec<FileStamp>, String> {
    files
        .iter()
        .map(|path| {
            let meta = std::fs::metadata(path)
                .map_err(|e| format!("Could not read a saved page: {e}"))?;
            let modified_nanos = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            Ok(FileStamp {
                path: path.clone(),
                len: meta.len(),
                modified_nanos,
            })
        })
        .collect()
}

/// One short name for a set of saved pages, from their stamps: it changes when
/// a page is added, replaced or rewritten, without reading any of them.
pub(super) fn fingerprint_of(stamps: &[FileStamp]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for stamp in stamps {
        stamp.path.hash(&mut hasher);
        stamp.len.hash(&mut hasher);
        stamp.modified_nanos.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// The name a chapter's saved pages go by in the preview caches, from their
/// stamps; the files are not read.
pub(super) async fn stamped_key(
    cache_key: String,
    files: Vec<std::path::PathBuf>,
) -> Result<String, String> {
    let stamps = tokio::task::spawn_blocking(move || stamp_files(&files))
        .await
        .map_err(|e| e.to_string())??;
    Ok(format!("{cache_key}/{}", fingerprint_of(&stamps)))
}

/// Read and decode a chapter's saved pages, in order. The decoded chapter is
/// remembered under a name made from the pages' stamps, so asking again for the
/// same pages only looks at the files' sizes and dates, off the UI thread.
pub(super) async fn decode_chapter(
    cache_key: String,
    files: Vec<std::path::PathBuf>,
) -> Result<(String, Arc<Vec<converter::GrayImage>>), String> {
    let stamps = tokio::task::spawn_blocking(move || stamp_files(&files).map(|s| (s, files)))
        .await
        .map_err(|e| e.to_string())??;
    let (stamps, files) = stamps;
    let cache_key = format!("{cache_key}/{}", fingerprint_of(&stamps));
    if let Some((k, grays)) = DECODED.lock().ok().and_then(|g| g.clone()) {
        if k == cache_key {
            return Ok((cache_key, grays));
        }
    }
    let raw = tokio::task::spawn_blocking(move || {
        files
            .into_iter()
            .map(|path| {
                std::fs::read(path).map_err(|e| format!("Could not read a saved page: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|e| e.to_string())??;
    let decoded: Vec<Result<converter::GrayImage, String>> = stream::iter(raw)
        .map(|bytes| async move {
            tokio::task::spawn_blocking(move || {
                pipeline::decode_page(&bytes).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        })
        .buffered(PREVIEW_THREADS)
        .collect()
        .await;
    let grays = Arc::new(decoded.into_iter().collect::<Result<Vec<_>, _>>()?);
    if let Ok(mut g) = DECODED.lock() {
        *g = Some((cache_key.clone(), grays.clone()));
    }
    Ok((cache_key, grays))
}

/// Plan the chapter with the series' settings, then draw each page a few at a
/// time in the background. The plan appears first, so the screen takes its
/// shape at once. Settings changes call this with a short delay, so dragging a
/// slider draws once it settles.
/// One number for everything that decides how the pages look. `0` if it
/// cannot be worked out.
fn preview_signature(state: AppState) -> u64 {
    let Some(ws) = state.workspace.peek().clone() else {
        return 0;
    };
    let (key, draft) = {
        let p = state.preview.peek();
        (p.chapter_key.clone(), p.draft_crop.clone())
    };
    let Some(key) = key else {
        return 0;
    };
    let library = state.library.peek();
    let Some(series) = library.find(&ws.series_id) else {
        return 0;
    };
    let device = state.settings.peek().device_profile();
    let opts = pipeline::process_options_for(&device, &series.settings);
    // Only the crops that apply to this chapter matter to how it looks.
    let crops = series.crops.chapter_configuration(&key);
    signature_of(&ws.series_id, &key, &opts, &crops, &draft)
}

/// Writes what is formatted straight into a hasher, so the options are hashed
/// without first being made into a string.
struct HashWriter<'a, H: std::hash::Hasher>(&'a mut H);

impl<H: std::hash::Hasher> std::fmt::Write for HashWriter<'_, H> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// One number for the chapter, the conversion options, the crops that apply to
/// it and the crop being dragged. Never 0.
pub(super) fn signature_of(
    series_id: &str,
    key: &str,
    opts: &converter::ProcessOptions,
    crops: &[u8],
    draft: &Option<(String, library::CropRect)>,
) -> u64 {
    use std::fmt::Write;
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    series_id.hash(&mut hasher);
    key.hash(&mut hasher);
    let _ = write!(HashWriter(&mut hasher), "{opts:?}");
    crops.hash(&mut hasher);
    let _ = write!(HashWriter(&mut hasher), "{draft:?}");
    hasher.finish().max(1)
}

pub fn schedule_preview(state: AppState, delay_ms: u64) {
    let mut preview = state.preview;
    let signature = preview_signature(state);
    let generation = {
        let mut p = preview.write();
        p.signature = signature;
        p.generation = next_preview_request();
        p.busy = true;
        p.error = None;
        p.fresh = 0;
        p.generation
    };
    spawn_forever(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        let current = |state: AppState| state.preview.peek().generation == generation;
        if !current(state) {
            return;
        }
        let fail = |message: String| {
            let mut p = state.preview;
            if p.peek().generation == generation {
                let mut w = p.write();
                w.busy = false;
                w.error = Some(message);
                w.pages.clear();
            }
        };
        let Some(ws) = state.workspace.peek().clone() else {
            return;
        };
        let key = state.preview.peek().chapter_key.clone();
        let Some(key) = key else {
            let mut p = state.preview;
            if current(state) {
                p.write().busy = false;
            }
            return;
        };
        let Some(series) = state.library.peek().find(&ws.series_id).cloned() else {
            return fail("This series is not in your library.".into());
        };
        let device = state.settings.peek().device_profile();
        let files = {
            let (id, k) = (ws.series_id.clone(), key.clone());
            tokio::task::spawn_blocking(move || library_files().page_files(&id, &k))
                .await
                .unwrap_or_default()
        };
        if !current(state) {
            return;
        }
        if files.is_empty() {
            return fail("This chapter has no saved pages. Download it again.".into());
        }
        let image_count = files.len();
        // The same planning and conversion the export runs.
        let opts = pipeline::process_options_for(&device, &series.settings);
        // With colour kept, the pages are decoded in colour, only for the
        // chapter in view, and only when it has to be prepared (below); the
        // gray decode is let go, so one chapter is held at a time.
        let color = opts.color;
        let color_files = color.then(|| files.clone());
        let (cache_key, grays) = if color {
            if let Ok(mut held) = DECODED.lock() {
                *held = None;
            }
            match stamped_key(format!("{}/{key}/{image_count}/color", ws.series_id), files).await {
                Ok(k) => (k, None),
                Err(message) => return fail(message),
            }
        } else {
            match decode_chapter(format!("{}/{key}/{image_count}", ws.series_id), files).await {
                Ok((k, g)) => (k, Some(g)),
                Err(message) => return fail(message),
            }
        };
        if !current(state) {
            return;
        }
        let shape_key = shape_options(&opts);
        let cached = PREPARED_PREVIEW.lock().ok().and_then(|g| {
            g.as_ref()
                .filter(|(k, o, _)| k == &cache_key && o == &shape_key)
                .map(|(_, _, p)| p.clone())
        });
        let chapter = if let Some(cached) = cached {
            // The prepared pictures are shared; only the plan is made again, as
            // the zoom may have changed since.
            let plan_opts = opts.clone();
            match tokio::task::spawn_blocking(move || pipeline::ChapterPreview {
                plan: converter::plan::plan(&cached.prepared, &plan_opts),
                prepared: cached.prepared,
            })
            .await
            {
                Ok(c) => c,
                Err(e) => return fail(e.to_string()),
            }
        } else {
            let prepare_opts = opts.clone();
            match tokio::task::spawn_blocking(move || match (grays, color_files) {
                (Some(grays), _) => Ok(pipeline::prepare_chapter(&grays, &prepare_opts)),
                (None, Some(files)) => {
                    pipeline::prepare_chapter_color(&files, &prepare_opts, PREVIEW_THREADS)
                        .map_err(|e| e.to_string())
                }
                (None, None) => Err("The chapter could not be read.".to_string()),
            })
            .await
            {
                Ok(Err(message)) => return fail(message),
                Ok(Ok(c)) => {
                    if let Ok(mut cache) = PREPARED_PREVIEW.lock() {
                        *cache = Some((cache_key, shape_key, c.clone()));
                    }
                    c
                }
                Err(e) => return fail(e.to_string()),
            }
        };
        if !current(state) {
            return;
        }
        let mut scroll_to = None;
        {
            let mut p = state.preview;
            let mut w = p.write();
            if w.generation != generation {
                return;
            }
            // The source page in view, to stay on when the pages are planned again
            // (a zoom or a spread setting changes how many there are).
            let anchor = w.pages.get(w.selected).and_then(|p| p.image);
            let old = std::mem::take(&mut w.pages);
            w.strip = chapter.plan.strip;
            w.images = image_count;
            let near = (
                w.window.0.saturating_sub(KEEP_DRAWN_AROUND),
                w.window.1 + KEEP_DRAWN_AROUND,
            );
            w.pages = chapter
                .plan
                .pages
                .iter()
                .enumerate()
                .map(|(position, page)| {
                    // Keep the previous drawing of this page until it is
                    // redrawn, but only near the screen: the full-size
                    // drawings of a long chapter would fill the memory.
                    let (shot, thumbnail, original, region, piece, kept, rendered_for) = old
                        .iter()
                        .find(|o| o.crop_key == pipeline::crop_page_key(page))
                        .map_or((None, None, None, None, None, None, 0), |o| {
                            let close = (near.0..=near.1).contains(&position);
                            (
                                o.shot.clone().filter(|_| close),
                                o.thumbnail.clone(),
                                o.original.clone().filter(|_| close),
                                o.region,
                                o.piece.clone().filter(|_| close),
                                o.kept,
                                o.rendered_for,
                            )
                        });
                    let image = match page.source {
                        converter::Source::Image { index, .. } => Some(index),
                        converter::Source::Strip { .. } => None,
                    };
                    Rc::new(PreviewPage {
                        shot,
                        thumbnail,
                        original,
                        region,
                        piece,
                        kept,
                        image,
                        crop_key: pipeline::crop_page_key(page),
                        source: Some(page.clone()),
                        generation: 0,
                        rendered_for,
                    })
                })
                .collect();
            w.selected = w.selected.min(w.pages.len().saturating_sub(1));
            if let Some(image) = anchor {
                if w.pages.get(w.selected).and_then(|p| p.image) != Some(image) {
                    if let Some(at) = w.pages.iter().position(|p| p.image == Some(image)) {
                        w.selected = at;
                        w.draft_crop = None;
                        scroll_to = Some(at);
                    }
                }
            }
        }
        if let Some(at) = scroll_to {
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(60)).await;
                let js = format!(
                    "document.querySelector('.editor-row[data-row=\"{at}\"]')?.scrollIntoView({{block:'start'}})"
                );
                let _ = document::eval(&js).await;
            });
        }

        let prepared = chapter.prepared.clone();
        let (selected, thumb_start, overview, draft, window, want_source) = {
            let p = state.preview.peek();
            (
                p.selected,
                p.thumb_start,
                p.overview,
                p.draft_crop.clone(),
                p.window,
                p.view.show_original,
            )
        };
        let full: std::collections::HashSet<usize> =
            full_size_pages(chapter.plan.pages.len(), selected, window)
                .into_iter()
                .collect();
        // Only pages that are not already drawn the way they should look: moving
        // along the chapter must not draw the pages that are there again.
        let drawn_now = state.preview.peek().pages.clone();
        let needs = |i: usize| -> bool {
            let Some(page) = drawn_now.get(i) else {
                return true;
            };
            i == selected
                || signature == 0
                || page.rendered_for != signature
                || page.thumbnail.is_none()
                || (full.contains(&i)
                    && (page.shot.is_none() || (want_source && page.original.is_none())))
        };
        let jobs: Vec<_> = preview_render_order(
            chapter.plan.pages.len(),
            selected,
            thumb_start,
            overview,
            window,
        )
        .into_iter()
        .filter(|i| needs(*i))
        .map(|i| (i, chapter.plan.pages[i].clone()))
        .collect();
        let crops = series.crops.clone();
        let chapter_key = key.clone();
        let mut drawn = stream::iter(jobs)
            .map(|(index, page)| {
                let (opts, prepared, crops, chapter_key, draft) = (
                    opts.clone(),
                    prepared.clone(),
                    crops.clone(),
                    chapter_key.clone(),
                    draft.clone(),
                );
                let (is_full, is_selected) = (full.contains(&index), index == selected);
                async move {
                    let result = tokio::task::spawn_blocking(move || {
                        let key = pipeline::crop_page_key(&page);
                        let crop = draft
                            .as_ref()
                            .filter(|(k, _)| k == &key)
                            .map(|(_, crop)| *crop)
                            .or_else(|| crops.effective(&chapter_key, &key));
                        if opts.color {
                            // Colour: the converted page and both source panes
                            // are drawn in colour. The shading samples are for
                            // gray pages' dithering and do not apply.
                            let drawn = pipeline::draw_color_page(
                                &prepared,
                                &page,
                                &opts,
                                crop.unwrap_or_default().as_array(),
                                is_full,
                                is_full && (want_source || is_selected),
                                is_selected
                                    && matches!(
                                        page.source,
                                        converter::Source::Image { part, .. }
                                            if part != converter::plan::Part::Whole
                                    ),
                            );
                            return (drawn.converted, drawn.thumbnail, drawn.original, drawn.piece, Vec::new());
                        }
                        let levels = converter::plan::render_page_cropped(
                            &prepared,
                            &page,
                            &opts,
                            crop.unwrap_or_default().as_array(),
                        );
                        let thumbnail = pipeline::levels_to_thumbnail(&levels);
                        let converted =
                            is_full.then(|| pipeline::levels_to_previews(&[levels]).remove(0));
                        let original = (is_full && (want_source || is_selected))
                            .then(|| pipeline::render_source_whole(&prepared, &page, &opts));
                        // The crop is measured on the part of the picture the
                        // page shows, so that part is drawn for the page in hand.
                        let piece = (is_selected
                            && matches!(
                                page.source,
                                converter::Source::Image { part, .. }
                                    if part != converter::plan::Part::Whole
                            ))
                        .then(|| pipeline::render_source_preview(&prepared, &page, &opts));
                        let samples = if is_selected {
                            let source = converter::plan::page_source(&prepared, &page, &opts);
                            [
                                converter::Dither::FloydSteinberg,
                                converter::Dither::Atkinson,
                                converter::Dither::Ordered,
                                converter::Dither::Threshold,
                            ]
                            .into_iter()
                            .map(|dither| {
                                let mut sample_opts = opts.clone();
                                sample_opts.width = 96;
                                sample_opts.height = 160;
                                sample_opts.layout = converter::process::Layout::Manga;
                                sample_opts.crop = converter::process::CropMode::Full;
                                sample_opts.margins = converter::process::Margins::default();
                                sample_opts.spread = converter::process::SpreadMode::Fit;
                                sample_opts.dither = dither;
                                let levels =
                                    converter::process::process_page(&source, &sample_opts);
                                pipeline::levels_to_previews(&levels)
                                    .first()
                                    .map(|image| shot(image).url)
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        (converted, thumbnail, original, piece, samples)
                    })
                    .await;
                    (index, result)
                }
            })
            .buffer_unordered(PREVIEW_THREADS);
        let mut first_error = None;
        while let Some((index, result)) = drawn.next().await {
            if !current(state) {
                return;
            }
            match result {
                Ok((converted, thumbnail, original, piece, samples)) => {
                    let mut p = state.preview;
                    let mut w = p.write();
                    if let Some(slot) = w.pages.get_mut(index) {
                        let mut next = (**slot).clone();
                        if let Some(converted) = converted {
                            next.shot = Some(shot(&converted));
                        }
                        next.thumbnail = Some(shot(&thumbnail));
                        if let Some((original, region)) = original {
                            next.original = Some(shot(&original));
                            next.region = region;
                        }
                        next.piece = piece.map(|piece| shot(&piece));
                        next.generation = generation;
                        next.rendered_for = signature;
                        *slot = Rc::new(next);
                    }
                    if index == selected {
                        w.shading_samples = samples;
                    }
                    w.fresh += 1;
                }
                Err(e) => {
                    first_error.get_or_insert(format!("Page {}: {e}", index + 1));
                }
            }
        }
        let mut p = state.preview;
        if p.peek().generation == generation {
            let mut w = p.write();
            w.busy = false;
            w.error = first_error;
        }
    });
}

/// Pages drawn at the same time while previewing.
pub(super) const PREVIEW_THREADS: usize = 4;
/// Pages either side of the screen whose full-size drawings are kept.
pub(super) const KEEP_DRAWN_AROUND: usize = 12;

pub fn grouping_of(opts: &ExportOpts) -> Grouping {
    match opts.grouping {
        GroupingChoice::Single => Grouping::Single,
        GroupingChoice::PerChapter => Grouping::PerChapter,
        GroupingChoice::EveryN => {
            Grouping::EveryN(opts.every_n.trim().parse::<usize>().unwrap_or(10).max(1))
        }
    }
}

#[cfg(test)]
mod page_editor_tests {
    use super::*;

    #[test]
    fn selected_page_precedes_visible_thumbnails_without_rendering_the_whole_chapter() {
        let order = preview_render_order(148, 100, 90, false, (99, 101));
        assert_eq!(order[0], 100);
        for i in 90..98 {
            assert!(order.contains(&i));
        }
        assert!(order.contains(&99));
        assert!(order.contains(&101));
        assert!(order.len() <= 11, "{}", order.len());
        let mut unique = order.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), order.len());
        assert_eq!(
            preview_render_order(0, 10, 0, false, (0, 0)),
            Vec::<usize>::new()
        );
        assert_eq!(preview_render_order(3, 99, 0, true, (0, 0)), [2, 0, 1]);
    }

    #[test]
    fn the_pages_on_screen_are_drawn_full_size_nearest_first_and_not_too_many() {
        assert_eq!(full_size_pages(0, 0, (0, 5)), Vec::<usize>::new());
        // The selected page is always drawn, even off screen.
        assert_eq!(full_size_pages(50, 40, (3, 4))[0], 40);
        // The window is drawn nearest the selected page first.
        assert_eq!(full_size_pages(50, 4, (3, 5)), [4, 3, 5]);
        // A tall window is capped.
        assert_eq!(full_size_pages(50, 10, (0, 40)).len(), MAX_FULL_PAGES);
        // A window past the end is clamped.
        assert_eq!(full_size_pages(5, 4, (3, 99)), [4, 3]);
    }

    #[test]
    fn appearance_changes_reuse_the_prepared_chapter_but_layout_changes_do_not() {
        let opts = converter::ProcessOptions::manga_default(480, 800, converter::Depth::Two);
        let mut changed = opts.clone();
        changed.contrast = 90;
        changed.auto_levels = !opts.auto_levels;
        changed.dither = converter::Dither::Atkinson;
        assert_eq!(shape_options(&opts), shape_options(&changed));
        changed.layout = converter::process::Layout::Webtoon;
        assert_ne!(shape_options(&opts), shape_options(&changed));
    }

    #[test]
    fn colour_and_gray_previews_never_share_a_prepared_chapter_or_a_signature() {
        let gray = converter::ProcessOptions::manga_default(480, 800, converter::Depth::Two);
        let mut colour = gray.clone();
        colour.color = true;
        // The prepared chapter is kept under these options.
        assert_ne!(shape_options(&gray), shape_options(&colour));
        // And the drawn pages are redrawn when the choice changes.
        assert_ne!(
            signature_of("s", "c1", &gray, &[], &None),
            signature_of("s", "c1", &colour, &[], &None)
        );
        // Tone settings still do not prepare a colour chapter again.
        let mut toned = colour.clone();
        toned.contrast = 90;
        assert_eq!(shape_options(&colour), shape_options(&toned));
    }

    #[tokio::test]
    async fn a_chapters_cache_name_follows_its_files_without_reading_them() {
        let dir = std::env::temp_dir().join(format!("mangapoint-stamped-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("00001.img");
        std::fs::write(&path, b"one").unwrap();
        let a = stamped_key("k".into(), vec![path.clone()]).await.unwrap();
        assert_eq!(a, stamped_key("k".into(), vec![path.clone()]).await.unwrap());
        std::fs::write(&path, b"longer").unwrap();
        assert_ne!(a, stamped_key("k".into(), vec![path]).await.unwrap());
        assert!(stamped_key("k".into(), vec![dir.join("missing")]).await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preview_tokens_do_not_reset_between_workspaces() {
        let first = next_preview_request();
        let second = next_preview_request();
        assert!(second > first);
    }

    #[tokio::test]
    async fn preview_reloads_replaced_sources_with_the_same_page_count() {
        let root = std::env::temp_dir().join(format!(
            "mangapoint-preview-revision-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("00001.img");
        fn image(value: u8) -> Vec<u8> {
            use image::ImageEncoder;
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(&[value; 16], 4, 4, image::ExtendedColorType::L8)
                .unwrap();
            bytes
        }
        std::fs::write(&path, image(0)).unwrap();
        let (old, first) = decode_chapter("revision-test".into(), vec![path.clone()])
            .await
            .unwrap();
        std::fs::write(&path, image(255)).unwrap();
        // Some file systems stamp two quick writes with the same time; a real
        // replacement happens later, so move the stamp on.
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        let (new, second) = decode_chapter("revision-test".into(), vec![path.clone()])
            .await
            .unwrap();
        assert_ne!(old, new);
        assert_eq!(first[0].pixels, [0; 16]);
        assert_eq!(second[0].pixels, [255; 16]);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> converter::ProcessOptions {
        pipeline::process_options_for(
            &device_profiles::x4(),
            &library::ConversionSettings::default(),
        )
    }

    #[test]
    fn the_signature_changes_with_the_settings_and_crops_and_only_then() {
        let base = opts();
        let sig = |o: &converter::ProcessOptions, crops: &[u8], d: &Option<(String, library::CropRect)>| {
            signature_of("s", "c1", o, crops, d)
        };
        let first = sig(&base, b"x", &None);
        assert_ne!(first, 0);
        assert_eq!(first, sig(&base.clone(), b"x", &None), "same inputs, same number");
        let mut other = base.clone();
        other.contrast = base.contrast.wrapping_add(7);
        assert_ne!(first, sig(&other, b"x", &None), "a setting changed");
        assert_ne!(first, sig(&base, b"y", &None), "a crop changed");
        let draft = Some(("p".to_string(), library::CropRect { left: 0.1, ..Default::default() }));
        assert_ne!(first, sig(&base, b"x", &draft), "a crop is being dragged");
        assert_ne!(first, signature_of("s", "c2", &base, b"x", &None), "another chapter");
    }

    #[test]
    fn a_decoded_chapter_is_found_by_the_files_stamps_without_reading_them() {
        let dir = std::env::temp_dir().join(format!("mp-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Not pictures: if they were read and decoded this would be an error.
        let files: Vec<_> = (0..3)
            .map(|i| {
                let p = dir.join(format!("{i}.img"));
                std::fs::write(&p, vec![i as u8; 100 + i]).unwrap();
                p
            })
            .collect();
        let key = format!("s/c1/3/{}", fingerprint_of(&stamp_files(&files).unwrap()));
        let grays = Arc::new(Vec::new());
        *DECODED.lock().unwrap() = Some((key.clone(), grays.clone()));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (got_key, got) = rt
            .block_on(decode_chapter("s/c1/3".into(), files.clone()))
            .expect("a cache hit needs no decoding");
        assert_eq!(got_key, key);
        assert!(Arc::ptr_eq(&got, &grays));
        // A page that changed size is not the same chapter: it is read, and
        // these are not pictures.
        std::fs::write(&files[1], b"changed on disk").unwrap();
        assert!(rt.block_on(decode_chapter("s/c1/3".into(), files)).is_err());
        *DECODED.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
