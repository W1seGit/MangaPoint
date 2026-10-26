//! Tests of the colour render: luma, chroma and padding follow the gray
//! pages exactly, and the chroma planes sit under the luma however the page is
//! cut, turned, zoomed or cropped.

use crate::dither::Dither;
use crate::gray::{ColorImage, Depth, GrayImage};
use crate::plan::{
    plan, prepare_owned, prepare_owned_color, render_all_color_with, render_page_color,
    render_page_cropped, Cancelled, Prepared,
};
use crate::process::{
    CropMode, Layout, PageZoom, ProcessOptions, SpreadMode, ZoomMode,
};
use crate::xtc::ReadDirection;

type Page = (GrayImage, [GrayImage; 2]);

fn never() -> bool {
    false
}

fn options(w: u32, h: u32) -> ProcessOptions {
    let mut o = ProcessOptions::manga_default(w, h, Depth::Eight);
    o.layout = Layout::Manga;
    o.crop = CropMode::Full;
    o.auto_levels = false;
    o.contrast = 50;
    o.dither = Dither::Threshold;
    o
}

fn rgb_to_ycc(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    (q(y), q(cb), q(cr))
}

/// A page from a function of the pixel position giving its colour.
fn page_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Page {
    let mut luma = Vec::new();
    let mut cb = Vec::new();
    let mut cr = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = f(x, y);
            let (yy, bb, rr) = rgb_to_ycc(r, g, b);
            luma.push(yy);
            cb.push(bb);
            cr.push(rr);
        }
    }
    let plane = |p| GrayImage::from_pixels(w, h, p).unwrap();
    (plane(luma), [plane(cb), plane(cr)])
}

/// What chroma a luma level gets in the test pictures: a plain function of the
/// luma that is neutral on white, so any picture whose chroma is not under its
/// luma breaks the relation at the edges of the drawing.
fn cb_of(y: u8) -> u8 {
    128 + ((255 - y as i32) / 10) as u8
}
fn cr_of(y: u8) -> u8 {
    128 + ((255 - y as i32) * 15 / 100) as u8
}

/// Blocky art on white paper with a wide white border, its chroma a function
/// of its luma. `gap`: columns left white all the way down (a gutter).
fn textured(w: u32, h: u32, seed: u32, gap: Option<std::ops::Range<u32>>) -> Page {
    let level = move |x: u32, y: u32| -> u8 {
        let border = 20;
        if x < border || y < border || x + border >= w || y + border >= h {
            return 255;
        }
        if gap.as_ref().is_some_and(|g| g.contains(&x)) {
            return 255;
        }
        let hash = ((x / 10) * 73_856_093) ^ ((y / 10) * 19_349_663) ^ seed.wrapping_mul(83_492_791);
        if hash.is_multiple_of(3) {
            60 + (hash / 3 % 120) as u8
        } else {
            255
        }
    };
    let luma = GrayImage::from_fn(w, h, &level).unwrap();
    let cb = GrayImage::from_fn(w, h, |x, y| cb_of(level(x, y))).unwrap();
    let cr = GrayImage::from_fn(w, h, |x, y| cr_of(level(x, y))).unwrap();
    (luma, [cb, cr])
}

fn both(images: &[Page], o: &ProcessOptions) -> (Prepared, Prepared) {
    let luma = images.iter().map(|(y, _)| y.clone()).collect();
    (
        prepare_owned(luma, o, &never).unwrap(),
        prepare_owned_color(images.to_vec(), o, &never).unwrap(),
    )
}

fn luma_of_rgb(c: &ColorImage, x: u32, y: u32) -> i32 {
    let [r, g, b] = c.get(x, y);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as i32
}

fn chroma_of_rgb(c: &ColorImage, x: u32, y: u32) -> (f32, f32) {
    let [r, g, b] = c.get(x, y).map(|v| v as f32);
    (
        128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b,
        128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b,
    )
}

const CROPS: [[f32; 4]; 2] = [[0.0; 4], [0.1, 0.05, 0.2, 0.1]];

