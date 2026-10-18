//! Stage 2: preview how a page will look on the reader.
//!
//! This runs the same conversion as the export ([`process_page`]), so the
//! preview shows exactly the pixels that would be written to the file. It
//! shows the ideal four (or two) gray levels; how the real panel renders
//! them is for a hardware test to confirm.

use crate::error::{PipelineError, Result};
use converter::decode::decode_to_gray;
use converter::process::resize;
use converter::{
    crop_bounds, process_page, ChapterPlan, GrayImage, LevelImage, PlannedPage, Prepared,
    ProcessOptions,
};
use image::{
    codecs::{jpeg::JpegEncoder, png::PngEncoder},
    ExtendedColorType, ImageEncoder,
};
use std::sync::Arc;

/// A page ready to show: encoded image bytes plus its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    pub bytes: Vec<u8>,
    /// "image/png" for converted pages (exact pixels), "image/jpeg" for originals.
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Decode a saved page for repeated previewing. Decode once, then call
/// [`render_levels`] each time a setting changes.
pub fn decode_page(bytes: &[u8]) -> Result<GrayImage> {
    decode_to_gray(bytes).map_err(|e| PipelineError::Decode(e.to_string()))
}

/// The device pages a saved page becomes: one normally, two for a double-page
/// spread that is split. This is the very function the export uses.
pub fn render_levels(gray: &GrayImage, opts: &ProcessOptions) -> Vec<LevelImage> {
    process_page(gray, opts)
}

/// Show converted pages as PNGs. Levels map evenly onto gray: black, dark
/// gray, light gray, white for four levels; black and white for two.
pub fn levels_to_previews(levels: &[LevelImage]) -> Vec<PreviewImage> {
    levels.iter().map(level_png).collect()
}

/// A small navigator image derived from the exact rendered levels.
pub fn levels_to_thumbnail(img: &LevelImage) -> PreviewImage {
    let max = img.depth.max_level() as u32;
    let gray = GrayImage {
        width: img.width,
        height: img.height,
        pixels: img
            .levels
            .iter()
            .map(|&l| (l as u32 * 255 / max) as u8)
            .collect(),
    };
    let height = 160;
    let width = (img.width as f64 * height as f64 / img.height as f64)
        .round()
        .max(1.0) as u32;
    let small = resize(&gray, width, height);
    PreviewImage {
        bytes: encode_png(&small.pixels, width, height),
        mime: "image/png",
        width,
        height,
    }
}

fn level_png(img: &LevelImage) -> PreviewImage {
    let max = img.depth.max_level() as u32;
    let pixels: Vec<u8> = img
        .levels
        .iter()
        .map(|&l| (l as u32 * 255 / max) as u8)
        .collect();
    PreviewImage {
        bytes: encode_png(&pixels, img.width, img.height),
        mime: "image/png",
        width: img.width,
        height: img.height,
    }
}

/// Widest the original shown beside the converted page is made: sharp enough
/// for a wide window, small enough to keep a long chapter light.
pub const ORIGINAL_MAX_WIDTH: u32 = 760;

/// Tallest it is made. Browsers refuse images much taller than this, so only
/// an absurdly long strip is shrunk further than the width limit alone would.
pub const ORIGINAL_MAX_HEIGHT: u32 = 16_000;

/// JPEG quality of the original shown beside the converted page.
const ORIGINAL_QUALITY: u8 = 82;

/// The downloaded page, in its own shape and tones, and which part of it the
/// conversion keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalPreview {
    pub image: PreviewImage,
    /// The kept area as fractions of the image: `[left, top, width, height]`,
    /// each 0..=1. Everything outside it is cut by the margin settings.
    pub kept: [f32; 4],
}

