//! The logo: a small gray pixel-art cat that licks its paw.
//!
//! An original 32x32 sprite drawn in code. Five frames are built once as SVG
//! rectangles; the browser plays the animation from CSS keyframes (see
//! `style.css`), so it costs almost nothing while it runs.

use dioxus::prelude::*;
use std::sync::OnceLock;

const SIZE: usize = 32;
type Grid = [[u8; SIZE]; SIZE];

const CLEAR: u8 = b'.';
const BODY: u8 = b'g';
const SHADE: u8 = b's';
const LIGHT: u8 = b'l';
const PINK: u8 = b'p';
const EYE: u8 = b'e';

/// Palette: gray accent body, darker shade, light chest, pink nose/tongue.
fn color(c: u8) -> &'static str {
    match c {
        BODY => "#b4b4b4",
        SHADE => "#707070",
        LIGHT => "#e8e8e8",
        PINK => "#f0a3b3",
        EYE => "#050505",
        b'o' => "#7fd18b",
        b'w' => "#e3b341",
        b'x' => "#f08585",
        b'd' => "#1a1a1a",
        b'h' => "#f5f5f5",
        b't' => "#8fb8e8",
        _ => "none",
    }
}

fn px(g: &mut Grid, x: usize, y: usize, c: u8) {
    g[y][x] = c;
}

/// Fill an inclusive rectangle.
fn rect(g: &mut Grid, x0: usize, y0: usize, x1: usize, y1: usize, c: u8) {
    for row in g.iter_mut().take(y1 + 1).skip(y0) {
        for cell in row.iter_mut().take(x1 + 1).skip(x0) {
            *cell = c;
        }
    }
}

/// What differs between frames.
#[derive(Clone, Copy)]
struct Pose {
    eyes_closed: bool,
    /// Top row of the raised paw, or `None` when both paws rest.
    paw_top: Option<usize>,
    /// 0 = no tongue, 1 = tip, 2 = out.
    tongue: u8,
}

const POSES: [Pose; 5] = [
    // 0: sitting, eyes open
    Pose {
        eyes_closed: false,
        paw_top: None,
        tongue: 0,
    },
    // 1: lifting the paw
    Pose {
        eyes_closed: false,
        paw_top: Some(19),
        tongue: 0,
    },
    // 2: licking, tongue tip
    Pose {
        eyes_closed: true,
        paw_top: Some(16),
        tongue: 1,
    },
    // 3: licking, tongue out, paw bobs down a pixel
    Pose {
        eyes_closed: true,
        paw_top: Some(17),
        tongue: 2,
    },
    // 4: blink
    Pose {
        eyes_closed: true,
        paw_top: None,
        tongue: 0,
    },
];

/// The tail, drawn separately so it can sway on its own.
fn tail() -> Grid {
    let mut g = [[CLEAR; SIZE]; SIZE];
    rect(&mut g, 23, 27, 26, 28, SHADE);
    rect(&mut g, 26, 24, 27, 26, SHADE);
    rect(&mut g, 25, 22, 26, 23, SHADE);
    g
}

