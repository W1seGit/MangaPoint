//! The mascot: the fur-coloured creature drawn by hand in `mangapoint-mc.ase`, and the calm little
//! scenes it stars in wherever the app has nothing to show yet or is waiting: asleep on an empty
//! shelf, looking through a magnifying glass, with a cup of tea, bobbing while something loads.
//!
//! The pixels are in `sprite.rs`. Everything here is SVG rectangles drawn from them, and the motion is
//! slow CSS (`art.css`): a one-pixel nod every few seconds, floating "z"s, drifting steam. It uses the
//! app's own palette, so the pictures sit in the pages as if they were part of them.

use super::sprite::{BODY, EYES_CLOSED, HALF, NECK};
use dioxus::prelude::*;
use std::fmt::Write;
use std::sync::OnceLock;

/// The colours of the drawing and of the little things around it.
const FUR: &str = "#aa8d7e";
const CHEEK: &str = "#3b2f2a";
const WOOD: &str = "#2a211d";
const DARK: &str = "#161210";
const LINE: &str = "#000000";

fn color(letter: char) -> &'static str {
    match letter {
        'k' => LINE,
        'f' => FUR,
        'c' => CHEEK,
        'd' => "#1e1916",
        'e' => DARK,
        _ => "none",
    }
}

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    let _ = write!(out, "<path fill=\"{fill}\" d=\"M{x} {y}h{w}v{h}h-{w}z\"/>");
}

/// Rows of colour letters (`from..to`), one path for each colour.
fn rows_markup(rows: &[&str], from: usize, to: usize) -> String {
    let mut out = String::new();
    for letter in ['k', 'f', 'c', 'd', 'e'] {
        let mut d = String::new();
        for (y, row) in rows.iter().enumerate().take(to).skip(from) {
            let bytes = row.as_bytes();
            let mut x = 0;
            while x < bytes.len() {
                if bytes[x] as char == letter {
                    let start = x;
                    while x < bytes.len() && bytes[x] as char == letter {
                        x += 1;
                    }
                    let _ = write!(d, "M{start} {y}h{}v1h-{}z", x - start, x - start);
                } else {
                    x += 1;
                }
            }
        }
        if !d.is_empty() {
            let _ = write!(out, "<path fill=\"{}\" d=\"{d}\"/>", color(letter));
        }
    }
    out
}

fn pixels_markup(pixels: &[(i32, i32)], fill: &str) -> String {
    let mut out = String::new();
    for &(x, y) in pixels {
        rect(&mut out, x, y, 1, 1, fill);
    }
    out
}

/// How the eyes are drawn: shut and smiling (asleep, content), or open with now and then a blink.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Eyes {
    Smiling,
    Open,
}

/// The two open eyes of the full-size drawing.
const EYES_OPEN: [(i32, i32, i32, i32); 2] = [(13, 15, 2, 3), (29, 15, 2, 3)];

fn eyes_markup(eyes: Eyes) -> String {
    let closed = pixels_markup(&EYES_CLOSED, LINE);
    match eyes {
        Eyes::Smiling => format!("<g class=\"m-eyes\">{closed}</g>"),
        Eyes::Open => {
            let mut open = String::new();
            for (x, y, w, h) in EYES_OPEN {
                rect(&mut open, x, y, w, h, LINE);
            }
            format!("<g class=\"m-eyes look-open\">{open}</g><g class=\"m-eyes look-blink\">{closed}</g>")
        }
    }
}

/// The mascot at `(x, y)`: the shoulders, and over them the head, which nods.
fn mascot(x: i32, y: i32, eyes: Eyes) -> String {
    format!(
        "<g transform=\"translate({x} {y})\"><g class=\"m-body\">{}</g><g class=\"m-head\">{}{}</g></g>",
        rows_markup(&BODY, NECK, BODY.len()),
        rows_markup(&BODY, 0, NECK),
        eyes_markup(eyes),
    )
}

/// The mascot at half the size, for the small scenes of the website's App page. Its eyes open and shut.
pub fn half_markup() -> String {
    let closed = pixels_markup(&[(6, 8), (7, 8), (14, 8), (15, 8)], LINE);
    let mut open = String::new();
    rect(&mut open, 6, 7, 1, 2, LINE);
    rect(&mut open, 15, 7, 1, 2, LINE);
    format!(
        "<g class=\"m-body\">{}</g><g class=\"m-head\">{}<g class=\"look-open\">{open}</g><g class=\"look-blink\">{closed}</g></g>",
        rows_markup(&HALF, 17, HALF.len()),
        rows_markup(&HALF, 0, 17),
    )
}