/// The downloaded page itself, without conversion, for comparing against the
/// converted result. It keeps its own shape (nothing is letterboxed) and is
/// only shrunk if it is wider than the preview can use. The limit is on the
/// width, not the longest side: limiting that would squash a tall strip into a
/// sliver and make it look blurry.
pub fn render_original(gray: &GrayImage, opts: &ProcessOptions) -> OriginalPreview {
    let scale = (ORIGINAL_MAX_WIDTH as f64 / gray.width as f64)
        .min(ORIGINAL_MAX_HEIGHT as f64 / gray.height as f64);
    // Only a shrunk copy is made; an original shown as it is is encoded from
    // the decoded image itself, not from a copy of it.
    let shrunk;
    let shown = if scale < 1.0 {
        let w = ((gray.width as f64 * scale).round() as u32).max(1);
        let h = ((gray.height as f64 * scale).round() as u32).max(1);
        shrunk = resize(gray, w, h);
        &shrunk
    } else {
        gray
    };
    let (x, y, w, h) = crop_bounds(gray, opts);
    let (gw, gh) = (gray.width as f32, gray.height as f32);
    OriginalPreview {
        image: PreviewImage {
            bytes: encode_jpeg(&shown.pixels, shown.width, shown.height),
            mime: "image/jpeg",
            width: shown.width,
            height: shown.height,
        },
        kept: [x as f32 / gw, y as f32 / gh, w as f32 / gw, h as f32 / gh],
    }
}

fn encode_jpeg(gray: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, ORIGINAL_QUALITY)
        .write_image(gray, width, height, ExtendedColorType::L8)
        .expect("encoding an in-memory gray image cannot fail");
    out
}

/// One saved page, ready to show: the device pages it becomes, and the
/// original beside them.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderedPage {
    pub converted: Vec<PreviewImage>,
    pub original: OriginalPreview,
}

/// Decode a saved page and run the export's conversion on it. Meant for a
/// background thread; a chapter's pages can be rendered independently.
pub fn render_page(bytes: &[u8], opts: &ProcessOptions) -> Result<RenderedPage> {
    Ok(render_gray(&decode_page(bytes)?, opts))
}

/// The same, for a page that is already decoded.
pub fn render_gray(gray: &GrayImage, opts: &ProcessOptions) -> RenderedPage {
    RenderedPage {
        converted: levels_to_previews(&render_levels(gray, opts)),
        original: render_original(gray, opts),
    }
}

/// A chapter prepared for previewing: cropped and (for a webtoon) stitched,
/// with the pages it will become. Share it between threads to draw pages in
/// parallel with [`render_planned`].
#[derive(Clone)]
pub struct ChapterPreview {
    pub prepared: Arc<Prepared>,
    pub plan: ChapterPlan,
}

/// Prepare and plan a whole decoded chapter, the way the export does.
pub fn prepare_chapter(grays: &[GrayImage], opts: &ProcessOptions) -> ChapterPreview {
    let prepared = Arc::new(converter::plan::prepare(grays, opts));
    let plan = converter::plan::plan(&prepared, opts);
    ChapterPreview { prepared, plan }
}

/// [`prepare_chapter`] for a caller that no longer needs the decoded images:
/// each is let go as soon as its cropped or stitched copy exists, so the
/// chapter is not held twice while it is prepared.
pub fn prepare_chapter_owned(grays: Vec<GrayImage>, opts: &ProcessOptions) -> ChapterPreview {
    let prepared = Arc::new(
        converter::plan::prepare_owned(grays, opts, &|| false).expect("nothing cancels it"),
    );
    let plan = converter::plan::plan(&prepared, opts);
    ChapterPreview { prepared, plan }
}

/// Draw one planned page: the exact pixels the export writes for it.
pub fn render_planned(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
) -> PreviewImage {
    level_png(&converter::plan::render_page(prepared, page, opts))
}

/// The downloaded rows a strip page is made from, as a picture to show beside
/// the converted page. `None` for a paged chapter, whose original is the
/// downloaded image itself ([`render_original`]).
pub fn render_strip_original(prepared: &Prepared, page: &PlannedPage) -> Option<PreviewImage> {
    let rows = converter::plan::original_rows(prepared, page)?;
    Some(PreviewImage {
        bytes: encode_jpeg(&rows.pixels, rows.width, rows.height),
        mime: "image/jpeg",
        width: rows.width,
        height: rows.height,
    })
}

