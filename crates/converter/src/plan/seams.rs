//! Telling a strip that arrives in slices from a chapter of pages.
//!
//! Many webtoons are downloaded as dozens of short images, each cut from one
//! long strip wherever the source happened to cut it. Their shape alone looks
//! like pages, so the picture has to be looked at: where one image ends, the
//! next carries on from the same row of the drawing.

use super::*;

/// A chapter needs at least this many images, and this many seams with
/// something drawn beside them, before the seams count as evidence.
const MIN_IMAGES: usize = 4;
const MIN_DRAWN_SEAMS: usize = 3;
/// A row beside a seam has something drawn on it when its tones spread this much.
const DRAWN_SPREAD: f32 = 12.0;
/// A seam is continuous when the jump across it is at most this many times a
/// jump between two rows inside the images.
const CONTINUOUS_RATIO: f32 = 2.5;
/// This share of the seams with something drawn must be continuous.
const CONTINUOUS_SHARE: f32 = 0.6;

/// Whether the images, in order, carry on from one into the next.
pub(super) fn run_on_across_seams(images: &[GrayImage]) -> bool {
    if images.len() < MIN_IMAGES {
        return false;
    }
    let (mut drawn, mut continuous) = (0usize, 0usize);
    for pair in images.windows(2) {
        let Some(seam) = seam(&pair[0], &pair[1]) else {
            continue;
        };
        if seam.drawn {
            drawn += 1;
            if seam.ratio <= CONTINUOUS_RATIO {
                continuous += 1;
            }
        }
    }
    drawn >= MIN_DRAWN_SEAMS && continuous as f32 >= drawn as f32 * CONTINUOUS_SHARE
}

struct Seam {
    /// A row beside the seam has something drawn on it, so a blank gap between
    /// two images (which says nothing) is left out.
    drawn: bool,
    /// The jump across the seam over the jump inside the images.
    ratio: f32,
}

/// How the bottom of `a` meets the top of `b`. Images of a strip are the same
/// width; if these are not, they do not run on.
fn seam(a: &GrayImage, b: &GrayImage) -> Option<Seam> {
    if a.height < 2 || b.height < 2 || a.width.abs_diff(b.width) * 100 > a.width * 3 {
        return None;
    }
    let width = a.width.min(b.width);
    let (above, last) = (row(a, a.height - 2, width), row(a, a.height - 1, width));
    let (first, below) = (row(b, 0, width), row(b, 1, width));
    let inside = (mean_difference(&above, &last) + mean_difference(&first, &below)) / 2.0;
    Some(Seam {
        drawn: spread(&last).max(spread(&first)) >= DRAWN_SPREAD,
        ratio: mean_difference(&last, &first) / (inside + 2.0),
    })
}

/// Row `y` of `img` sampled at `width` points.
fn row(img: &GrayImage, y: u32, width: u32) -> Vec<f32> {
    (0..width)
        .map(|x| img.get((x as u64 * img.width as u64 / width as u64) as u32, y) as f32)
        .collect()
}

fn spread(row: &[f32]) -> f32 {
    let mean = row.iter().sum::<f32>() / row.len().max(1) as f32;
    (row.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / row.len().max(1) as f32).sqrt()
}

fn mean_difference(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len().max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tall picture with something on every row, so a cut anywhere shows.
    fn picture(width: u32, height: u32) -> GrayImage {
        GrayImage::from_fn(width, height, |x, y| {
            (128.0 + 100.0 * ((x as f32 / 13.0).sin() + (y as f32 / 29.0).cos()) / 2.0) as u8
        })
        .unwrap()
    }

    fn slices(whole: &GrayImage, each: u32) -> Vec<GrayImage> {
        (0..whole.height / each)
            .map(|i| crate::process::crop_rect(whole, 0, i * each, whole.width, each))
            .collect()
    }

    #[test]
    fn slices_of_one_picture_run_on() {
        let whole = picture(480, 6000);
        assert!(run_on_across_seams(&slices(&whole, 600)));
    }

    #[test]
    fn unrelated_pictures_do_not_run_on() {
        // Each page has its own drawing: the seams jump.
        let pages: Vec<GrayImage> = (0..6)
            .map(|i| {
                let (fx, fy) = (7.0 + 5.0 * i as f32, 11.0 + 3.0 * i as f32);
                GrayImage::from_fn(480, 700, |x, y| {
                    (128.0 + 100.0 * ((x as f32 / fx).sin() * (y as f32 / fy).cos())) as u8
                })
                .unwrap()
            })
            .collect();
        assert!(!run_on_across_seams(&pages));
    }

    #[test]
    fn pages_with_blank_margins_say_nothing() {
        // White edges beside every seam: nothing is drawn there to compare.
        let pages: Vec<GrayImage> = (0..6)
            .map(|_| {
                GrayImage::from_fn(480, 700, |_, y| if !(40..660).contains(&y) { 255 } else { 90 })
                    .unwrap()
            })
            .collect();
        assert!(!run_on_across_seams(&pages));
    }

    #[test]
    fn a_few_images_are_not_enough_to_tell() {
        let whole = picture(480, 1800);
        assert!(!run_on_across_seams(&slices(&whole, 600)));
    }

    #[test]
    fn images_of_different_widths_do_not_run_on() {
        let whole = picture(480, 6000);
        let mut images = slices(&whole, 600);
        for (i, img) in images.iter_mut().enumerate() {
            if i % 2 == 1 {
                *img = picture(300, 600);
            }
        }
        assert!(!run_on_across_seams(&images));
    }
}