/// A letter z, five pixels across, for the sleeper's dream.
fn z(out: &mut String, x: i32, y: i32, class: &str) {
    let _ = write!(out, "<g class=\"{class}\">");
    rect(out, x, y, 5, 1, FUR);
    rect(out, x + 3, y + 1, 1, 1, FUR);
    rect(out, x + 2, y + 2, 1, 1, FUR);
    rect(out, x + 1, y + 3, 1, 1, FUR);
    rect(out, x, y + 4, 5, 1, FUR);
    out.push_str("</g>");
}

/// The pixels of a ring around (`cx`, `cy`) between two radii.
fn ring(out: &mut String, cx: f32, cy: f32, inner: f32, outer: f32, fill: &str) {
    for y in 0..(cy + outer + 1.0) as i32 {
        for x in 0..(cx + outer + 1.0) as i32 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d >= inner && d <= outer {
                rect(out, x, y, 1, 1, fill);
            }
        }
    }
}

/// A magnifying glass, its lens centred on (`cx`, `cy`) and its handle running down to the left.
fn glass(cx: i32, cy: i32) -> String {
    let mut s = String::new();
    // The handle first, so the rim is drawn over its end.
    for i in 0..9 {
        rect(&mut s, cx - 9 - i, cy + 9 + i, 3, 2, LINE);
        rect(&mut s, cx - 8 - i, cy + 9 + i, 1, 2, FUR);
    }
    ring(&mut s, cx as f32, cy as f32, 0.0, 9.0, "#3b2f2a");
    ring(&mut s, cx as f32, cy as f32, 8.0, 10.0, LINE);
    ring(&mut s, cx as f32, cy as f32, 7.0, 8.5, FUR);
    // A glint on the glass.
    rect(&mut s, cx - 5, cy - 4, 3, 1, FUR);
    rect(&mut s, cx - 5, cy - 3, 1, 2, FUR);
    s
}

