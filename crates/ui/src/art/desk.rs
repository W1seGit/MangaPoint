//! Pixel pictures for the App part of Settings: one small scene for each thing
//! the desktop app does, starring the gray cat (the same sprite as everywhere
//! else), and the marks of macOS, Windows, Linux and GitHub drawn as pixels.
//! Original art drawn in code as SVG rectangles; the motion is CSS (see
//! `web.css`), in the same steps as the cat's own.

use super::cat::sitting_markup;
use dioxus::prelude::*;
use std::sync::OnceLock;

/// Every scene is this many pixels across and down.
const W: i32 = 56;
const H: i32 = 32;

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    out.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>"));
}

/// The colours of the drawings below, by the letter that stands for them.
fn color(c: char) -> Option<&'static str> {
    Some(match c {
        'k' => "#151515",
        'd' => "#2b2b2b",
        'm' => "#4a4a4a",
        's' => "#707070",
        'g' => "#b4b4b4",
        'l' => "#d6d6d6",
        'w' => "#f5f5f5",
        'a' => "#d4b26a",
        'y' => "#e3b341",
        'o' => "#e8963a",
        'r' => "#f08585",
        'p' => "#f0a3b3",
        'G' => "#7fd18b",
        'b' => "#8fb8e8",
        'B' => "#3a8fd9",
        'c' => "#cfdcf5",
        'n' => "#a8826a",
        'v' => "#b39ddb",
        _ => return None,
    })
}

