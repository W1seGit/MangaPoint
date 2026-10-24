//! A few made-up pages, so the Export screen has something to show (and to try
//! presets on) before any series has been chosen. They are kept as a series of
//! their own, which the library never lists and which cannot be exported.

use crate::state::{library_files, now_secs, AppState};
use dioxus::prelude::*;
use image::{ExtendedColorType, GrayImage, ImageEncoder, Luma};
use library::{series_id, DownloadedChapter, SAMPLE_TITLE};

/// Letters and digits of a 5 by 7 pixel font, as rows of "1" (ink) and "0".
fn glyph(c: char) -> Option<[&'static str; 7]> {
    Some(match c {
        'A' => [
            "01110", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'E' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "11111",
        ],
        'G' => [
            "01110", "10001", "10000", "10111", "10001", "10001", "01110",
        ],
        'K' => [
            "10001", "10010", "10100", "11000", "10100", "10010", "10001",
        ],
        'L' => [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
        'M' => [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
        'P' => [
            "11110", "10001", "10001", "11110", "10000", "10000", "10000",
        ],
        'R' => [
            "11110", "10001", "10001", "11110", "10100", "10010", "10001",
        ],
        'S' => [
            "01111", "10000", "10000", "01110", "00001", "00001", "11110",
        ],
        'T' => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
        'W' => [
            "10001", "10001", "10001", "10101", "10101", "11011", "10001",
        ],
        '1' => [
            "00100", "01100", "00100", "00100", "00100", "00100", "01110",
        ],
        '2' => [
            "01110", "10001", "00001", "00010", "00100", "01000", "11111",
        ],
        '3' => [
            "11110", "00001", "00001", "01110", "00001", "00001", "11110",
        ],
        '4' => [
            "00010", "00110", "01010", "10010", "11111", "00010", "00010",
        ],
        _ => return None,
    })
}

/// Write `text` with its top-left corner at (`x`, `y`), each font pixel being
/// `scale` image pixels.
fn text(img: &mut GrayImage, x: u32, y: u32, text: &str, scale: u32, shade: u8) {
    let mut left = x;
    for c in text.chars() {
        if let Some(rows) = glyph(c) {
            for (dy, row) in rows.iter().enumerate() {
                for (dx, bit) in row.chars().enumerate() {
                    if bit == '1' {
                        fill(
                            img,
                            left + dx as u32 * scale,
                            y + dy as u32 * scale,
                            scale,
                            scale,
                            shade,
                        );
                    }
                }
            }
        }
        left += 6 * scale;
    }
}

/// How wide `text` is when written at `scale`.
fn text_width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32 * 6).saturating_sub(1) * scale
}

fn fill(img: &mut GrayImage, x: u32, y: u32, w: u32, h: u32, shade: u8) {
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, Luma([shade]));
        }
    }
}

