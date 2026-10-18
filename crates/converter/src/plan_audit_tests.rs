//! Tests of the faster strip and zoom code against the routines they replaced.

use super::*;
use crate::dither::Dither;
use crate::gray::Depth;
use crate::reference::{self, scan, Rng};

/// The strip as it was built before it was done image by image on every core.
fn build_strip_old(images: &[GrayImage], opts: &ProcessOptions) -> Strip {
    let side = |p: u8| p.min(Margins::MAX_PERCENT) as u64;
    let cut = |len: u32, p: u8| (len as u64 * side(p) / 100) as u32;

    // Fixed margins first. Top and bottom apply to every image (a banner on
    // each slice); left and right are the same cut everywhere.
    let fixed: Vec<GrayImage> = images
        .iter()
        .map(|img| {
            let x = cut(img.width, opts.margins.left);
            let y = cut(img.height, opts.margins.top);
            let w = img
                .width
                .saturating_sub(x + cut(img.width, opts.margins.right))
                .max(1);
            let h = img
                .height
                .saturating_sub(y + cut(img.height, opts.margins.bottom))
                .max(1);
            crop_rect(img, x, y, w, h)
        })
        .collect();

    // Blank side margins: one cut for the whole chapter, so every image keeps
    // the same width and the strip does not jitter sideways.
    let (left, right) = if opts.crop == CropMode::Margins {
        let threshold = trim_threshold(opts.trim_sensitivity);
        let mut left = 1.0f32;
        let mut right = 0.0f32;
        let mut any = false;
        for img in &fixed {
            if let Some((x, _, w, _)) = reference::trim_bounds_old(img, threshold, opts.trim_padding) {
                left = left.min(x as f32 / img.width as f32);
                right = right.max((x + w) as f32 / img.width as f32);
                any = true;
            } else {
                left = 0.0;
                right = 1.0;
                any = true;
            }
        }
        if any && right > left {
            (left, right)
        } else {
            (0.0, 1.0)
        }
    } else {
        (0.0, 1.0)
    };

    let width = opts.width;
    let threshold = trim_threshold(opts.trim_sensitivity);
    let mut parts = Vec::with_capacity(fixed.len());
    let mut starts = Vec::with_capacity(fixed.len());
    let mut blank = Vec::new();
    let mut total = 0u32;
    for img in &fixed {
        let x = (left * img.width as f32).floor() as u32;
        let w = (((right - left) * img.width as f32).round() as u32).clamp(1, img.width - x);
        let sideways = if (x, w) == (0, img.width) {
            img.clone()
        } else {
            crop_rect(img, x, 0, w, img.height)
        };
        let scaled_h = (sideways.height as f64 * width as f64 / sideways.width as f64)
            .round()
            .max(1.0) as u32;
        let scaled = reference::resize_old(&sideways, width, scaled_h);
        let bg = border_median(&scaled);
        let row_limit = (width / 200).max(1);
        for y in 0..scaled.height {
            let ink = (0..width)
                .filter(|&x| (scaled.get(x, y) as i16 - bg as i16).abs() > threshold)
                .count() as u32;
            blank.push(ink <= row_limit);
        }
        starts.push(total);
        total += scaled.height;
        parts.push(scaled);
    }
    Strip {
        width,
        parts,
        starts,
        total,
        blank,
        truncated: false,
    }
}


fn options() -> ProcessOptions {
    let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
    o.layout = Layout::Webtoon;
    o.auto_levels = false;
    o.contrast = 50;
    o.dither = Dither::Threshold;
    o
}

fn same_strip(a: &Strip, b: &Strip, what: &str) {
    assert_eq!(a.width, b.width, "{what}");
    assert_eq!(a.starts, b.starts, "{what}");
    assert_eq!(a.total, b.total, "{what}");
    assert_eq!(a.blank, b.blank, "{what}");
    assert_eq!(a.parts, b.parts, "{what}");
}