fn frame(pose: Pose) -> Grid {
    let mut g = [[CLEAR; SIZE]; SIZE];

    // Ears (left, then mirrored), with pink insides.
    for (y, from, to) in [(4, 8, 8), (5, 8, 9), (6, 8, 10), (7, 8, 11), (8, 8, 12)] {
        rect(&mut g, from, y, to, y, BODY);
        rect(&mut g, 31 - to, y, 31 - from, y, BODY);
    }
    for (x, y) in [(9, 6), (9, 7), (10, 7)] {
        px(&mut g, x, y, PINK);
        px(&mut g, 31 - x, y, PINK);
    }

    // Head, rounded at the corners.
    rect(&mut g, 9, 9, 22, 9, BODY);
    rect(&mut g, 8, 10, 23, 17, BODY);
    rect(&mut g, 9, 18, 22, 18, BODY);
    // Forehead stripes.
    px(&mut g, 12, 10, SHADE);
    px(&mut g, 19, 10, SHADE);
    rect(&mut g, 15, 10, 16, 11, SHADE);

    // Body.
    rect(&mut g, 10, 19, 21, 19, BODY);
    rect(&mut g, 9, 20, 22, 27, BODY);
    rect(&mut g, 10, 28, 21, 28, BODY);
    rect(&mut g, 11, 29, 20, 29, BODY);
    rect(&mut g, 13, 20, 18, 27, LIGHT);
    for (x, y) in [(10, 22), (10, 25), (21, 22), (21, 25)] {
        px(&mut g, x, y, SHADE);
    }

    // Resting paws.
    rect(&mut g, 11, 27, 14, 30, LIGHT);
    rect(&mut g, 17, 27, 20, 30, LIGHT);
    px(&mut g, 15, 29, SHADE);
    px(&mut g, 16, 29, SHADE);

    // Muzzle and nose.
    rect(&mut g, 13, 14, 18, 17, LIGHT);
    rect(&mut g, 15, 14, 16, 14, PINK);

    // Eyes.
    if pose.eyes_closed {
        rect(&mut g, 11, 13, 12, 13, EYE);
        rect(&mut g, 19, 13, 20, 13, EYE);
        px(&mut g, 10, 14, EYE);
        px(&mut g, 21, 14, EYE);
    } else {
        rect(&mut g, 11, 12, 12, 14, EYE);
        rect(&mut g, 19, 12, 20, 14, EYE);
        px(&mut g, 12, 12, LIGHT);
        px(&mut g, 20, 12, LIGHT);
    }

    // Raised paw: the right paw leaves the ground and comes up to the mouth.
    if let Some(top) = pose.paw_top {
        rect(&mut g, 17, 27, 20, 30, CLEAR);
        rect(&mut g, 17, 27, 20, 29, LIGHT);
        // Arm from the body up to the paw.
        rect(&mut g, 19, top + 3, 21, 26, BODY);
        // Paw with a pink pad.
        rect(&mut g, 18, top, 21, top + 3, LIGHT);
        rect(&mut g, 19, top + 2, 20, top + 2, PINK);
    }

    // Tongue, reaching toward the paw.
    match pose.tongue {
        1 => px(&mut g, 17, 17, PINK),
        2 => {
            px(&mut g, 17, 17, PINK);
            px(&mut g, 17, 18, PINK);
        }
        _ => {}
    }
    g
}

/// The resting pose: a sleeping loaf, head on its paws. `breathe` lifts the
/// back one pixel so alternating the two frames looks like breathing.
fn rest_frame(breathe: bool) -> Grid {
    let mut g = [[CLEAR; SIZE]; SIZE];

    // Tail along the bottom and up the right side.
    rect(&mut g, 14, 29, 29, 29, SHADE);
    rect(&mut g, 28, 21, 29, 28, SHADE);

    // Body.
    let top = if breathe { 18 } else { 19 };
    rect(&mut g, 8, top, 25, top, BODY);
    rect(&mut g, 6, top + 1, 27, 27, BODY);
    rect(&mut g, 8, 28, 25, 28, BODY);
    for x in [13, 17, 21] {
        rect(&mut g, x, top, x, top + 2, SHADE);
    }

    // Ears (left, then mirrored across the head).
    for (y, from, to) in [(12, 3, 3), (13, 3, 4), (14, 3, 5), (15, 3, 6)] {
        rect(&mut g, from, y, to, y, BODY);
        rect(&mut g, 15 - to, y, 15 - from, y, BODY);
    }
    px(&mut g, 4, 14, PINK);
    px(&mut g, 11, 14, PINK);

    // Head, rounded.
    rect(&mut g, 4, 16, 11, 16, BODY);
    rect(&mut g, 3, 17, 12, 24, BODY);
    rect(&mut g, 4, 25, 11, 25, BODY);
    px(&mut g, 7, 17, SHADE);
    px(&mut g, 8, 17, SHADE);

    // Sleeping face.
    rect(&mut g, 5, 20, 6, 20, EYE);
    rect(&mut g, 9, 20, 10, 20, EYE);
    rect(&mut g, 6, 22, 9, 23, LIGHT);
    rect(&mut g, 7, 22, 8, 22, PINK);

    // Paws under the chin.
    rect(&mut g, 4, 26, 13, 28, LIGHT);
    px(&mut g, 8, 27, SHADE);
    g
}

