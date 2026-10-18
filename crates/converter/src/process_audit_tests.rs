//! Tests of the faster pixel routines against the ones they replaced, and of
//! the edges that used to panic.

use super::*;
use crate::reference::{self, scan, Rng};

#[test]
fn resize_gives_the_same_pixels_as_the_whole_image_version() {
    let mut rng = Rng(0xC0FFEE);
    let sizes = [
        (1, 1, 5, 7),
        (7, 9, 1, 1),
        (480, 900, 240, 450),
        (333, 777, 480, 800),
        (1000, 3000, 480, 1440),
        (480, 2000, 480, 3),
        (50, 50, 490, 1200),
        (100, 40, 100, 33),
        (640, 480, 641, 479),
        (3, 4000, 480, 6000),
    ];
    for (sw, sh, dw, dh) in sizes {
        let img = scan(&mut rng, sw, sh);
        assert_eq!(
            resize(&img, dw, dh),
            reference::resize_old(&img, dw, dh),
            "{sw}x{sh} -> {dw}x{dh}"
        );
    }
    for _ in 0..30 {
        let (sw, sh) = (1 + rng.below(300), 1 + rng.below(300));
        let (dw, dh) = (1 + rng.below(400), 1 + rng.below(400));
        let img = scan(&mut rng, sw, sh);
        assert_eq!(
            resize(&img, dw, dh),
            reference::resize_old(&img, dw, dh),
            "{sw}x{sh} -> {dw}x{dh}"
        );
    }
}

#[test]
fn the_trim_finds_the_same_bounds_as_the_column_by_column_scan() {
    let mut rng = Rng(42);
    for round in 0..60 {
        let (w, h) = (20 + rng.below(600), 20 + rng.below(900));
        let img = scan(&mut rng, w, h);
        let threshold = trim_threshold(rng.below(101) as u8);
        let pad = rng.below(12) as u8;
        assert_eq!(
            trim_bounds(&img, threshold, pad),
            reference::trim_bounds_old(&img, threshold, pad),
            "round {round}: {w}x{h}"
        );
    }
    // Blank and tiny pictures.
    for img in [
        GrayImage::new(50, 50, 255).unwrap(),
        GrayImage::new(1, 1, 0).unwrap(),
        GrayImage::new(3, 200, 10).unwrap(),
    ] {
        assert_eq!(
            trim_bounds(&img, 28, 4),
            reference::trim_bounds_old(&img, 28, 4)
        );
    }
}

#[test]
fn placing_a_picture_on_a_canvas_matches_the_pixel_by_pixel_copy() {
    let mut rng = Rng(9);
    for _ in 0..40 {
        let (cw, ch) = (1 + rng.below(80), 1 + rng.below(80));
        let (sw, sh) = (1 + rng.below(100), 1 + rng.below(100));
        let (x0, y0) = (rng.below(90), rng.below(90));
        let src = scan(&mut rng, sw, sh);
        let mut fast = GrayImage::new(cw, ch, 255).unwrap();
        let mut slow = fast.clone();
        blit(&mut fast, &src, x0, y0);
        reference::blit_old(&mut slow, &src, x0, y0);
        assert_eq!(fast, slow, "{sw}x{sh} at {x0},{y0} on {cw}x{ch}");
    }
}

#[test]
fn a_crop_starting_outside_the_picture_is_held_to_its_last_pixel() {
    let img = GrayImage::from_fn(10, 6, |x, y| (x + y * 10) as u8).unwrap();
    // The corner on the edge itself used to read past the end of the pixels.
    let corner = crop_rect(&img, 10, 6, 5, 5);
    assert_eq!((corner.width, corner.height), (1, 1));
    assert_eq!(corner.pixels, vec![img.get(9, 5)]);
    let beyond = crop_rect(&img, 4_000_000_000, 99, u32::MAX, 3);
    assert_eq!((beyond.width, beyond.height), (1, 1));
    assert_eq!(beyond.pixels, vec![img.get(9, 5)]);
    let bottom = crop_rect(&img, 3, 6, 4, 4);
    assert_eq!((bottom.width, bottom.height), (4, 1));
    assert_eq!(bottom.pixels, img.pixels[5 * 10 + 3..5 * 10 + 7].to_vec());
    // A crop that is inside is as before.
    let inner = crop_rect(&img, 2, 1, 3, 2);
    assert_eq!(inner.pixels, vec![12, 13, 14, 22, 23, 24]);
}