/// Every page of the chapter, plain and manually cropped: the colour page is
/// the gray page's size, its brightness is the gray page's before tone work
/// (within one level), and its chroma is still the function of the luma the
/// art was made with. Returns how many pages were checked.
fn check_layout(name: &str, images: &[Page], o: &ProcessOptions) -> usize {
    let (gray, color) = both(images, o);
    assert!(color.has_color() && !gray.has_color());
    let pg = plan(&gray, o);
    let pc = plan(&color, o);
    assert_eq!(pg, pc, "{name}: the plan is made on the luma alone");
    assert!(!pg.pages.is_empty(), "{name}");
    let mut checked = 0;
    for crop in CROPS {
        for page in &pg.pages {
            let g = render_page_cropped(&gray, page, o, crop);
            let c = render_page_color(&color, page, o, crop);
            assert_eq!((c.width, c.height), (g.width, g.height), "{name}");
            assert_eq!(c.rgb.len(), g.levels.len() * 3, "{name}");
            let mut inked = 0;
            for y in 0..c.height {
                for x in 0..c.width {
                    let level = g.levels[y as usize * g.width as usize + x as usize] as i32;
                    let luma = luma_of_rgb(&c, x, y);
                    assert!(
                        (luma - level).abs() <= 1,
                        "{name} {page:?} crop {crop:?} at {x},{y}: colour luma {luma}, gray {level}"
                    );
                    let (cb, cr) = chroma_of_rgb(&c, x, y);
                    let (want_cb, want_cr) = (cb_of(level as u8) as f32, cr_of(level as u8) as f32);
                    // Rounding in the planes, in RGB and in the luma read back.
                    assert!(
                        (cb - want_cb).abs() <= 4.0 && (cr - want_cr).abs() <= 4.0,
                        "{name} {page:?} crop {crop:?} at {x},{y}: chroma ({cb:.1}, {cr:.1}) with luma {level}, expected ({want_cb}, {want_cr})"
                    );
                    inked += (level < 200) as u32;
                }
            }
            assert!(inked > 0, "{name}: a page with nothing on it proves nothing");
            checked += 1;
        }
    }
    checked
}

#[test]
fn colour_pages_have_the_gray_pages_brightness_and_their_chroma_under_it_in_every_layout() {
    let mut total = 0;

    // Whole pages with the blank border trimmed away.
    let mut o = options(120, 200);
    o.crop = CropMode::Margins;
    let pages = [textured(150, 240, 1, None), textured(170, 230, 2, None), textured(140, 260, 3, None)];
    total += check_layout("trimmed pages", &pages, &o);

    // A double page split at a gutter that is not the middle, both reading ways.
    let spread = || textured(300, 180, 4, Some(180..188));
    for direction in [ReadDirection::RightToLeft, ReadDirection::LeftToRight] {
        let mut o = options(120, 200);
        o.direction = direction;
        o.spread = SpreadMode::Split;
        total += check_layout("split spread", &[spread()], &o);
    }
    let mut o = options(120, 200);
    o.spread = SpreadMode::Rotate;
    total += check_layout("rotated spread", &[spread()], &o);
    o.spread = SpreadMode::Fit;
    total += check_layout("fitted spread", &[spread()], &o);

    // Zoomed: tiles in either direction, and the kept middle.
    for direction in [ReadDirection::RightToLeft, ReadDirection::LeftToRight] {
        let mut o = options(120, 200);
        o.zoom = PageZoom::Percent(200);
        o.zoom_mode = ZoomMode::Tile;
        o.direction = direction;
        total += check_layout("tiled page", &[textured(240, 400, 5, None)], &o);
        o.spread = SpreadMode::Split;
        total += check_layout("tiled halves", &[spread()], &o);
        o.spread = SpreadMode::Rotate;
        total += check_layout("tiled rotated", &[spread()], &o);
    }
    let mut o = options(120, 200);
    o.zoom = PageZoom::Percent(150);
    o.zoom_mode = ZoomMode::Crop;
    total += check_layout("zoomed and cropped", &[textured(240, 400, 6, None)], &o);

    // A page held sideways in the reader's frame.
    let mut o = options(200, 120);
    o.turn = true;
    total += check_layout("turned page", &[textured(150, 240, 7, None)], &o);

    // A webtoon strip made of several pictures, its side margins trimmed.
    let mut o = options(100, 160);
    o.layout = Layout::Webtoon;
    o.crop = CropMode::Margins;
    let strip: Vec<Page> = (0..4).map(|i| textured(120, 320, 10 + i, None)).collect();
    total += check_layout("webtoon strip", &strip, &o);

    assert!(total > 40, "{total} pages checked");
}