/// A small pixel "z" of the given size (4 or 5 pixels square).
fn z_rects(x: usize, y: usize, size: usize) -> String {
    let rows: &[&str] = if size >= 5 {
        &["#####", "...#.", "..#..", ".#...", "#####"]
    } else {
        &["####", "..#.", ".#..", "####"]
    };
    let mut g = [[CLEAR; SIZE]; SIZE];
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch == '#' {
                px(&mut g, x + dx, y + dy, LIGHT);
            }
        }
    }
    rects(&g)
}

fn rest_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        format!(
            "<g class=\"r r0\">{}</g><g class=\"r r1\">{}</g>\
             <g class=\"z z1\">{}</g><g class=\"z z2\">{}</g>",
            rects(&rest_frame(false)),
            rects(&rest_frame(true)),
            z_rects(15, 8, 4),
            z_rects(20, 2, 5),
        )
    })
}

/// The sleeping sprite as rectangles in its own 32x32 space, for scenes that
/// place it somewhere (see `shelf.rs`).
pub(super) fn rest_frame_markup(breathe: bool) -> String {
    rects(&rest_frame(breathe))
}

/// The sitting cat, eyes open or shut, with the right paw raised to `paw_top`
/// (or resting), as rectangles in its own 32x32 space.
pub fn sitting_markup(eyes_closed: bool, paw_top: Option<usize>) -> String {
    format!(
        "<g class=\"tail\">{}</g>{}",
        rects(&tail()),
        rects(&frame(Pose {
            eyes_closed,
            paw_top,
            tongue: 0,
        }))
    )
}

/// The sitting cat (the logo) without its tail, eyes open or shut, as
/// rectangles in its own 32x32 space.
pub(super) fn body_markup(eyes_closed: bool) -> String {
    rects(&frame(Pose {
        eyes_closed,
        paw_top: None,
        tongue: 0,
    }))
}

/// Every pixel of the sitting mascot (tail included) as (x, y, colour), for
/// pictures that draw it themselves.
pub(super) fn mascot_pixels() -> Vec<(usize, usize, &'static str)> {
    let mut out = Vec::new();
    for layer in [tail(), frame(POSES[0])] {
        for (y, row) in layer.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                if c != CLEAR {
                    out.push((x, y, color(c)));
                }
            }
        }
    }
    out
}

/// The cat seen from above, running with its nose up: 12 pixels wide, 20 tall.
/// The two frames swap legs (diagonal pairs) and swing the tail.
fn runner_frame(step: bool) -> String {
    const W: usize = 12;
    const H: usize = 20;
    let mut g = [[CLEAR; W]; H];
    let head = [
        "..gg....gg..",
        "..gggggggg..",
        ".gggggggggg.",
        ".ggeggggegg.",
        ".ggggppgggg.",
        "..gggggggg..",
        "...gggggg...",
        "..ggsggsgg..",
        "..gggggggg..",
        "..ggsggsgg..",
        "..gggggggg..",
        "..ggsggsgg..",
        "...gggggg...",
        "....gggg....",
    ];
    for (y, row) in head.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            g[y][x] = if c == b'.' { CLEAR } else { c };
        }
    }
    // Each paw is 2x2; a diagonal pair reaches forward while the other pushes back.
    let (fl, fr, rl, rr) = if step { (5, 8, 12, 9) } else { (8, 5, 9, 12) };
    for (x, y) in [(0, fl), (10, fr), (0, rl), (10, rr)] {
        for dy in 0..2 {
            for dx in 0..2 {
                g[y + dy][x + dx] = LIGHT;
            }
        }
    }
    // The tail trails behind and swings from side to side.
    for (y, row) in g.iter_mut().enumerate().skip(14) {
        let x = match (y >= 18, step) {
            (false, _) => 5,
            (true, true) => 6,
            (true, false) => 4,
        };
        row[x] = SHADE;
        row[x + 1] = SHADE;
    }
    let mut out = String::new();
    for (y, row) in g.iter().enumerate() {
        let mut x = 0;
        while x < W {
            let c = row[x];
            let mut end = x;
            while end + 1 < W && row[end + 1] == c {
                end += 1;
            }
            if c != CLEAR {
                out.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{}\"/>",
                    end - x + 1,
                    color(c)
                ));
            }
            x = end + 1;
        }
    }
    out
}

