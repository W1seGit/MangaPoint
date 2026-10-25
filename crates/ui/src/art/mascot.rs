//! The mascot: the fur-coloured creature drawn by hand in `mangapoint-mc.ase`, and the calm little
//! scenes it stars in wherever the app has nothing to show yet or is waiting: asleep on a bookcase,
//! holding up a magnifying glass, with a cup of tea, going round a circle while something loads.
//!
//! The pixels are in `sprite.rs`. Everything here is SVG rectangles drawn from them, and the motion is
//! slow CSS (`art.css`): a one-pixel nod every few seconds, drifting steam, a glass that tilts. It uses the
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
fn rows_markup<R: AsRef<str>>(rows: &[R], from: usize, to: usize) -> String {
    let mut out = String::new();
    for letter in ['k', 'f', 'c', 'd', 'e'] {
        let mut d = String::new();
        for (y, row) in rows.iter().enumerate().take(to).skip(from) {
            let bytes = row.as_ref().as_bytes();
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

/// A magnifying glass, its lens centred on (`cx`, `cy`) and its handle running down to the left, to about
/// (`cx - 16`, `cy + 18`).
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

// ---- arms -------------------------------------------------------------------------------------------------

/// Which of the arms that hang at the mascot's sides a scene draws reaching for something instead.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lift {
    Right,
    Both,
}

/// The drawing with the lifted arms taken out of it: where they hung is the belly's fur.
fn lifted(lift: Lift) -> Vec<String> {
    BODY.iter()
        .enumerate()
        .map(|(y, row)| {
            let mut row = row.as_bytes().to_vec();
            if y >= 46 {
                row[29..36].fill(b'f');
                if lift == Lift::Both {
                    row[8..15].fill(b'f');
                }
            }
            String::from_utf8(row).unwrap_or_default()
        })
        .collect()
}

/// An arm from the shoulder `a` to the paw `b`, in the drawing's pixels: fur in a black outline, ending in
/// a dark paw like the ones the mascot sits on. The outline stops at the shoulder, so the arm grows out of
/// the body.
fn arm(a: (f32, f32), b: (f32, f32)) -> String {
    const WIDTH: f32 = 2.6;
    const PAW: f32 = 3.2;
    let dist = |p: (f32, f32), q: (f32, f32)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    let to_arm = |p: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        dist(p, (a.0 + t * dx, a.1 + t * dy))
    };
    let (mut line, mut fur, mut paw) = (Vec::new(), Vec::new(), Vec::new());
    for y in (a.1.min(b.1) - 6.0) as i32..(a.1.max(b.1) + 6.0) as i32 {
        for x in (a.0.min(b.0) - 6.0) as i32..(a.0.max(b.0) + 6.0) as i32 {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let (d, dp) = (to_arm(p), dist(p, b));
            if dp <= PAW {
                paw.push((x, y));
            } else if d <= WIDTH {
                fur.push((x, y));
            } else if (d <= WIDTH + 1.0 || dp <= PAW + 1.0) && dist(p, a) > WIDTH + 2.5 {
                line.push((x, y));
            }
        }
    }
    format!("{}{}{}", pixels_markup(&line, LINE), pixels_markup(&fur, FUR), pixels_markup(&paw, CHEEK))
}

/// An arm, from the shoulder to the paw, in the drawing's pixels.
type Reach = ((f32, f32), (f32, f32));

/// The mascot at (`x`, `y`) with its `lift`ed arms reaching from the shoulder to the paws in `arms` (in the
/// drawing's pixels). `props` are drawn between the mascot and its arms, so the paws close over them.
fn posed(x: i32, y: i32, eyes: Eyes, lift: Lift, arms: &[Reach], props: &str) -> String {
    let rows = lifted(lift);
    let reach: String = arms.iter().map(|&(a, b)| arm(a, b)).collect();
    format!(
        "<g transform=\"translate({x} {y})\"><g class=\"m-body\">{}</g><g class=\"m-head\">{}{}</g></g>{props}<g transform=\"translate({x} {y})\">{reach}</g>",
        rows_markup(&rows, NECK, rows.len()),
        rows_markup(&rows, 0, NECK),
        eyes_markup(eyes),
    )
}

/// The right shoulder, where a lifted right arm starts.
const RIGHT_SHOULDER: (f32, f32) = (31.5, 44.0);

/// The magnifying glass held up in the right paw, at (`px`, `py`) in the drawing's pixels. It tilts a
/// little about the paw.
fn holding_glass(x: i32, y: i32, px: f32, py: f32, eyes: Eyes, before: &str) -> String {
    let (gx, gy) = (x as f32 + px, y as f32 + py);
    let lens = glass(gx as i32 + 16, gy as i32 - 18);
    let props = format!("{before}<g class=\"m-glass\" style=\"transform-origin: {gx}px {gy}px\">{lens}</g>");
    posed(x, y, eyes, Lift::Right, &[(RIGHT_SHOULDER, (px, py))], &props)
}

fn cached(cell: &'static OnceLock<String>, make: fn() -> String) -> &'static str {
    cell.get_or_init(make)
}

// ---- the scenes ----------------------------------------------------------------------------------------

/// Asleep, for a page with nothing on it yet.
fn rest() -> String {
    mascot(10, 6, Eyes::Smiling)
}

/// Asleep with its paws on the top of a bookcase, for a Library with nothing in it.
fn shelf() -> String {
    // The bookcase: its top, with the mascot's chest behind it, and the shelf below.
    let mut case = String::new();
    rect(&mut case, 2, 52, 84, 2, FUR);
    rect(&mut case, 2, 54, 84, 4, CHEEK);
    rect(&mut case, 2, 58, 84, 14, DARK);
    rect(&mut case, 2, 58, 3, 14, WOOD);
    rect(&mut case, 83, 58, 3, 14, WOOD);
    rect(&mut case, 2, 71, 84, 3, WOOD);
    // The books: (left, width, height), standing on the bottom board, dark and fur by turns, each with a
    // band across its spine.
    let books = [
        (6, 4, 11), (10, 3, 9), (13, 5, 12), (18, 3, 10), (21, 4, 11),
        (29, 3, 12), (32, 5, 10), (37, 4, 12), (41, 3, 9),
        (49, 4, 11), (53, 3, 12), (56, 5, 9), (61, 4, 11), (65, 3, 10),
        (72, 4, 12), (76, 3, 10), (79, 4, 11),
    ];
    for (i, (x, w, h)) in books.into_iter().enumerate() {
        let (spine, band) = if i % 2 == 0 { (CHEEK, FUR) } else { (FUR, CHEEK) };
        rect(&mut case, x, 71 - h, w, h, spine);
        rect(&mut case, x, 71 - h, 1, h, LINE);
        rect(&mut case, x + 1, 71 - h + 2, w - 1, 1, band);
    }
    let arms = [((10.5, 42.0), (10.5, 46.5)), ((33.5, 42.0), (33.5, 46.5))];
    posed(22, 6, Eyes::Smiling, Lift::Both, &arms, &case)
}

/// Holding up a magnifying glass, for a search with nothing in it yet.
fn search() -> String {
    holding_glass(4, 8, 47.0, 36.0, Eyes::Open, "")
}

/// Holding up a cup of tea, for a page where the work has not begun.
fn waiting() -> String {
    // The cup: a black outline round a fur-coloured body with the tea at its brim, and a handle.
    let mut cup = String::new();
    rect(&mut cup, 52, 40, 15, 15, LINE);
    rect(&mut cup, 53, 41, 13, 13, FUR);
    rect(&mut cup, 53, 41, 13, 2, CHEEK);
    rect(&mut cup, 66, 44, 5, 2, LINE);
    rect(&mut cup, 70, 44, 2, 8, LINE);
    rect(&mut cup, 66, 50, 5, 2, LINE);
    // Steam, rising and fading one after another.
    for (i, x) in [56, 59, 62].into_iter().enumerate() {
        let _ = write!(cup, "<g class=\"m-steam s{i}\">");
        rect(&mut cup, x, 30, 1, 3, FUR);
        rect(&mut cup, x + 1, 33, 1, 3, FUR);
        cup.push_str("</g>");
    }
    posed(4, 8, Eyes::Smiling, Lift::Right, &[(RIGHT_SHOULDER, (49.0, 40.0))], &cup)
}

/// The mascot going on its way, with the ground slipping by, for the first start.
fn walking() -> String {
    let mut s = String::from("<g class=\"m-ground\">");
    for i in 0..9 {
        rect(&mut s, i * 14 - 8, 62, 7, 1, CHEEK);
        rect(&mut s, i * 14 - 1, 64, 4, 1, WOOD);
    }
    s.push_str("</g>");
    s.push_str(&mascot(23, 4, Eyes::Open).replace("m-head", "m-head m-walk"));
    s
}

/// Holding a magnifying glass up to the websites it goes through, while the sources are looked at.
fn browsing() -> String {
    let mut windows = String::new();
    for (i, y) in [4, 22, 40].into_iter().enumerate() {
        let _ = write!(windows, "<g class=\"m-window w{i}\">");
        rect(&mut windows, 78, y, 44, 15, LINE);
        rect(&mut windows, 79, y + 1, 42, 13, WOOD);
        rect(&mut windows, 79, y + 1, 42, 3, CHEEK);
        rect(&mut windows, 81, y + 2, 1, 1, FUR);
        rect(&mut windows, 83, y + 2, 1, 1, FUR);
        rect(&mut windows, 81, y + 6, 20, 1, CHEEK);
        rect(&mut windows, 81, y + 9, 30, 1, CHEEK);
        windows.push_str("<g class=\"m-lit\">");
        rect(&mut windows, 81, y + 6, 20, 1, FUR);
        rect(&mut windows, 81, y + 9, 30, 1, FUR);
        windows.push_str("</g></g>");
    }
    holding_glass(4, 6, 45.0, 37.0, Eyes::Open, &windows)
}

/// With a paw on a parcel and an arrow going down onto it, while the app is brought up to date.
fn updating() -> String {
    // The parcel.
    let mut parcel = String::new();
    rect(&mut parcel, 50, 40, 26, 22, LINE);
    rect(&mut parcel, 51, 41, 24, 20, CHEEK);
    rect(&mut parcel, 61, 41, 4, 20, FUR);
    rect(&mut parcel, 51, 49, 24, 1, LINE);
    let mut s = posed(4, 6, Eyes::Smiling, Lift::Right, &[(RIGHT_SHOULDER, (48.0, 33.5))], &parcel);
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
        svg { class: "mascot scene shelf", view_box: "0 0 88 76", role: "img", "aria-label": "The mascot, asleep on a bookcase",
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
        svg { class: "mascot scene browse", view_box: "0 0 126 66", role: "img", "aria-label": "The mascot looking through websites",
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

/// The mascot on its way, for the first start.
#[component]
pub fn Walking() -> Element {
    static M: OnceLock<String> = OnceLock::new();
    let markup = cached(&M, walking);
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

/// Something is loading: the mascot goes round and round a dotted circle, staying upright. `size` is the
/// width of the whole in pixels.
#[component]
pub fn Loader(#[props(default = 120)] size: u32) -> Element {
    static M: OnceLock<String> = OnceLock::new();
    fn make() -> String {
        mascot(0, 0, Eyes::Open)
    }
    rsx! {
        span { class: "mloader", style: "--m: {size}px", role: "img", "aria-label": "Loading",
            span { class: "m-orbit",
                svg { class: "mascot", view_box: "0 0 44 57", "aria-hidden": "true", dangerous_inner_html: "{cached(&M, make)}" }
            }
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
pub(super) fn walking_markup() -> String { walking() }
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
            ("rest", rest(), &["m-head", "m-eyes"][..]),
            ("shelf", shelf(), &["m-head", "m-body"][..]),
            ("search", search(), &["m-glass", "look-blink"][..]),
            ("waiting", waiting(), &["m-steam s0", "m-steam s2"][..]),
            ("browsing", browsing(), &["m-window w2", "m-lit"][..]),
            ("updating", updating(), &["m-drop"][..]),
            ("walking", walking(), &["m-ground", "m-walk"][..]),
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