#[test]
fn colour_render_is_the_gray_one_when_the_chapter_has_no_colour() {
    let o = options(120, 200);
    let (y, _) = textured(150, 240, 8, None);
    let gray = prepare_owned(vec![y.clone()], &o, &never).unwrap();
    let p = plan(&gray, &o);
    let c = render_page_color(&gray, &p.pages[0], &o, [0.0; 4]);
    let g = render_page_cropped(&gray, &p.pages[0], &o, [0.0; 4]);
    for (px, &level) in c.rgb.chunks(3).zip(&g.levels) {
        assert_eq!(px, [level; 3]);
    }
    // And a chapter whose chroma is neutral is exactly gray as well.
    let neutral = GrayImage::new(150, 240, 128).unwrap();
    let color = prepare_owned_color(vec![(y, [neutral.clone(), neutral])], &o, &never).unwrap();
    let c2 = render_page_color(&color, &p.pages[0], &o, [0.0; 4]);
    assert_eq!(c, c2);
}

#[test]
fn pure_colours_keep_their_hue_and_the_padding_stays_white() {
    let colours: [([u8; 3], usize); 3] = [([255, 0, 0], 0), ([0, 255, 0], 1), ([0, 0, 255], 2)];
    for (rgb, strong) in colours {
        // Portrait art on a portrait-ish screen: bars at the sides.
        let flat = |w, h| vec![page_of(w, h, |_, _| rgb)];
        let mut o = options(200, 200);
        o.spread = SpreadMode::Rotate;
        let cases: Vec<(&str, Vec<Page>, ProcessOptions, [f32; 4])> = vec![
            ("fit", flat(100, 200), o.clone(), [0.0; 4]),
            ("crop", flat(100, 200), o.clone(), [0.1, 0.1, 0.2, 0.2]),
            ("rotate", flat(240, 100), o.clone(), [0.0; 4]),
            ("rotate and crop", flat(240, 100), o.clone(), [0.0, 0.2, 0.1, 0.0]),
        ];
        for (name, images, o, crop) in cases {
            let color = prepare_owned_color(images, &o, &never).unwrap();
            let p = plan(&color, &o);
            let c = render_page_color(&color, &p.pages[0], &o, crop);
            let mut art = 0;
            let mut bars = 0;
            for y in 0..c.height {
                for x in 0..c.width {
                    let px = c.get(x, y);
                    if px == [255, 255, 255] {
                        bars += 1;
                        continue;
                    }
                    art += 1;
                    // The strong channel stays high, the other two stay low.
                    for (k, &v) in px.iter().enumerate() {
                        if k == strong {
                            assert!(v >= 245, "{name} {rgb:?} at {x},{y}: {px:?}");
                        } else {
                            assert!(v <= 12, "{name} {rgb:?} at {x},{y}: {px:?}");
                        }
                    }
                }
            }
            assert!(art > 5_000, "{name}: only {art} coloured pixels");
            if name == "fit" || name == "crop" {
                assert!(bars > 5_000, "{name}: the padding must be white, got {bars} white pixels");
            }
        }
    }
}