fn runner_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        format!(
            "<g class=\"a\">{}</g><g class=\"b\">{}</g>",
            runner_frame(true),
            runner_frame(false)
        )
    })
}

/// The cat seen from above, running in circles, for whatever is loading.
/// `size` is the width of the whole circle in pixels.
#[component]
pub fn CirclingCat(#[props(default = 120)] size: u32) -> Element {
    rsx! {
        span { class: "orbit", style: "--orbit: {size}px", role: "img", "aria-label": "Loading",
            svg {
                class: "orbit-cat",
                view_box: "0 0 12 20",
                "aria-hidden": "true",
                dangerous_inner_html: "{runner_markup()}",
            }
        }
    }
}

/// The cat asleep, for screens with nothing on them yet.
#[component]
pub fn RestingCat() -> Element {
    rsx! {
        svg {
            class: "cat rest",
            width: "32",
            height: "32",
            view_box: "0 0 32 32",
            "aria-hidden": "true",
            dangerous_inner_html: "{rest_markup()}",
        }
    }
}

/// The idle cat as an RGBA image for the window icon: `scale` screen pixels
/// per sprite pixel, transparent background.
pub fn icon_rgba(scale: usize) -> (Vec<u8>, u32, u32) {
    let size = SIZE * scale;
    let mut rgba = vec![0u8; size * size * 4];
    let layers = [tail(), frame(POSES[0])];
    for layer in &layers {
        for (y, row) in layer.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                if c == CLEAR {
                    continue;
                }
                let hex = color(c).trim_start_matches('#');
                let v = u32::from_str_radix(hex, 16).unwrap_or(0);
                let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
                for dy in 0..scale {
                    for dx in 0..scale {
                        let i = ((y * scale + dy) * size + x * scale + dx) * 4;
                        rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
                    }
                }
            }
        }
    }
    (rgba, size as u32, size as u32)
}

/// One `<rect>` per horizontal run of equal color.
fn rects(g: &Grid) -> String {
    let mut out = String::new();
    for (y, row) in g.iter().enumerate() {
        let mut x = 0;
        while x < SIZE {
            let c = row[x];
            let mut end = x;
            while end + 1 < SIZE && row[end + 1] == c {
                end += 1;
            }
            if c != CLEAR {
                out.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{}\"/>",
                    end - x + 1,
                    color(c)
                ));
            }
            x = end + 1;
        }
    }
    out
}

/// The cat for a toast: pleased with a green tick when it worked, wary with an
/// amber warning sign, and teary with a red cross when it went wrong. The same
/// cat as the logo, with a badge by its side.
pub(super) fn toast_markup(look: &str) -> &'static str {
    static MARKUPS: OnceLock<[String; 3]> = OnceLock::new();
    let all = MARKUPS.get_or_init(|| {
        let make = |closed: bool, badge: fn(&mut Grid), tears: bool| {
            let mut g: Grid = [[CLEAR; SIZE]; SIZE];
            badge(&mut g);
            if tears {
                for (x, y) in [(11, 15), (11, 16), (20, 15)] {
                    px(&mut g, x, y, b't');
                }
            }
            super::pixels::compact(&format!("{}{}", body_markup(closed), rects(&g)))
        };
        [
            make(true, tick_badge, false),
            make(false, warning_badge, false),
            make(true, cross_badge, true),
        ]
    });
    match look {
        "success" => &all[0],
        "warning" => &all[1],
        _ => &all[2],
    }
}

/// A badge in the bottom right corner, 9 by 9 pixels from (22, 21).
fn badge(g: &mut Grid, set: impl Fn(i32, i32) -> Option<u8>) {
    for by in 0..9 {
        for bx in 0..9 {
            if let Some(c) = set(bx, by) {
                px(g, 22 + bx as usize, 21 + by as usize, c);
            }
        }
    }
}

fn disk(bx: i32, by: i32) -> bool {
    let (dx, dy) = (bx as f32 - 4.0, by as f32 - 4.0);
    dx * dx + dy * dy <= 4.6 * 4.6
}

