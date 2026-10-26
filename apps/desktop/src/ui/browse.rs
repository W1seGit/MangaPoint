//! The cat going through the sources: a row of little browser windows hangs on
//! the wall, one of them loading, and the cat sits on the floor holding a
//! magnifying glass up to them. Drawn in code as SVG rectangles on a coarse
//! grid like the other cats, with its edges breaking up into the page's black;
//! the motion is CSS (see `crates/ui/css/art.css`) and moves in whole pixels.

use super::cat::sitting_markup;
use dioxus::prelude::*;
use std::sync::OnceLock;

const W: i32 = 120;
const H: i32 = 56;
/// Where the floor starts.
const FLOOR: i32 = 48;

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    out.push_str(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>"
    ));
}

/// The pixels of a ring around (`cx`, `cy`) between two radii.
fn ring(out: &mut String, cx: f32, cy: f32, inner: f32, outer: f32, fill: &str) {
    for y in 0..H {
        for x in 0..W {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d >= inner && d <= outer {
                rect(out, x, y, 1, 1, fill);
            }
        }
    }
}

fn noise(x: i32, y: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65_536.0
}

/// Does the pixel at (`x`, `y`) show? Whole in the middle, breaking up toward
/// the sides and the bottom, pixel by pixel.
fn shows(x: i32, y: i32) -> bool {
    let side = x.min(W - 1 - x) as f32;
    let bottom = (H - 1 - y) as f32;
    let top = y as f32;
    let keep = ((side - 2.0) / 24.0)
        .clamp(0.0, 1.0)
        .min(((bottom - 1.0) / 6.0).clamp(0.0, 1.0))
        .min(((top - 1.0) / 5.0).clamp(0.0, 1.0));
    let r = 0.6 * noise(x, y) + 0.4 * noise(x.div_euclid(2) + 91, y.div_euclid(2) + 17);
    r < keep
}

/// Page-black squares over everything that does not show.
fn edge_cover() -> String {
    let mut out = String::new();
    rect(&mut out, -1, -1, W + 2, 1, "#000000");
    rect(&mut out, -1, H, W + 2, 1, "#000000");
    rect(&mut out, -1, 0, 1, H, "#000000");
    rect(&mut out, W, 0, 1, H, "#000000");
    for y in 0..H {
        let mut x = 0;
        while x < W {
            if shows(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < W && !shows(x, y) {
                x += 1;
            }
            rect(&mut out, start, y, x - start, 1, "#000000");
        }
    }
    out
}

/// One little browser window: a title bar with three dots, and lines of page.
fn window(out: &mut String, x: i32, y: i32, lines: &[i32], hot: bool) {
    let (frame, bar, line) = if hot {
        ("#4a5d8a", "#33466e", "#7d93c4")
    } else {
        ("#2a2f3a", "#222733", "#3a4252")
    };
    rect(out, x, y, 18, 15, frame);
    rect(out, x + 1, y + 1, 16, 3, bar);
    for dx in [2, 4, 6] {
        rect(out, x + dx, y + 2, 1, 1, "#6b7388");
    }
    rect(out, x + 1, y + 4, 16, 10, "#0e1018");
    for (i, len) in lines.iter().enumerate() {
        rect(out, x + 2, y + 6 + i as i32 * 3, *len, 1, line);
    }
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let mut s = String::new();
        // A few stars.
        for (x, y) in [(10, 4), (34, 2), (60, 5), (88, 3), (108, 6), (22, 14), (100, 18)] {
            rect(&mut s, x, y, 1, 1, "#4a4a4a");
        }
        // The wall of windows: five sources, the middle one loading.
        let lines: [&[i32]; 5] = [&[10, 7, 11], &[12, 9], &[8, 12, 6], &[11, 6, 10], &[9, 12]];
        for (i, l) in lines.iter().enumerate() {
            window(&mut s, 8 + i as i32 * 22, 6, l, i == 2);
        }
        // The middle window loads: a bar that fills in steps.
        s.push_str("<g class=\"browse-load\">");
        rect(&mut s, 52 + 2, 6 + 12, 12, 1, "#9fb4e6");
        s.push_str("</g>");
        // The floor.
        rect(&mut s, 0, FLOOR, W, H - FLOOR, "#15110f");
        rect(&mut s, 0, FLOOR, W, 1, "#3a2f27");
        for x in (0..W).step_by(12) {
            rect(&mut s, x, FLOOR + 1, 1, H - FLOOR - 1, "#1d1713");
        }
        // The cat, blinking now and then, with its paw raised to hold the glass.
        let (tx, ty) = (22, FLOOR - 30);
        s.push_str(&format!(
            "<g transform=\"translate({tx},{ty})\"><g class=\"look-open\">{}</g><g class=\"look-blink\">{}</g></g>",
            sitting_markup(false, Some(19)),
            sitting_markup(true, Some(19))
        ));
        // The glass, held out toward the loading window: a handle from the paw,
        // an amber rim, pale glass with a glint and the page seen larger in it.
        s.push_str("<g class=\"browse-glass\">");
        let (px, py) = (tx + 21, ty + 20);
        let (lx, ly) = (tx + 36, ty - 3);
        for step in 0..=14 {
            let x = px + step;
            let y = py - step * (py - ly - 4) / 14;
            rect(&mut s, x, y, 2, 2, "#a8826a");
        }
        let mut glass = String::new();
        ring(&mut glass, lx as f32 + 0.5, ly as f32 + 0.5, 0.0, 6.6, "#cfdcf5");
        s.push_str(&format!("<g opacity=\"0.4\">{glass}</g>"));
        ring(&mut s, lx as f32 + 0.5, ly as f32 + 0.5, 6.6, 8.6, "#d4b26a");
        rect(&mut s, lx - 3, ly - 3, 6, 5, "#0e1018");
        rect(&mut s, lx - 3, ly - 3, 6, 1, "#33466e");
        rect(&mut s, lx - 2, ly - 1, 4, 1, "#7d93c4");
        rect(&mut s, lx - 2, ly + 1, 3, 1, "#7d93c4");
        for (x, y) in [(lx - 6, ly - 4), (lx - 5, ly - 5), (lx - 6, ly - 3)] {
            rect(&mut s, x, y, 1, 1, "#ffffff");
        }
        s.push_str("<g class=\"sparkle\">");
        rect(&mut s, lx + 5, ly - 6, 1, 1, "#ffffff");
        rect(&mut s, lx + 4, ly - 5, 3, 1, "#ffffff");
        rect(&mut s, lx + 5, ly - 4, 1, 1, "#ffffff");
        s.push_str("</g></g>");
        s.push_str(&edge_cover());
        super::pixels::compact(&s)
    })
}

/// The cat going through the websites with a magnifying glass.
#[component]
pub fn BrowsingCat() -> Element {
    rsx! {
        svg {
            class: "scene-cat browse-cat", view_box: "-1 -1 {W + 2} {H + 2}",
            role: "img", "aria-label": "A cat looking at websites through a magnifying glass",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_holds_the_windows_the_cat_and_the_glass() {
        let m = markup();
        assert!(m.contains("browse-load") && m.contains("browse-glass") && m.contains("look-blink"));
    }

    #[test]
    fn the_edges_break_up_and_the_middle_stays_whole() {
        assert!((30..90).all(|x| (8..46).all(|y| shows(x, y))));
        assert!((0..W).all(|x| !shows(x, 0)), "the top edge is bare too");
        assert!((0..2).all(|x| (0..H).all(|y| !shows(x, y))));
        assert!((0..W).all(|x| !shows(x, H - 1)));
    }
}