/// A page whose luma is perfectly flat and whose chroma carries a marker
/// (a rectangle with a high Cr), and the same marker drawn in the luma of an
/// otherwise flat page. Where the marker lands on the screen must be the same.
fn marker_masks(
    w: u32,
    h: u32,
    marker: (u32, u32, u32, u32),
    o: &ProcessOptions,
    crop: [f32; 4],
) -> (Vec<bool>, Vec<bool>) {
    let inside = move |x: u32, y: u32| {
        let (mx, my, mw, mh) = marker;
        x >= mx && x < mx + mw && y >= my && y < my + mh
    };
    let flat = GrayImage::new(w, h, 128).unwrap();
    let neutral = GrayImage::new(w, h, 128).unwrap();
    let marked_chroma = GrayImage::from_fn(w, h, |x, y| if inside(x, y) { 255 } else { 128 }).unwrap();
    let in_chroma = prepare_owned_color(vec![(flat.clone(), [neutral.clone(), marked_chroma])], o, &never).unwrap();
    let marked_luma = GrayImage::from_fn(w, h, |x, y| if inside(x, y) { 0 } else { 128 }).unwrap();
    let in_luma = prepare_owned(vec![marked_luma], o, &never).unwrap();
    // A flat page trims and splits differently from a marked one, so the
    // plans are made by hand: the same page of each chapter.
    let pages = plan(&in_luma, o).pages;
    let tinted: Vec<bool> = pages
        .iter()
        .flat_map(|page| {
            let c = render_page_color(&in_chroma, page, o, crop);
            (0..c.height)
                .flat_map(|y| (0..c.width).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let [r, _, b] = c.get(x, y);
                    r as i32 - b as i32 > 89
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let dark: Vec<bool> = pages
        .iter()
        .flat_map(|page| {
            let g = render_page_cropped(&in_luma, page, o, crop);
            g.levels.iter().map(|&l| l < 64).collect::<Vec<_>>()
        })
        .collect();
    (tinted, dark)
}

#[test]
fn chroma_with_a_flat_luma_lands_where_the_same_mark_in_the_luma_does() {
    type Case = (&'static str, u32, u32, (u32, u32, u32, u32), ProcessOptions, [f32; 4]);
    let mut cases: Vec<Case> = Vec::new();
    cases.push(("whole", 120, 300, (10, 20, 40, 60), options(100, 160), [0.0; 4]));
    cases.push(("whole cropped", 120, 300, (10, 20, 40, 60), options(100, 160), [0.05, 0.1, 0.1, 0.2]));
    let mut o = options(100, 160);
    o.spread = SpreadMode::Rotate;
    cases.push(("rotated", 300, 120, (200, 10, 60, 40), o.clone(), [0.0; 4]));
    o.turn = true;
    cases.push(("rotated and turned", 300, 120, (200, 10, 60, 40), o, [0.0; 4]));
    let mut o = options(100, 160);
    o.zoom = PageZoom::Percent(200);
    o.zoom_mode = ZoomMode::Tile;
    cases.push(("tiles", 200, 300, (20, 30, 150, 40), o.clone(), [0.0; 4]));
    o.direction = ReadDirection::LeftToRight;
    cases.push(("tiles left to right", 200, 300, (20, 130, 150, 100), o.clone(), [0.0; 4]));
    o.zoom_mode = ZoomMode::Crop;
    cases.push(("zoomed middle", 200, 300, (60, 100, 80, 100), o, [0.0; 4]));
    for (name, w, h, marker, o, crop) in cases {
        let (tinted, dark) = marker_masks(w, h, marker, &o, crop);
        let marked = dark.iter().filter(|&&d| d).count();
        assert!(marked > 300, "{name}: the mark must show ({marked} pixels)");
        let differ = tinted.iter().zip(&dark).filter(|(a, b)| a != b).count();
        // Edge pixels may round either way; a displaced mark would differ by far more.
        assert!(differ * 100 <= marked * 3, "{name}: {differ} of {marked} mark pixels differ");
    }
}

#[test]
fn chroma_is_cut_at_the_spine_found_on_the_luma() {
    // Two dark halves on white paper with a gutter well off the middle (columns
    // 88..93 of 200). Chroma is high on the right of the gutter only. The
    // right half must be tinted everywhere it is dark, the left half nowhere.
    let (w, h) = (200, 100);
    let luma = GrayImage::from_fn(w, h, |x, y| {
        if (88..93).contains(&x) || !(10..90).contains(&y) {
            255
        } else {
            40
        }
    })
    .unwrap();
    let cb = GrayImage::new(w, h, 128).unwrap();
    let cr = GrayImage::from_fn(w, h, |x, _| if x >= 90 { 200 } else { 128 }).unwrap();
    let mut o = options(100, 150);
    o.spread = SpreadMode::Split;
    o.direction = ReadDirection::LeftToRight;
    let (gray, color) = both(&[(luma, [cb, cr])], &o);
    let p = plan(&gray, &o);
    assert_eq!(p.pages.len(), 2);
    for (i, page) in p.pages.iter().enumerate() {
        let g = render_page_cropped(&gray, page, &o, [0.0; 4]);
        let c = render_page_color(&color, page, &o, [0.0; 4]);
        let mut ink = 0;
        for y in 0..c.height {
            for x in 0..c.width {
                if g.levels[(y * g.width + x) as usize] > 100 {
                    continue;
                }
                ink += 1;
                let [r, _, b] = c.get(x, y);
                let tint = r as i32 - b as i32;
                if i == 0 {
                    assert!(tint.abs() <= 2, "left half tinted at {x},{y}");
                } else {
                    assert!(tint > 40, "right half untinted at {x},{y}: {:?}", c.get(x, y));
                }
            }
        }
        assert!(ink > 1_000);
    }
}

#[test]
fn cancelling_stops_the_colour_render_and_the_colour_prepare() {
    let o = options(120, 200);
    let images: Vec<Page> = (0..6).map(|i| textured(150, 240, i, None)).collect();
    let (_, color) = both(&images, &o);
    let p = plan(&color, &o);
    let drawn = std::sync::atomic::AtomicUsize::new(0);
    let result = render_all_color_with(&color, &p.pages, &o, &[], &|| true, |_, _| {
        drawn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    assert_eq!(result.err(), Some(Cancelled));
    assert_eq!(drawn.load(std::sync::atomic::Ordering::SeqCst), 0, "no page is drawn once cancelled");
    assert!(prepare_owned_color(images.clone(), &o, &|| true).is_err());
    let mut strip = o.clone();
    strip.layout = Layout::Webtoon;
    assert!(prepare_owned_color(images, &strip, &|| true).is_err());

    // Not cancelled: every page comes back, in order.
    let all = render_all_color_with(&color, &p.pages, &o, &[], &never, |i, c| (i, c.width)).unwrap();
    assert_eq!(all.iter().map(|&(i, _)| i).collect::<Vec<_>>(), (0..p.pages.len()).collect::<Vec<_>>());
}

#[test]
fn the_parallel_colour_render_equals_rendering_page_by_page() {
    let mut o = options(120, 200);
    o.zoom = PageZoom::Percent(200);
    o.spread = SpreadMode::Split;
    let images = vec![textured(300, 180, 4, Some(180..188)), textured(240, 400, 5, None)];
    let (_, color) = both(&images, &o);
    let p = plan(&color, &o);
    let crops = vec![[0.0; 4], [0.1, 0.0, 0.0, 0.1]];
    let all = render_all_color_with(&color, &p.pages, &o, &crops, &never, |_, c| c).unwrap();
    for (i, page) in p.pages.iter().enumerate() {
        let one = render_page_color(&color, page, &o, crops.get(i).copied().unwrap_or([0.0; 4]));
        assert_eq!(all[i], one, "page {i}");
    }
}

#[test]
fn colour_from_planes_is_exact_on_gray_and_white() {
    let luma = GrayImage::from_fn(256, 1, |x, _| x as u8).unwrap();
    let flat = GrayImage::new(256, 1, 128).unwrap();
    let c = ColorImage::from_ycc(&luma, &flat, &flat);
    for x in 0..256u32 {
        assert_eq!(c.get(x, 0), [x as u8; 3]);
    }
}