/// A drawing written as rows of letters (see [`color`]; anything else is see-through).
fn sprite(out: &mut String, x: i32, y: i32, rows: &[&str]) {
    for (dy, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        let mut dx = 0;
        while dx < chars.len() {
            let Some(fill) = color(chars[dx]) else {
                dx += 1;
                continue;
            };
            // A run of the same colour is one rectangle.
            let start = dx;
            while dx < chars.len() && chars[dx] == chars[start] {
                dx += 1;
            }
            rect(out, x + start as i32, y + dy as i32, (dx - start) as i32, 1, fill);
        }
    }
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

/// The cat sitting at the left, blinking now and then, `dx` pixels to the left of its own place.
fn cat(paw_top: Option<usize>, dx: i32) -> String {
    format!(
        "<g transform=\"translate({},0)\"><g class=\"look-open\">{}</g><g class=\"look-blink\">{}</g></g>",
        -dx,
        sitting_markup(false, paw_top),
        sitting_markup(true, paw_top)
    )
}

/// An e-reader: a dark frame around a pale screen, with a few rows of a manga page on it.
fn ereader(out: &mut String, x: i32, y: i32, w: i32, h: i32) {
    rect(out, x, y, w, h, "#151515");
    rect(out, x + 1, y + 1, w - 2, h - 2, "#4a4a4a");
    rect(out, x + 2, y + 2, w - 4, h - 5, "#e8e8e8");
    // Panels: a dark band, then dotted screentone, then lines of speech.
    let (sx, sy, sw, sh) = (x + 3, y + 3, w - 6, h - 7);
    if sw < 2 || sh < 3 {
        return;
    }
    rect(out, sx, sy, sw, 1, "#707070");
    for py in sy + 2..sy + sh / 2 {
        for px in sx..sx + sw {
            if (px + py) % 2 == 0 {
                rect(out, px, py, 1, 1, "#b4b4b4");
            }
        }
    }
    for (i, py) in (sy + sh / 2 + 1..sy + sh).step_by(2).enumerate() {
        let len = if i % 2 == 0 { sw } else { sw * 2 / 3 };
        rect(out, sx, py, len.max(1), 1, "#707070");
    }
    // The button under the screen.
    rect(out, x + w / 2 - 1, y + h - 2, 2, 1, "#707070");
}

fn search() -> String {
    let mut s = cat(Some(19), 6);
    // The handle runs from the paw up to the lens.
    for x in 15..=24 {
        let y = 20 - (x - 15) * 4 / 10;
        rect(&mut s, x, y, 1, 2, "#a8826a");
    }
    let mut glass = String::new();
    ring(&mut glass, 28.0, 12.0, 0.0, 3.8, "#cfdcf5");
    s.push_str(&format!("<g opacity=\"0.45\">{glass}</g>"));
    ring(&mut s, 28.0, 12.0, 3.8, 5.6, "#d4b26a");
    // What it found: three covers, one after another.
    for (i, (fill, y)) in [("#f08585", 6), ("#8fb8e8", 13), ("#7fd18b", 20)].into_iter().enumerate() {
        s.push_str(&format!("<g class=\"found f{i}\">"));
        rect(&mut s, 40, y, 5, 6, fill);
        rect(&mut s, 41, y + 1, 3, 2, "#f5f5f5");
        rect(&mut s, 46, y + 1, 7, 1, "#b4b4b4");
        rect(&mut s, 46, y + 3, 5, 1, "#707070");
        s.push_str("</g>");
    }
    s
}

fn download() -> String {
    let mut s = cat(None, 6);
    // Chapters stacking up.
    for (i, fill) in ["#8fb8e8", "#b39ddb", "#f0a3b3"].into_iter().enumerate() {
        let y = 24 - i as i32 * 4;
        rect(&mut s, 32, y, 18, 3, fill);
        rect(&mut s, 33, y + 1, 6, 1, "#f5f5f5");
    }
    // The arrow that keeps bringing more.
    s.push_str("<g class=\"drop\">");
    sprite(&mut s, 38, 2, &["..ww..", "..ww..", "..ww..", "wwwwww", ".wwww.", "..ww.."]);
    s.push_str("</g>");
    // A progress bar filling up, in steps.
    rect(&mut s, 32, 29, 18, 2, "#2b2b2b");
    for (i, w) in [5, 10, 18].into_iter().enumerate() {
        s.push_str(&format!("<g class=\"fill p{i}\">"));
        rect(&mut s, 32, 29, w, 2, "#7fd18b");
        s.push_str("</g>");
    }
    s
}

fn screen() -> String {
    let mut s = cat(Some(17), 6);
    ereader(&mut s, 34, 3, 16, 26);
    // Crop marks that close in on the page.
    s.push_str("<g class=\"crop\">");
    for (x, y, w, h) in [(31, 0, 4, 1), (31, 0, 1, 4), (49, 0, 4, 1), (52, 0, 1, 4), (31, 31, 4, 1), (31, 28, 1, 4), (49, 31, 4, 1), (52, 28, 1, 4)] {
        rect(&mut s, x, y, w, h, "#d4b26a");
    }
    s.push_str("</g>");
    s
}

fn send() -> String {
    let mut s = cat(Some(18), 6);
    ereader(&mut s, 42, 9, 12, 18);
    // Waves from the cat to the reader, one after another.
    for (i, r) in [3.0f32, 5.5, 8.0].into_iter().enumerate() {
        let mut arc = String::new();
        for y in 0..H {
            for x in 26..40 {
                let d = ((x as f32 + 0.5 - 27.0).powi(2) + (y as f32 + 0.5 - 16.0).powi(2)).sqrt();
                if (d - r).abs() < 0.7 && x as f32 > 27.0 + r * 0.4 {
                    rect(&mut arc, x, y, 1, 1, "#8fb8e8");
                }
            }
        }
        s.push_str(&format!("<g class=\"wave w{i}\">{arc}</g>"));
    }
    // A USB stick on the floor.
    sprite(&mut s, 28, 27, &["gggg", "sBBBBBs", "sBBBBBs"]);
    s
}

fn readers() -> String {
    let mut s = cat(None, 8);
    // Readers of every size, side by side.
    ereader(&mut s, 24, 16, 9, 14);
    ereader(&mut s, 34, 11, 10, 19);
    ereader(&mut s, 45, 6, 11, 24);
    // A tick over the one that is chosen.
    s.push_str("<g class=\"pick\">");
    sprite(&mut s, 48, 0, &[".....G", "....GG", "G..GG.", "GGGG..", ".GG..."]);
    s.push_str("</g>");
    s
}

fn library() -> String {
    let mut s = cat(None, 6);
    // A shelf of books.
    let books = [("#f08585", 6), ("#8fb8e8", 7), ("#e3b341", 5), ("#b39ddb", 7), ("#7fd18b", 6), ("#e8963a", 7)];
    for row in 0..2 {
        let base = 13 + row * 12;
        let mut x = 30;
        for (i, (fill, h)) in books.iter().enumerate() {
            let h = if row == 1 { 13 - h } else { *h + 3 };
            let w = if i % 2 == 0 { 3 } else { 4 };
            rect(&mut s, x, base - h, w, h, fill);
            rect(&mut s, x, base - h + 2, w, 1, "#f5f5f5");
            x += w;
        }
        rect(&mut s, 29, base, 26, 1, "#a8826a");
    }
    // One book is out, open on the floor.
    sprite(&mut s, 18, 27, &["wwwwkwwwww", "wllwkwllww", "wwwwkwwwww"]);
    s
}

fn cached(cell: &'static OnceLock<String>, make: fn() -> String) -> &'static str {
    cell.get_or_init(|| super::pixels::compact(&make()))
}

/// The markup of the scene called `name`.
fn scene_markup(name: &str) -> &'static str {
    static SEARCH: OnceLock<String> = OnceLock::new();
    static DOWNLOAD: OnceLock<String> = OnceLock::new();
    static SCREEN: OnceLock<String> = OnceLock::new();
    static SEND: OnceLock<String> = OnceLock::new();
    static READERS: OnceLock<String> = OnceLock::new();
    static LIBRARY: OnceLock<String> = OnceLock::new();
    match name {
        "search" => cached(&SEARCH, search),
        "download" => cached(&DOWNLOAD, download),
        "screen" => cached(&SCREEN, screen),
        "send" => cached(&SEND, send),
        "readers" => cached(&READERS, readers),
        _ => cached(&LIBRARY, library),
    }
}

/// One of the scenes: "search", "download", "screen", "send", "readers" or "library".
#[component]
pub fn FeatureScene(name: &'static str, label: &'static str) -> Element {
    rsx! {
        svg {
            class: "scene-cat desk-scene", view_box: "0 0 {W} {H}", width: "224", height: "128",
            role: "img", "aria-label": "{label}",
            dangerous_inner_html: "{scene_markup(name)}",
        }
    }
}

const APPLE: [&str; 16] = [
    "..........ll....",
    ".........ll.....",
    "........ll......",
    "...llll.l.llll..",
    "..llllllllllll..",
    ".llllllllllllll.",
    ".lllllllllllll..",
    "lllllllllllll...",
    "lllllllllllll...",
    "lllllllllllll...",
    "llllllllllllll..",
    ".lllllllllllllll",
    ".llllllllllllll.",
    "..llllllllllll..",
    "...llll..llll...",
    "................",
];

const WINDOWS: [&str; 16] = [
    "................",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    "................",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
    ".BBBBBB.BBBBBBB.",
];

const LINUX: [&str; 16] = [
    "......kkkk......",
    ".....kkkkkk.....",
    ".....kwkkwk.....",
    ".....kkyykk.....",
    "....kkyyyykk....",
    "....kwwyywwk....",
    "...kkwwwwwwkk...",
    "...kwwwwwwwwk...",
    "..kkwwwwwwwwkk..",
    "..kkwwwwwwwwkk..",
    "..kkwwwwwwwwkk..",
    "..kkwwwwwwwwkk..",
    "...kkwwwwwwkk...",
    ".yyyykkkkkkyyyy.",
    "yyyyyy....yyyyyy",
    "................",
];

const GITHUB: [&str; 16] = [
    ".....wwwwww.....",
    "...wwwwwwwwww...",
    "..wwkwwwwwwkww..",
    ".wwwkkwwwwkkwww.",
    ".wwwkkkkkkkkwww.",
    "wwwkkkkkkkkkkwww",
    "wwwkkkkkkkkkkwww",
    "wwwkkkkkkkkkkwww",
    "wwwkkkkkkkkkkwww",
    "wwwwkkkkkkkkwwww",
    ".wkwwwkkkkwwwww.",
    ".wwkkwkkkkwwwww.",
    "..wwwkkkkkkwww..",
    "...wwwkkkkwww...",
    ".....wkkkkw.....",
    "................",
];

/// The mark of "macos", "windows", "linux" or "github", as pixels.
fn mark_markup(name: &str) -> &'static str {
    static MARKS: OnceLock<Vec<(&'static str, String)>> = OnceLock::new();
    let marks = MARKS.get_or_init(|| {
        [("macos", &APPLE), ("windows", &WINDOWS), ("linux", &LINUX), ("github", &GITHUB)]
            .into_iter()
            .map(|(n, rows)| {
                let mut s = String::new();
                sprite(&mut s, 0, 0, rows);
                (n, s)
            })
            .collect()
    });
    marks.iter().find(|(n, _)| *n == name).map_or("", |(_, s)| s.as_str())
}

#[component]
pub fn PixelMark(name: &'static str, #[props(default = 48)] size: u32) -> Element {
    rsx! {
        svg {
            class: "pixel-mark", view_box: "0 0 16 16", width: "{size}", height: "{size}", "aria-hidden": "true",
            dangerous_inner_html: "{mark_markup(name)}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_stays_inside_its_frame() {
        for name in ["search", "download", "screen", "send", "readers", "library"] {
            let mut m = String::new();
            m.push_str(&match name {
                "search" => search(),
                "download" => download(),
                "screen" => screen(),
                "send" => send(),
                "readers" => readers(),
                _ => library(),
            });
            // The cat is drawn in its own space (moved by a transform); the props are not.
            let props = m.rsplit("</g></g>").next().unwrap_or(&m);
            for part in props.split("<rect ").skip(1) {
                let num = |key: &str| -> i32 {
                    let rest = &part[part.find(key).unwrap() + key.len()..];
                    rest[..rest.find('"').unwrap()].parse().unwrap()
                };
                let (x, y, w, h) = (num("x=\""), num("y=\""), num("width=\""), num("height=\""));
                assert!(x >= 0 && y >= 0 && x + w <= W && y + h <= H, "{name}: {part}");
            }
        }
    }

    #[test]
    fn every_mark_is_drawn() {
        for name in ["macos", "windows", "linux", "github"] {
            assert!(mark_markup(name).contains("<rect"), "{name}");
        }
    }
}