/// A page drawn from scratch to try settings on when there is nothing
/// downloaded: panels with borders and white margins, a smooth screentone
/// sky, halftone dots, speed lines, a dark scene and a speech bubble. It
/// exercises what the settings change: trimming, contrast and shading.
pub fn sample_page() -> GrayImage {
    const W: u32 = 900;
    const H: u32 = 1400;
    let mut img = GrayImage::new(W, H, 255).expect("constant size");
    // A panel: black border, then its contents drawn by `paint(x, y) -> gray`.
    let mut panel = |x0: u32, y0: u32, w: u32, h: u32, paint: &dyn Fn(u32, u32, u32, u32) -> u8| {
        img.fill_rect(x0, y0, w, h, 0);
        for y in 0..h - 12 {
            for x in 0..w - 12 {
                img.set(x0 + 6 + x, y0 + 6 + y, paint(x, y, w - 12, h - 12));
            }
        }
    };
    // Top: a sky that fades from dark to light, with a sun of dots.
    panel(70, 70, 760, 380, &|x, y, w, h| {
        let sky = 70.0 + 170.0 * (y as f32 / h as f32);
        let (dx, dy) = (x as f32 - w as f32 * 0.72, y as f32 - h as f32 * 0.34);
        let d = (dx * dx + dy * dy).sqrt();
        // The sun: solid in the middle, a ring of dots around it.
        if d < 62.0 || (d < 90.0 && (x / 6 + y / 6) % 2 == 0) {
            255
        } else {
            sky as u8
        }
    });
    // Middle left: a dark scene crossed by light speed lines.
    panel(70, 480, 360, 400, &|x, y, w, h| {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let angle = (y as f32 - cy).atan2(x as f32 - cx);
        let ray = ((angle * 38.0).sin() > 0.55) as u8;
        let near = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < 54.0;
        if near {
            235
        } else if ray == 1 {
            205
        } else {
            45
        }
    });
    // Middle right: halftone dots that grow toward the bottom.
    panel(470, 480, 360, 400, &|x, y, _w, h| {
        let (cell_x, cell_y) = (x % 12, y % 12);
        let r = 1.0 + 5.2 * (y as f32 / h as f32);
        let d = (((cell_x as f32 - 5.5).powi(2)) + ((cell_y as f32 - 5.5).powi(2))).sqrt();
        if d < r {
            25
        } else {
            245
        }
    });
    // Bottom: a mid-gray wash with a speech bubble full of lines of "text".
    panel(70, 910, 760, 420, &|x, y, w, h| {
        let wash = 150.0 + 60.0 * (x as f32 / w as f32);
        let (dx, dy) = (
            (x as f32 - w as f32 * 0.5) / 270.0,
            (y as f32 - h as f32 * 0.46) / 120.0,
        );
        let inside = dx * dx + dy * dy;
        if inside < 1.0 {
            // Text lines inside the bubble.
            let line = (y as i32 - (h as f32 * 0.46) as i32 + 70).rem_euclid(34);
            let margin = ((x as f32 - w as f32 * 0.5).abs() < 200.0) as u8;
            if line < 9 && margin == 1 && inside < 0.72 && (x / 17) % 5 != 0 {
                20
            } else {
                255
            }
        } else if inside < 1.08 {
            0
        } else {
            wash as u8
        }
    });
    img
}