fn cached(cell: &'static OnceLock<String>, make: fn() -> String) -> &'static str {
    cell.get_or_init(make)
}

// ---- the scenes ----------------------------------------------------------------------------------------

/// Asleep, for a page with nothing on it yet.
fn rest() -> String {
    let mut s = mascot(7, 6, Eyes::Smiling);
    z(&mut s, 52, 20, "m-z z1");
    z(&mut s, 47, 11, "m-z z2");
    z(&mut s, 55, 4, "m-z z3");
    s
}

/// Asleep behind the edge of an empty bookshelf, for a Library with nothing in it.
fn shelf() -> String {
    let mut s = mascot(22, 6, Eyes::Smiling);
    // The shelf: a board with the mascot's chest behind it, and the empty room below.
    rect(&mut s, 2, 52, 84, 2, FUR);
    rect(&mut s, 2, 54, 84, 4, CHEEK);
    rect(&mut s, 2, 58, 84, 14, DARK);
    rect(&mut s, 2, 58, 3, 14, WOOD);
    rect(&mut s, 83, 58, 3, 14, WOOD);
    rect(&mut s, 2, 71, 84, 3, WOOD);
    // One book, leaning on the board's end.
    rect(&mut s, 70, 40, 7, 12, CHEEK);
    rect(&mut s, 70, 40, 1, 12, LINE);
    rect(&mut s, 76, 40, 1, 12, LINE);
    rect(&mut s, 70, 40, 7, 1, LINE);
    rect(&mut s, 72, 43, 3, 1, FUR);
    rect(&mut s, 72, 45, 3, 1, FUR);
    z(&mut s, 68, 20, "m-z z1");
    z(&mut s, 74, 12, "m-z z2");
    s
}

/// Looking through a magnifying glass, for a search with nothing in it yet.
fn search() -> String {
    let mut s = mascot(4, 8, Eyes::Open);
    s.push_str(&format!("<g class=\"m-glass\">{}</g>", glass(66, 34)));
    s
}

/// With a cup of tea, for a page where the work has not begun.
fn waiting() -> String {
    let mut s = mascot(4, 8, Eyes::Smiling);
    // The cup: a black outline round a fur-coloured body with a darker rim, a handle, a saucer.
    rect(&mut s, 52, 42, 15, 15, LINE);
    rect(&mut s, 53, 43, 13, 13, FUR);
    rect(&mut s, 53, 43, 13, 2, CHEEK);
    rect(&mut s, 52, 55, 1, 2, "none");
    rect(&mut s, 66, 46, 5, 2, LINE);
    rect(&mut s, 70, 46, 2, 8, LINE);
    rect(&mut s, 66, 52, 5, 2, LINE);
    rect(&mut s, 49, 57, 21, 2, LINE);
    rect(&mut s, 50, 58, 19, 1, CHEEK);
    // Steam, rising and fading one after another.
    for (i, x) in [56, 60, 64].into_iter().enumerate() {
        let _ = write!(s, "<g class=\"m-steam s{i}\">");
        rect(&mut s, x, 34, 1, 3, FUR);
        rect(&mut s, x + 1, 37, 1, 3, FUR);
        s.push_str("</g>");
    }
    s
}

/// The mascot going on its way, with the ground slipping by, for the first start.
fn walking(day: bool) -> String {
    let mut s = String::new();
    // A light in the sky: the sun by day, the moon by night.
    if day {
        ring(&mut s, 77.0, 14.0, 0.0, 5.0, FUR);
    } else {
        ring(&mut s, 77.0, 14.0, 0.0, 5.0, FUR);
        ring(&mut s, 80.0, 12.0, 0.0, 4.5, "#161210");
    }
    s.push_str("<g class=\"m-ground\">");
    for i in 0..9 {
        rect(&mut s, i * 14 - 8, 62, 7, 1, CHEEK);
        rect(&mut s, i * 14 - 1, 64, 4, 1, WOOD);
    }
    s.push_str("</g>");
    s.push_str(&mascot(23, 4, Eyes::Open).replace("m-head", "m-head m-walk"));
    s
}

/// Going through websites with a magnifying glass, while the sources are looked at.
fn browsing() -> String {
    let mut s = mascot(4, 6, Eyes::Open);
    for (i, y) in [4, 22, 40].into_iter().enumerate() {
        let _ = write!(s, "<g class=\"m-window w{i}\">");
        rect(&mut s, 74, y, 44, 15, LINE);
        rect(&mut s, 75, y + 1, 42, 13, WOOD);
        rect(&mut s, 75, y + 1, 42, 3, CHEEK);
        rect(&mut s, 77, y + 2, 1, 1, FUR);
        rect(&mut s, 79, y + 2, 1, 1, FUR);
        rect(&mut s, 77, y + 6, 20, 1, CHEEK);
        rect(&mut s, 77, y + 9, 30, 1, CHEEK);
        s.push_str("<g class=\"m-lit\">");
        rect(&mut s, 77, y + 6, 20, 1, FUR);
        rect(&mut s, 77, y + 9, 30, 1, FUR);
        s.push_str("</g></g>");
    }
    s.push_str(&format!("<g class=\"m-glass\">{}</g>", glass(58, 34)));
    s
}

/// Next to a parcel with an arrow going down onto it, while the app is brought up to date.
fn updating() -> String {
    let mut s = mascot(4, 6, Eyes::Smiling);
    // The parcel.
    rect(&mut s, 50, 40, 26, 22, LINE);
    rect(&mut s, 51, 41, 24, 20, CHEEK);
    rect(&mut s, 61, 41, 4, 20, FUR);
    rect(&mut s, 51, 49, 24, 1, LINE);
    // The arrow that brings the new version.
    s.push_str("<g class=\"m-drop\">");
    rect(&mut s, 60, 8, 6, 10, FUR);
    rect(&mut s, 56, 18, 14, 2, FUR);
    rect(&mut s, 58, 20, 10, 2, FUR);
    rect(&mut s, 60, 22, 6, 2, FUR);
    rect(&mut s, 62, 24, 2, 2, FUR);
    s.push_str("</g>");
    s
}

// ---- the pieces for the pages ---------------------------------------------------------------------------

/// The mascot asleep, for screens with nothing on them yet.
#[component]
pub fn Resting() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene rest", view_box: "0 0 64 66", role: "img", "aria-label": "The mascot, asleep",
            dangerous_inner_html: "{cached(&M, rest)}",
        }
    }
}

/// The mascot asleep on an empty shelf, for a Library with nothing in it.
#[component]
pub fn ShelfScene() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene shelf", view_box: "0 0 88 76", role: "img", "aria-label": "The mascot, asleep beside an empty shelf",
            dangerous_inner_html: "{cached(&M, shelf)}",
        }
    }
}