#[test]
fn the_parallel_strip_is_the_serial_original_on_random_chapters() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for round in 0..24 {
        let n = 1 + rng.below(7) as usize;
        let images: Vec<GrayImage> = (0..n)
            .map(|_| {
                let w = 40 + rng.below(700);
                let h = 30 + rng.below(1800);
                scan(&mut rng, w, h)
            })
            .collect();
        let mut o = options();
        o.crop = if round % 3 == 0 { CropMode::Full } else { CropMode::Margins };
        o.trim_sensitivity = 20 + rng.below(70) as u8;
        o.trim_padding = rng.below(12) as u8;
        if round % 2 == 1 {
            o.margins = Margins {
                top: rng.below(15) as u8,
                bottom: rng.below(15) as u8,
                left: rng.below(15) as u8,
                right: rng.below(15) as u8,
            };
        }
        let new = build_strip(images.clone(), &o, &never).unwrap();
        let old = build_strip_old(&images, &o);
        assert!(!new.truncated);
        same_strip(&new, &old, &format!("round {round}"));
    }
}

#[test]
fn owned_and_borrowed_conversion_give_the_same_pages() {
    let mut rng = Rng(77);
    let images: Vec<GrayImage> = (0..5).map(|_| scan(&mut rng, 500, 1400)).collect();
    for layout in [Layout::Webtoon, Layout::Manga] {
        let mut o = options();
        o.layout = layout;
        o.crop = CropMode::Margins;
        let borrowed = convert_chapter(&images, &o);
        let owned = convert_chapter_owned(images.clone(), &o, &never).unwrap();
        assert_eq!(borrowed, owned, "{layout:?}");
        assert!(!borrowed.is_empty());
    }
}

#[test]
fn a_tall_narrow_image_is_cut_short_instead_of_allocating_gigabytes() {
    // 40 x 800,000 is far below the decoder's 120 MP limit, but scaled to a
    // 480-wide screen it would be 9.6 million rows (4.6 GB).
    let tall = GrayImage::from_fn(40, 800_000, |_, y| if (y / 50) % 2 == 0 { 30 } else { 230 }).unwrap();
    // And a picture one pixel wide: its scaled height would overflow a u32.
    let sliver = GrayImage::from_fn(1, 400_000, |_, y| (y % 251) as u8).unwrap();
    let o = options();
    let started = std::time::Instant::now();
    let prepared = prepare_owned(vec![tall, sliver], &o, &never).unwrap();
    assert!(prepared.truncated());
    let Kind::Strip(strip) = &prepared.kind else { panic!("a strip") };
    assert!(strip.total <= MAX_STRIP_ROWS, "{} rows", strip.total);
    assert!(strip.parts.iter().all(|p| p.height <= MAX_IMAGE_ROWS));
    assert_eq!(strip.blank.len(), strip.total as usize);
    let plan = plan(&prepared, &o);
    assert!(!plan.pages.is_empty());
    let page = render_page(&prepared, &plan.pages[0], &o);
    assert_eq!((page.width, page.height), (480, 800));
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

#[test]
fn images_that_fit_are_not_truncated_and_a_long_chapter_stops_at_the_strip_limit() {
    let o = options();
    let few = vec![GrayImage::new(480, 3000, 200).unwrap(); 3];
    assert!(!prepare(&few, &o).truncated());
    // 5 images of 140,000 rows each is 700,000 rows: over the 600,000 limit.
    let mut o = options();
    o.crop = CropMode::Full;
    let long: Vec<GrayImage> = (0..5)
        .map(|_| GrayImage::new(48, 14_000, 200).unwrap())
        .collect();
    let prepared = prepare_owned(long, &o, &never).unwrap();
    assert!(prepared.truncated());
    let Kind::Strip(strip) = &prepared.kind else { panic!("a strip") };
    assert_eq!(strip.total, MAX_STRIP_ROWS);
}

#[test]
fn cancelling_stops_the_preparation_and_the_rendering_early() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let o = {
        let mut o = options();
        o.crop = CropMode::Full;
        o
    };
    // Cancelled before it starts.
    let images = vec![GrayImage::new(480, 3000, 200).unwrap(); 4];
    assert_eq!(prepare_owned(images.clone(), &o, &|| true).err(), Some(Cancelled));
    let mut paged = o.clone();
    paged.layout = Layout::Manga;
    assert_eq!(prepare_owned(images, &paged, &|| true).err(), Some(Cancelled));

    // A chapter of many pages, cancelled after a few have been asked for.
    let prepared = prepare(&[GrayImage::from_fn(480, 140_000, |x, y| if (x / 6 + y / 9) % 2 == 0 { 40 } else { 220 }).unwrap()], &o);
    let plan = plan(&prepared, &o);
    assert!(plan.pages.len() > 150, "{} pages", plan.pages.len());
    let asked = AtomicUsize::new(0);
    let cancel = || asked.fetch_add(1, Ordering::SeqCst) >= 5;
    let result = render_all_cropped_cancellable(&prepared, &plan.pages, &o, &[], &cancel);
    assert_eq!(result.err(), Some(Cancelled));
    // Counting the checks proves it stopped early without depending on how fast the machine is.
    let checks = asked.load(Ordering::SeqCst);
    assert!(checks < 5 + 2 * 64 + 8, "{checks} checks for {} pages", plan.pages.len());
    assert!(checks < plan.pages.len(), "{checks} checks for {} pages", plan.pages.len());
}

