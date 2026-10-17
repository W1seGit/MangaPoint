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
        let on_disk = p
            .chapter_key
            .as_ref()
            .map_or(0, |k| library_files().page_files(&ws.series_id, k).len());
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

/// Read and decode a chapter's saved pages, in order.
pub(super) async fn decode_chapter(
    cache_key: String,
    files: Vec<std::path::PathBuf>,
) -> Result<(String, Arc<Vec<converter::GrayImage>>), String> {
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
    let mut digest = Sha256::new();
    for bytes in &raw {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    let cache_key = format!("{cache_key}/{:x}", digest.finalize());
    if let Some((k, grays)) = DECODED.lock().ok().and_then(|g| g.clone()) {
        if k == cache_key {
            return Ok((cache_key, grays));
        }
    }
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
    use std::hash::{Hash, Hasher};
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
    let Some(series) = state.library.peek().find(&ws.series_id).cloned() else {
        return 0;
    };
    let device = state.settings.peek().device_profile();
    let opts = pipeline::process_options_for(&device, &series.settings);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{}|{key}|{opts:?}|{:?}|{draft:?}", ws.series_id, series.crops).hash(&mut hasher);
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
        let files = library_files().page_files(&ws.series_id, &key);
        if files.is_empty() {
            return fail("This chapter has no saved pages. Download it again.".into());
        }
        let image_count = files.len();
        let (cache_key, grays) =
            match decode_chapter(format!("{}/{key}/{image_count}", ws.series_id), files).await {
                Ok(g) => g,
                Err(message) => return fail(message),
            };
        if !current(state) {
            return;
        }

        // The same planning and conversion the export runs.
        let opts = pipeline::process_options_for(&device, &series.settings);
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
            let (prepare_opts, grays) = (opts.clone(), grays.clone());
            match tokio::task::spawn_blocking(move || {
                pipeline::prepare_chapter(&grays, &prepare_opts)
            })
            .await
            {
                Ok(c) => {
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