/// The mascot with a magnifying glass.
#[component]
pub fn SearchScene() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene search", view_box: "0 0 84 72", role: "img", "aria-label": "The mascot looking through a magnifying glass",
            dangerous_inner_html: "{cached(&M, search)}",
        }
    }
}

/// The mascot with a cup of tea, waiting.
#[component]
pub fn WaitingScene() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene waiting", view_box: "0 0 76 72", role: "img", "aria-label": "The mascot, waiting with a cup of tea",
            dangerous_inner_html: "{cached(&M, waiting)}",
        }
    }
}

/// The mascot among the websites it is looking through, for the first set-up.
#[component]
pub fn BrowsingScene() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene browse", view_box: "0 0 122 66", role: "img", "aria-label": "The mascot looking through websites",
            dangerous_inner_html: "{cached(&M, browsing)}",
        }
    }
}

/// The mascot with a parcel, for whatever is being brought up to date.
#[component]
pub fn UpdateScene() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    rsx! {
        svg { class: "mascot scene update", view_box: "0 0 82 68", role: "img", "aria-label": "The mascot with a parcel",
            dangerous_inner_html: "{cached(&M, updating)}",
        }
    }
}

/// The mascot on its way, at night or (with `day`) in daylight, for the first start.
#[component]
pub fn Walking(#[props(default = false)] day: bool) -> Element {
    static DAY: OnceLock<String> = OnceLock::new();
    static NIGHT: OnceLock<String> = OnceLock::new();
    let markup = if day { cached(&DAY, || walking(true)) } else { cached(&NIGHT, || walking(false)) };
    rsx! {
        div { class: "walker",
            svg { class: "mascot scene walk", view_box: "0 0 90 68", role: "img", "aria-label": "The mascot walking",
                dangerous_inner_html: "{markup}",
            }
        }
    }
}

/// The mascot, resting calmly in a small place (a line of text, a box that is waiting).
#[component]
pub fn Idle() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    fn make() -> String {
        mascot(0, 0, Eyes::Open)
    }
    rsx! {
        svg { class: "mascot m-idle", view_box: "0 0 44 57", "aria-hidden": "true", dangerous_inner_html: "{cached(&M, make)}" }
    }
}

/// Something is loading: the mascot nods, with three dots coming and going under it. `size` is its width
/// in pixels.
#[component]
pub fn Loader(#[props(default = 120)] size: u32) -> Element {
    static M: OnceLock<String> = OnceLock::new();
    fn make() -> String {
        mascot(0, 0, Eyes::Open)
    }
    rsx! {
        span { class: "mloader", style: "--m: {size}px", role: "img", "aria-label": "Loading",
            svg { class: "mascot", view_box: "0 0 44 57", "aria-hidden": "true", dangerous_inner_html: "{cached(&M, make)}" }
            span { class: "mdots", span {} span {} span {} }
        }
    }
}

/// The mascot on a toast, with a mark for how it went: `look` is "success", "warning" or "error".
#[component]
pub fn ToastMascot(look: String) -> Element {
    static M: OnceLock<String> = OnceLock::new();
    fn make() -> String {
        half_markup()
    }
    let badge = match look.as_str() {
        "success" => "check",
        "warning" => "triangle-alert",
        _ => "x",
    };
    rsx! {
        span { class: "toast-mascot {look}",
            svg { class: "mascot", view_box: "0 0 22 29", "aria-hidden": "true", dangerous_inner_html: "{cached(&M, make)}" }
            span { class: "toast-badge", super::super::widgets::Icon { name: badge, size: 12 } }
        }
    }
}

// ---- the icon ---------------------------------------------------------------------------------------------