#[test]
fn screens_of_a_zoomed_page_share_one_cut_and_match_rendering_them_alone() {
    let mut rng = Rng(5);
    let images = vec![
        scan(&mut rng, 900, 1300),
        scan(&mut rng, 1700, 1000), // a double page
        scan(&mut rng, 1500, 900),
    ];
    for (spread, arrows, crop) in [
        (SpreadMode::Split, false, [0.0; 4]),
        (SpreadMode::Split, true, [0.0; 4]),
        (SpreadMode::Rotate, true, [0.0; 4]),
        (SpreadMode::Split, true, [0.05, 0.1, 0.0, 0.07]),
        (SpreadMode::Rotate, false, [0.1, 0.0, 0.02, 0.0]),
    ] {
        let mut o = options();
        o.layout = Layout::Manga;
        o.crop = CropMode::Full;
        o.spread = spread;
        o.guide_arrows = arrows;
        o.zoom = crate::process::PageZoom::Percent(220);
        let prepared = prepare(&images, &o);
        let plan = plan(&prepared, &o);
        assert!(plan.pages.len() > 6, "{} pages", plan.pages.len());
        let crops = vec![crop; plan.pages.len()];
        let together = render_all_cropped(&prepared, &plan.pages, &o, &crops);
        for (i, page) in plan.pages.iter().enumerate() {
            let alone = render_page_cropped(&prepared, page, &o, crop);
            assert_eq!(together[i], alone, "{spread:?} arrows {arrows} page {i}");
        }
    }
}

/// `cargo test -p converter --release -- --ignored --nocapture timing`
#[test]
#[ignore]
fn timing_of_the_strip_and_the_zoom_against_the_originals() {
    let mut rng = Rng(11);
    let images: Vec<GrayImage> = (0..40).map(|_| scan(&mut rng, 800, 4000)).collect();
    let mut o = options();
    o.crop = CropMode::Margins;
    let started = std::time::Instant::now();
    let old = build_strip_old(&images, &o);
    let before = started.elapsed();
    let started = std::time::Instant::now();
    let new = build_strip(images.clone(), &o, &never).unwrap();
    let after = started.elapsed();
    same_strip(&new, &old, "timing");
    eprintln!("build_strip, 40 images 800x4000: before {before:?}, after {after:?}");

    let pages: Vec<GrayImage> = (0..6).map(|_| scan(&mut rng, 1700, 1100)).collect();
    let mut z = options();
    z.layout = Layout::Manga;
    z.crop = CropMode::Full;
    z.zoom = crate::process::PageZoom::Percent(250);
    let prepared = prepare(&pages, &z);
    let plan = plan(&prepared, &z);
    let started = std::time::Instant::now();
    let alone: Vec<_> = plan
        .pages
        .iter()
        .map(|p| render_page_cropped(&prepared, p, &z, [0.0; 4]))
        .collect();
    let before = started.elapsed();
    let started = std::time::Instant::now();
    let together = render_all_cropped(&prepared, &plan.pages, &z, &[]);
    let after = started.elapsed();
    assert_eq!(alone, together);
    eprintln!(
        "{} zoom screens: one by one, one core {before:?}; shared cuts on all cores {after:?}",
        plan.pages.len()
    );
}