fn encode_png(gray: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(gray, width, height, ExtendedColorType::L8)
        .expect("encoding an in-memory gray image cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::{Depth, Dither};

    fn opts(depth: Depth) -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, depth);
        o.dither = Dither::Threshold;
        o
    }

    fn page() -> GrayImage {
        GrayImage::from_fn(600, 900, |x, _| (x * 255 / 599) as u8).unwrap()
    }

    #[test]
    fn a_chapter_preview_matches_what_the_export_writes() {
        // Three short webtoon images: stitched, planned, and drawn per page.
        let images: Vec<GrayImage> = (0..3)
            .map(|i| GrayImage::new(480, 900, 60 + i * 50).unwrap())
            .collect();
        let mut o = opts(Depth::Two);
        o.layout = converter::Layout::Webtoon;
        o.crop = converter::CropMode::Full;
        let chapter = prepare_chapter(&images, &o);
        assert!(chapter.plan.strip);
        let drawn: Vec<PreviewImage> = chapter
            .plan
            .pages
            .iter()
            .map(|p| render_planned(&chapter.prepared, p, &o))
            .collect();
        let exported = converter::plan::convert_chapter(&images, &o);
        assert_eq!(drawn.len(), exported.len());
        assert_eq!(
            drawn,
            levels_to_previews(&exported),
            "preview and export draw the same pixels"
        );
    }

    #[test]
    fn preparing_a_chapter_by_value_gives_the_same_preview_as_by_reference() {
        let images: Vec<GrayImage> = (0..4)
            .map(|i| GrayImage::from_fn(500, 900 + i * 50, |x, y| ((x * 3 + y * 5) % 251) as u8).unwrap())
            .collect();
        for layout in [converter::Layout::Webtoon, converter::Layout::Manga] {
            let mut o = opts(Depth::Two);
            o.layout = layout;
            let by_ref = prepare_chapter(&images, &o);
            let by_value = prepare_chapter_owned(images.clone(), &o);
            assert_eq!(by_ref.plan, by_value.plan, "{layout:?}");
            for page in &by_ref.plan.pages {
                assert_eq!(
                    render_planned(&by_ref.prepared, page, &o),
                    render_planned(&by_value.prepared, page, &o)
                );
            }
        }
    }

    #[test]
    fn previews_are_device_sized_pngs() {
        let levels = render_levels(&page(), &opts(Depth::Two));
        let previews = levels_to_previews(&levels);
        assert_eq!(previews.len(), 1);
        assert_eq!((previews[0].width, previews[0].height), (480, 800));
        assert_eq!(&previews[0].bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn four_levels_show_as_four_grays_and_two_as_black_and_white() {
        let shades = |depth| {
            let previews = levels_to_previews(&render_levels(&page(), &opts(depth)));
            let decoded = image::load_from_memory(&previews[0].bytes)
                .unwrap()
                .to_luma8();
            let mut seen: Vec<u8> = decoded.pixels().map(|p| p.0[0]).collect();
            seen.sort_unstable();
            seen.dedup();
            seen
        };
        assert_eq!(shades(Depth::Two), vec![0, 85, 170, 255]);
        assert_eq!(shades(Depth::One), vec![0, 255]);
    }

    #[test]
    fn a_spread_previews_as_two_pages() {
        let spread = GrayImage::new(1800, 900, 120).unwrap();
        assert_eq!(
            levels_to_previews(&render_levels(&spread, &opts(Depth::Two))).len(),
            2
        );
    }

    #[test]
    fn the_original_keeps_its_shape_and_tones() {
        let original = render_original(&page(), &opts(Depth::Two)).image;
        assert_eq!(
            (original.width, original.height),
            (600, 900),
            "no letterboxing"
        );
        let decoded = image::load_from_memory(&original.bytes).unwrap().to_luma8();
        let distinct: std::collections::HashSet<u8> = decoded.pixels().map(|p| p.0[0]).collect();
        assert!(
            distinct.len() > 16,
            "all the original's tones remain, not just four"
        );
        assert_eq!(original.mime, "image/jpeg");
    }

    #[test]
    fn very_large_originals_are_shrunk_but_keep_their_proportions() {
        let big = GrayImage::new(3000, 4500, 200).unwrap();
        let o = render_original(&big, &opts(Depth::Two)).image;
        assert_eq!((o.width, o.height), (ORIGINAL_MAX_WIDTH, 1140));
    }

    #[test]
    fn a_tall_strip_keeps_its_width_and_is_not_squashed_into_a_sliver() {
        // Limiting the longest side would have made this about 110 px wide.
        let strip = GrayImage::new(960, 9600, 200).unwrap();
        let o = render_original(&strip, &opts(Depth::Two)).image;
        assert_eq!((o.width, o.height), (ORIGINAL_MAX_WIDTH, 7600));
        // Narrower than the limit: untouched.
        let narrow = GrayImage::new(600, 6000, 200).unwrap();
        let o = render_original(&narrow, &opts(Depth::Two)).image;
        assert_eq!((o.width, o.height), (600, 6000));
        // Absurdly tall: held under the height limit, proportions kept.
        let huge = GrayImage::new(2000, 60_000, 200).unwrap();
        let o = render_original(&huge, &opts(Depth::Two)).image;
        assert_eq!(o.height, ORIGINAL_MAX_HEIGHT);
        assert!((o.width as i32 - 533).abs() <= 1, "{}", o.width);
    }

    #[test]
    fn the_kept_area_follows_the_margin_settings() {
        let img = GrayImage::new(1000, 2000, 255).unwrap();
        let mut o = opts(Depth::Two);
        o.crop = converter::CropMode::Full;
        assert_eq!(render_original(&img, &o).kept, [0.0, 0.0, 1.0, 1.0]);
        o.margins = converter::Margins {
            top: 10,
            bottom: 10,
            left: 20,
            right: 0,
        };
        let kept = render_original(&img, &o).kept;
        assert_eq!(kept, [0.2, 0.1, 0.8, 0.8]);
    }

    #[test]
    fn a_whole_page_renders_to_converted_pages_and_an_original() {
        let source = {
            let g = page();
            let mut out = Vec::new();
            PngEncoder::new(&mut out)
                .write_image(&g.pixels, g.width, g.height, ExtendedColorType::L8)
                .unwrap();
            out
        };
        let rendered = render_page(&source, &opts(Depth::Two)).unwrap();
        assert_eq!(rendered.converted.len(), 1);
        assert_eq!(rendered.converted[0].mime, "image/png");
        assert_eq!(rendered.original.image.mime, "image/jpeg");
        assert!(render_page(b"junk", &opts(Depth::Two)).is_err());
    }

    #[test]
    fn the_sample_page_has_what_the_settings_act_on() {
        let page = sample_page();
        assert_eq!((page.width, page.height), (900, 1400));
        // White margins all round: there is something for the trimming to cut.
        assert!((0..900).all(|x| page.get(x, 10) == 255 && page.get(x, 1390) == 255));
        // A wide spread of tones, so contrast and shading show.
        let mut seen = std::collections::HashSet::new();
        for y in (0..1400).step_by(3) {
            for x in (0..900).step_by(3) {
                seen.insert(page.get(x, y) / 16);
            }
        }
        assert!(seen.len() >= 12, "only {} tone bands", seen.len());
    }

    #[test]
    fn the_sample_page_converts_to_one_screen_in_four_grays() {
        let rendered = render_gray(&sample_page(), &opts(Depth::Two));
        assert_eq!(rendered.converted.len(), 1);
        let decoded = image::load_from_memory(&rendered.converted[0].bytes)
            .unwrap()
            .to_luma8();
        let mut shades: Vec<u8> = decoded.pixels().map(|p| p.0[0]).collect();
        shades.sort_unstable();
        shades.dedup();
        assert_eq!(shades, vec![0, 85, 170, 255], "all four levels are used");
        // The margins were trimmed away by the default settings.
        let kept = rendered.original.kept;
        assert!(kept[2] < 0.95 && kept[3] < 0.95, "{kept:?}");
    }

    #[test]
    fn unreadable_data_is_a_clear_error() {
        assert!(matches!(
            decode_page(b"not an image"),
            Err(PipelineError::Decode(_))
        ));
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;
    use library::CropRect;
    #[test]
    fn stable_keys_ignore_page_positions_but_separate_source_sections() {
        let mut p = PlannedPage {
            source: converter::plan::Source::Image {
                index: 7,
                part: converter::plan::Part::Whole,
            },
            unit_top: 0,
            unit_height: 0,
            landscape: false,
        };
        let key = crop_page_key(&p);
        p.landscape = true;
        assert_eq!(key, crop_page_key(&p));
        p.source = converter::plan::Source::Image {
            index: 7,
            part: converter::plan::Part::Half(0),
        };
        assert_ne!(key, crop_page_key(&p));
        p.source = converter::plan::Source::Strip {
            top: 50,
            height: 80,
        };
        p.unit_top = 100;
        p.unit_height = 160;
        let key = crop_page_key(&p);
        p.source = converter::plan::Source::Strip {
            top: 100,
            height: 160,
        };
        assert_eq!(key, crop_page_key(&p));
        p.unit_height += 1;
        assert_ne!(key, crop_page_key(&p));
    }
    #[test]
    fn cropped_preview_matches_converter_and_source_keeps_raw_tones() {
        let mut o = ProcessOptions::manga_default(40, 60, converter::Depth::Two);
        o.layout = converter::Layout::Manga;
        o.crop = converter::CropMode::Full;
        let gray = GrayImage::from_fn(80, 120, |x, _| (x * 3) as u8).unwrap();
        let chapter = prepare_chapter(std::slice::from_ref(&gray), &o);
        let p = &chapter.plan.pages[0];
        let crop = CropRect {
            left: 0.25,
            ..Default::default()
        };
        let shown = render_planned_with_crop(&chapter.prepared, p, &o, Some(crop));
        let expected =
            converter::plan::render_page_cropped(&chapter.prepared, p, &o, crop.as_array());
        assert_eq!(shown, levels_to_previews(&[expected])[0]);
        let source = render_source_preview(&chapter.prepared, p, &o);
        assert_eq!((source.width, source.height), (80, 120));
        let restored = image::load_from_memory(&source.bytes).unwrap().to_luma8();
        assert_eq!(restored.get_pixel(40, 50).0[0], gray.get(40, 50));
    }
}

/// Crop identity follows source content, never its position in the page list.
pub fn crop_page_key(page: &PlannedPage) -> String {
    use converter::plan::{Part, Piece, Source};
    match page.source {
        Source::Image { index, part } => {
            let name = |piece| match piece {
                Piece::Whole => "whole".to_string(),
                Piece::Rotated => "rotated".to_string(),
                Piece::Half(n) => format!("half:{n}"),
            };
            let part = match part {
                Part::Whole => name(Piece::Whole),
                Part::Rotated => name(Piece::Rotated),
                Part::Half(n) => name(Piece::Half(n)),
                // Each screen of a zoomed page is its own page to crop.
                Part::Tile { of, col, row } => format!("{}:tile{col}x{row}", name(of)),
            };
            format!("image:{index}:{part}")
        }
        Source::Strip { .. } => format!("strip:{}:{}", page.unit_top, page.unit_height),
    }
}
pub fn render_planned_with_crop(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
    crop: Option<library::CropRect>,
) -> PreviewImage {
    level_png(&converter::plan::render_page_cropped(
        prepared,
        page,
        opts,
        crop.unwrap_or_default().as_array(),
    ))
}
/// Source sections retain all tones; PNG makes crop inspection exact.
pub fn render_source_preview(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
) -> PreviewImage {
    shrunk_png(converter::plan::page_source(prepared, page, opts))
}

/// The downloaded picture a page comes from, whole and as it was (a double
/// page stays one wide picture whatever the spread setting), and where on it
/// this page lies (`None` for a turned page).
pub fn render_source_whole(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
) -> (PreviewImage, Option<converter::plan::SourceRegion>) {
    (
        shrunk_png(converter::plan::whole_source(prepared, page, opts)),
        converter::plan::source_region(prepared, page, opts),
    )
}

fn shrunk_png(raw: GrayImage) -> PreviewImage {
    let scale = (ORIGINAL_MAX_WIDTH as f64 / raw.width as f64)
        .min(ORIGINAL_MAX_HEIGHT as f64 / raw.height as f64)
        .min(1.0);
    let shown = if scale < 1.0 {
        resize(
            &raw,
            (raw.width as f64 * scale).round().max(1.0) as u32,
            (raw.height as f64 * scale).round().max(1.0) as u32,
        )
    } else {
        raw
    };
    PreviewImage {
        bytes: encode_png(&shown.pixels, shown.width, shown.height),
        mime: "image/png",
        width: shown.width,
        height: shown.height,
    }
}