/// The app's icon as RGBA pixels: the mascot on a dark rounded square, `scale` screen pixels for each of the
/// drawing's 64 by 64.
pub fn icon_rgba(scale: usize) -> (Vec<u8>, u32, u32) {
    const GRID: usize = 64;
    // Where the drawing sits on its canvas (it was drawn there).
    const AT: (usize, usize) = (10, 7);
    let side = GRID * scale;
    let mut rgba = vec![0u8; side * side * 4];
    let radius = 14.0f32;
    let put = |rgba: &mut Vec<u8>, gx: usize, gy: usize, c: (u8, u8, u8)| {
        for dy in 0..scale {
            for dx in 0..scale {
                let i = ((gy * scale + dy) * side + gx * scale + dx) * 4;
                rgba[i..i + 4].copy_from_slice(&[c.0, c.1, c.2, 255]);
            }
        }
    };
    for gy in 0..GRID {
        for gx in 0..GRID {
            // The corners of the square are cut away.
            let (cx, cy) = (gx as f32 + 0.5, gy as f32 + 0.5);
            let (ex, ey) = ((radius - cx).max(cx - (GRID as f32 - radius)).max(0.0), (radius - cy).max(cy - (GRID as f32 - radius)).max(0.0));
            if ex > 0.0 && ey > 0.0 && (ex * ex + ey * ey).sqrt() > radius {
                continue;
            }
            put(&mut rgba, gx, gy, (0x1e, 0x19, 0x16));
        }
    }
    let hex = |c: &str| {
        let n = u32::from_str_radix(&c[1..], 16).unwrap_or(0);
        ((n >> 16) as u8, (n >> 8) as u8, n as u8)
    };
    for (y, row) in BODY.iter().enumerate() {
        for (x, letter) in row.chars().enumerate() {
            let c = color(letter);
            let (gx, gy) = (AT.0 + x, AT.1 + y);
            if c != "none" && gx < GRID && gy < GRID {
                put(&mut rgba, gx, gy, hex(c));
            }
        }
    }
    for &(x, y) in &EYES_CLOSED {
        put(&mut rgba, AT.0 + x as usize, AT.1 + y as usize, (0, 0, 0));
    }
    (rgba, side as u32, side as u32)
}

/// The scenes as markup, for the gallery test.
#[cfg(test)]
pub(super) fn rest_markup() -> String { rest() }
#[cfg(test)]
pub(super) fn shelf_markup() -> String { shelf() }
#[cfg(test)]
pub(super) fn search_markup() -> String { search() }
#[cfg(test)]
pub(super) fn waiting_markup() -> String { waiting() }
#[cfg(test)]
pub(super) fn browsing_markup() -> String { browsing() }
#[cfg(test)]
pub(super) fn updating_markup() -> String { updating() }
#[cfg(test)]
pub(super) fn walking_markup(day: bool) -> String { walking(day) }
#[cfg(test)]
pub(super) fn loader_markup() -> String { mascot(0, 0, Eyes::Open) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drawing_is_whole_and_in_the_apps_colours() {
        assert_eq!(BODY.len(), 57);
        assert!(BODY.iter().all(|r| r.len() == 44));
        assert!(BODY.iter().all(|r| r.chars().all(|c| ".kfcde".contains(c))));
        assert_eq!(HALF.len(), 29);
        assert!(HALF.iter().all(|r| r.len() == 22));
    }

    #[test]
    fn the_eyes_are_where_the_face_has_room_for_them() {
        for &(x, y) in &EYES_CLOSED {
            assert_eq!(BODY[y as usize].as_bytes()[x as usize] as char, 'f', "{x},{y} is fur once the eyes are taken out");
        }
        for (x, y, w, h) in EYES_OPEN {
            for dy in 0..h {
                for dx in 0..w {
                    assert_eq!(BODY[(y + dy) as usize].as_bytes()[(x + dx) as usize] as char, 'f');
                }
            }
        }
    }

    #[test]
    fn every_scene_is_drawn_and_has_its_motion() {
        for (name, svg, classes) in [
            ("rest", rest(), &["m-head", "m-z z1", "m-z z3"][..]),
            ("shelf", shelf(), &["m-head", "m-z z2"][..]),
            ("search", search(), &["m-glass", "look-blink"][..]),
            ("waiting", waiting(), &["m-steam s0", "m-steam s2"][..]),
            ("browsing", browsing(), &["m-window w2", "m-lit"][..]),
            ("updating", updating(), &["m-drop"][..]),
            ("walking", walking(true), &["m-ground", "m-walk"][..]),
        ] {
            for class in classes {
                assert!(svg.contains(&format!("class=\"{class}\"")) || svg.contains(class), "{name}: {class}");
            }
        }
    }

    #[test]
    fn the_icon_is_the_mascot_on_a_rounded_square() {
        let (rgba, w, h) = icon_rgba(4);
        assert_eq!((w, h), (256, 256));
        let alpha = |x: usize, y: usize| rgba[(y * 256 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0, "the corners are cut");
        assert_eq!(alpha(128, 4), 255, "the middle of the top edge is the square");
        let px = |x: usize, y: usize| &rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 3];
        assert_eq!(px(129, 69), &[0xaa, 0x8d, 0x7e], "the fur of the face");
    }
}