/// A page: a title, a shaded panel, a speech bubble and a strip along the
/// bottom, like the watermark some sites put on their pages.
fn page(number: u32, width: u32, height: u32) -> GrayImage {
    let mut img = GrayImage::from_pixel(width, height, Luma([255]));
    let title = format!("SAMPLE PAGE {number}");
    let scale = (width / 70).max(4);
    text(
        &mut img,
        (width.saturating_sub(text_width(&title, scale))) / 2,
        height / 40 + scale,
        &title,
        scale,
        20,
    );
    // The panel, shaded from dark to light.
    let (px, py) = (width / 10, height / 7);
    let (pw, ph) = (width * 8 / 10, height / 2);
    for y in 0..ph {
        for x in 0..pw {
            let shade = 40 + ((x + y) * 180 / (pw + ph)) as u8;
            img.put_pixel(px + x, py + y, Luma([shade]));
        }
    }
    // A few circles inside it, as line art.
    for k in 0..5u32 {
        let (cx, cy, r) = (px + pw / 4 + k * pw / 9, py + ph / 3 + k * ph / 12, ph / 6);
        for a in 0..720 {
            let t = (a as f32).to_radians() / 2.0;
            for w in 0..3 {
                let rr = (r + w) as f32;
                let (x, y) = (cx as f32 + rr * t.cos(), cy as f32 + rr * t.sin());
                if x >= 0.0 && y >= 0.0 && (x as u32) < width && (y as u32) < height {
                    img.put_pixel(x as u32, y as u32, Luma([10]));
                }
            }
        }
    }
    // A speech bubble with lines of lettering.
    let (bx, by, bw, bh) = (px, py + ph + height / 14, pw * 6 / 10, height / 6);
    for y in 0..bh {
        for x in 0..bw {
            let (dx, dy) = (
                (x as f32 - bw as f32 / 2.0) / (bw as f32 / 2.0),
                (y as f32 - bh as f32 / 2.0) / (bh as f32 / 2.0),
            );
            if dx * dx + dy * dy <= 1.0 {
                let edge = dx * dx + dy * dy > 0.94;
                img.put_pixel(bx + x, by + y, Luma([if edge { 10 } else { 250 }]));
            }
        }
    }
    for line in 0..3 {
        fill(
            &mut img,
            bx + bw / 6,
            by + bh / 4 + line * bh / 6,
            bw * 2 / 3 - line * bw / 10,
            (bh / 24).max(3),
            30,
        );
    }
    // The strip along the bottom.
    let strip = height / 20;
    fill(&mut img, 0, height - strip, width, strip, 0);
    text(
        &mut img,
        scale * 2,
        height - strip + strip / 6,
        "SAMPLE WATERMARK",
        (strip / 10).max(2),
        255,
    );
    img
}

/// The sample pages as PNG files: three upright pages and one double page.
fn sample_pages() -> Vec<Vec<u8>> {
    [
        (1, 800, 1200),
        (2, 800, 1200),
        (3, 1600, 1000),
        (4, 800, 1200),
    ]
    .into_iter()
    .map(|(n, w, h)| {
        let img = page(n, w, h);
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), w, h, ExtendedColorType::L8)
            .expect("a gray image encodes as a PNG");
        out
    })
    .collect()
}

/// Make the sample series, and its pages on disk, if they are not there yet.
/// Returns its id.
pub fn ensure_sample(state: AppState) -> String {
    let id = series_id(SAMPLE_TITLE);
    let files = library_files();
    let pages = sample_pages();
    let mut bytes = 0u64;
    for (i, png) in pages.iter().enumerate() {
        let path = files.page_path(&id, "c1", i);
        bytes += png.len() as u64;
        let there = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
        if !there {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, png);
        }
    }
    if state.library.peek().find(&id).is_none() {
        let (defaults, preset) = {
            let s = state.ui.settings.peek();
            (s.conversion.clone(), s.default_preset_id.clone())
        };
        let chapter = DownloadedChapter {
            key: "c1".into(),
            number: Some(1.0),
            name: "Sample pages".into(),
            source_id: "sample".into(),
            chapter_id: 0,
            pages: pages.len(),
            bytes,
            downloaded_at: now_secs(),
            quality: None,
        };
        let mut library = state.library;
        {
            let mut db = library.write();
            db.record_chapter(SAMPLE_TITLE, &defaults, chapter, now_secs());
            db.adopt_default_preset(&id, &defaults, &preset);
        }
        state.save_library();
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_has_upright_pages_and_a_double_page() {
        let pages = sample_pages();
        assert_eq!(pages.len(), 4);
        let sizes: Vec<(u32, u32)> = pages
            .iter()
            .map(|p| image::load_from_memory(p).unwrap())
            .map(|i| (i.width(), i.height()))
            .collect();
        assert_eq!(sizes[2], (1600, 1000), "the third page is a spread");
        assert!(sizes[0].1 > sizes[0].0);
    }

    #[test]
    fn a_page_has_ink_and_blank_paper_and_a_strip_to_crop() {
        let img = page(1, 800, 1200);
        assert_eq!(img.get_pixel(5, 600).0[0], 255, "paper");
        assert_eq!(img.get_pixel(400, 1195).0[0], 0, "the strip at the bottom");
        let dark = img.pixels().filter(|p| p.0[0] < 60).count();
        assert!(dark > 5000, "{dark}");
    }
}