fn tick_badge(g: &mut Grid) {
    const TICK: [(i32, i32); 6] = [(2, 4), (3, 5), (4, 6), (5, 5), (6, 4), (7, 3)];
    badge(g, |x, y| {
        if TICK.contains(&(x, y)) || TICK.contains(&(x, y - 1)) && x < 6 {
            Some(b'd')
        } else {
            disk(x, y).then_some(b'o')
        }
    });
}

fn warning_badge(g: &mut Grid) {
    badge(g, |x, y| {
        let half = y / 2;
        if (x - 4).abs() > half {
            return None;
        }
        // The exclamation mark.
        if x == 4 && ((3..=5).contains(&y) || y == 7) {
            return Some(b'd');
        }
        Some(b'w')
    });
}

fn cross_badge(g: &mut Grid) {
    badge(g, |x, y| {
        if (x == y || x + y == 8) && (2..=6).contains(&x) {
            return Some(b'h');
        }
        disk(x, y).then_some(b'x')
    });
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let mut s = format!("<g class=\"tail\">{}</g>", rects(&tail()));
        for (i, pose) in POSES.iter().enumerate() {
            s.push_str(&format!("<g class=\"f f{i}\">{}</g>", rects(&frame(*pose))));
        }
        s
    })
}

/// The cat on a toast: `look` is "success", "warning" or "error".
#[component]
pub fn ToastCat(look: String) -> Element {
    rsx! {
        svg {
            class: "toast-cat",
            width: "52",
            height: "56",
            view_box: "6 3 26 28",
            "aria-hidden": "true",
            dangerous_inner_html: "{toast_markup(&look)}",
        }
    }
}

#[component]
pub fn Cat() -> Element {
    rsx! {
        svg {
            class: "cat",
            width: "32",
            height: "32",
            view_box: "0 0 32 32",
            "aria-hidden": "true",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pose_is_drawn_and_stays_on_the_canvas() {
        let m = markup();
        for i in 0..POSES.len() {
            assert!(m.contains(&format!("class=\"f f{i}\"")), "frame {i}");
        }
        assert!(m.contains("class=\"tail\""));
        // Every rectangle lies inside the 32x32 canvas.
        for part in m.split("<rect ").skip(1) {
            let num = |key: &str| -> usize {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x + w <= SIZE && y < SIZE, "{part}");
        }
    }

    #[test]
    fn the_resting_cat_breathes_and_dreams() {
        let m = rest_markup();
        for class in ["r r0", "r r1", "z z1", "z z2"] {
            assert!(m.contains(&format!("class=\"{class}\"")), "{class}");
        }
        assert_ne!(
            rest_frame(false),
            rest_frame(true),
            "the two breaths differ"
        );
        // Sleeping: no open-eye highlight, and nothing outside the canvas.
        for part in m.split("<rect ").skip(1) {
            let num = |key: &str| -> usize {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            assert!(num("x=\"") + num("width=\"") <= SIZE && num("y=\"") < SIZE);
        }
    }

    #[test]
    fn the_window_icon_is_the_cat_on_a_transparent_background() {
        let (rgba, w, h) = icon_rgba(4);
        assert_eq!((w, h), (128, 128));
        assert_eq!(rgba.len(), 128 * 128 * 4);
        let alpha = |x: usize, y: usize| rgba[(y * 128 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0, "corners are transparent");
        // Something solid where the head is (sprite pixel 15,12 -> 60..63).
        assert_eq!(alpha(62, 50), 255);
        let opaque = rgba.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 1000 && opaque < 128 * 128 / 2, "{opaque}");
    }

    #[test]
    fn the_frames_actually_differ() {
        let frames: Vec<Grid> = POSES.iter().map(|p| frame(*p)).collect();
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "frames {i} and {j} are identical");
            }
        }
    }

    #[test]
    fn licking_poses_show_the_tongue_and_closed_eyes() {
        let tongue_pixels = |g: &Grid| g.iter().flatten().filter(|&&c| c == PINK).count();
        let rest = tongue_pixels(&frame(POSES[0]));
        assert!(tongue_pixels(&frame(POSES[2])) > rest);
        assert!(tongue_pixels(&frame(POSES[3])) > tongue_pixels(&frame(POSES[2])));
        // Eyes closed: the open-eye highlight pixel is gone.
        assert_eq!(frame(POSES[2])[12][12], BODY);
        assert_eq!(frame(POSES[0])[12][12], LIGHT);
    }
}
