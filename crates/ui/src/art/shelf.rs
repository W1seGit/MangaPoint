//! The empty bookshelf: a small pixel-art scene for screens with nothing on
//! them yet. The cat sleeps on the middle shelf; a lamp glows, a mug steams,
//! and the bottom shelf is left empty on purpose.
//!
//! An original scene drawn in code on a 96x80 grid, turned into SVG rectangles
//! once. The cat is the same sleeping sprite as the one on its own (see
//! `cat.rs`), so it breathes and dreams the same way; the rest of the motion is
//! CSS (see `css/art.css`).

use dioxus::prelude::*;
use std::sync::OnceLock;

const W: usize = 96;
const H: usize = 80;
type Grid = [[u8; W]; H];

const CLEAR: u8 = b'.';

/// Colors by code. Grays like the rest of the app, with a few soft colors on
/// the books and the plant so the shelf feels lived in.
fn color(c: u8) -> &'static str {
    match c {
        b'k' => "#1f1f1f", // frame
        b'K' => "#343434", // frame highlight
        b'b' => "#0b0b0b", // back of the shelf
        b'B' => "#050505", // back, in shadow
        b'p' => "#4d4d4d", // plank
        b'P' => "#6a6a6a", // plank top edge
        b'q' => "#363636", // plank front shade
        b'a' => "#8a8a8a", // gray books
        b'c' => "#b4b4b4",
        b'd' => "#6a6a6a",
        b'e' => "#d0d0d0",
        b'w' => "#e8e8e8", // page edges, white things
        b'r' => "#c98a98", // rose
        b'x' => "#a56d7c", // darker rose
        b's' => "#86a587", // sage
        b'u' => "#7f93b5", // slate blue
        b'y' => "#d4b26a", // amber
        b't' => "#a8826a", // clay pot
        b'g' => "#6f9472", // leaf
        b'G' => "#4f6f52", // dark leaf
        b'm' => "#707070", // lamp metal
        b'o' => "#f3e6b0", // bulb
        b'f' => "#b4b4b4", // picture frame
        b'n' => "#2d3350", // night sky
        b'v' => "#171a2a", // hills
        b'j' => "#2e2e2e", // cobweb
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

/// A book standing on `floor`, `w` wide and `h` tall, with a lighter band near
/// the top and bottom of its spine.
fn standing_book(g: &mut Grid, x: usize, floor: usize, w: usize, h: usize, c: u8) {
    let top = floor + 1 - h;
    rect(g, x, top, x + w - 1, floor, c);
    rect(g, x, top + 2, x + w - 1, top + 2, b'w');
    rect(g, x, floor - 2, x + w - 1, floor - 2, b'w');
}

/// A book lying flat with its pages toward us, four rows tall, top row at `y`.
fn lying_book(g: &mut Grid, x: usize, y: usize, w: usize, cover: u8) {
    rect(g, x, y, x + w - 1, y + 3, cover);
    rect(g, x + 1, y + 1, x + w - 1, y + 2, b'w');
}

/// The cat lies here: its top-left corner on the grid.
pub const CAT_AT: (usize, usize) = (31, 22);
/// The floor of the middle shelf: the last row the cat may touch.
#[cfg(test)]
const MIDDLE_FLOOR: usize = 51;

/// Where the sprite's own bottom row falls, to check the cat sits on the shelf.
#[cfg(test)]
const SPRITE_BOTTOM: usize = 29;

fn scene() -> Grid {
    let mut g = [[CLEAR; W]; H];

    // Frame, with a slightly wider top.
    rect(&mut g, 0, 1, 95, 3, b'k');
    rect(&mut g, 0, 1, 95, 1, b'K');
    rect(&mut g, 1, 4, 3, 76, b'k');
    rect(&mut g, 92, 4, 94, 76, b'k');
    rect(&mut g, 1, 4, 1, 76, b'K');
    rect(&mut g, 92, 4, 92, 76, b'K');
    // The back, a little darker at the top.
    rect(&mut g, 4, 4, 91, 76, b'b');
    rect(&mut g, 4, 4, 91, 5, b'B');
    // Planks: top, middle, and the thick bottom one with feet.
    for y in [23, 52] {
        rect(&mut g, 4, y, 91, y + 2, b'p');
        rect(&mut g, 4, y, 91, y, b'P');
        rect(&mut g, 4, y + 2, 91, y + 2, b'q');
    }
    rect(&mut g, 4, 73, 91, 76, b'p');
    rect(&mut g, 4, 73, 91, 73, b'P');
    rect(&mut g, 4, 76, 91, 76, b'q');
    rect(&mut g, 6, 77, 12, 78, b'k');
    rect(&mut g, 83, 77, 89, 78, b'k');

    // ---- Top shelf (floor at row 22): books, a picture, a lamp.
    let mut x = 7;
    for (w, h, c) in [
        (3, 15, b'u'),
        (4, 17, b'c'),
        (3, 13, b'r'),
        (5, 16, b'a'),
        (3, 14, b's'),
        (4, 17, b'd'),
    ] {
        standing_book(&mut g, x, 22, w, h, c);
        x += w + 1;
    }
    // One book leaning on the others.
    for i in 0..15 {
        let shift = i / 4;
        rect(&mut g, x + shift, 22 - i, x + shift + 3, 22 - i, b'y');
    }
    rect(&mut g, x + 1, 22, x + 3, 22, b'y');
    // A small framed picture on the back wall: night, a moon, two hills.
    rect(&mut g, 44, 7, 61, 18, b'f');
    rect(&mut g, 46, 9, 59, 16, b'n');
    rect(&mut g, 54, 10, 56, 12, b'w');
    rect(&mut g, 47, 15, 52, 16, b'v');
    rect(&mut g, 49, 14, 50, 14, b'v');
    rect(&mut g, 53, 14, 58, 16, b'v');
    // A lamp.
    rect(&mut g, 75, 21, 83, 22, b'm');
    rect(&mut g, 78, 14, 80, 20, b'm');
    px(&mut g, 79, 15, b'o');
    for (y, x0, x1) in [
        (7, 76, 82),
        (8, 75, 83),
        (9, 74, 84),
        (10, 73, 85),
        (11, 73, 85),
        (12, 73, 85),
        (13, 73, 85),
    ] {
        rect(&mut g, x0, y, x1, y, b'w');
    }
    rect(&mut g, 78, 14, 80, 14, b'o');

    // ---- Middle shelf (floor at row 51): books and a sprout on the left, a
    // mug and a cactus on the right. The cat goes in the middle.
    lying_book(&mut g, 7, 48, 20, b'd');
    lying_book(&mut g, 9, 44, 17, b'r');
    lying_book(&mut g, 8, 40, 15, b'u');
    rect(&mut g, 12, 38, 16, 39, b't');
    rect(&mut g, 13, 35, 13, 37, b'g');
    rect(&mut g, 15, 34, 15, 37, b'g');
    px(&mut g, 14, 36, b'G');
    // The mug.
    rect(&mut g, 66, 45, 72, 51, b'w');
    rect(&mut g, 66, 51, 72, 51, b'a');
    rect(&mut g, 67, 45, 71, 45, b't');
    rect(&mut g, 73, 46, 74, 46, b'w');
    rect(&mut g, 74, 47, 74, 49, b'w');
    rect(&mut g, 73, 50, 74, 50, b'w');
    // A cactus in a clay pot.
    rect(&mut g, 80, 47, 87, 51, b't');
    rect(&mut g, 79, 46, 88, 46, b't');
    rect(&mut g, 82, 36, 85, 45, b'G');
    rect(&mut g, 83, 36, 83, 45, b'g');
    rect(&mut g, 80, 40, 81, 43, b'G');
    rect(&mut g, 80, 40, 82, 40, b'G');
    rect(&mut g, 86, 38, 87, 41, b'G');
    rect(&mut g, 85, 41, 86, 41, b'G');

    // ---- Bottom shelf (floor at row 72): mostly empty. A ball of yarn that
    // has rolled a little, two books on the far side, and a cobweb.
    for (y, x0, x1) in [
        (65, 14, 17),
        (66, 13, 18),
        (67, 12, 19),
        (68, 12, 19),
        (69, 12, 19),
        (70, 12, 19),
        (71, 13, 18),
        (72, 14, 17),
    ] {
        rect(&mut g, x0, y, x1, y, b'r');
    }
    for (x0, y0) in [(14, 67), (15, 68), (16, 69), (17, 70)] {
        px(&mut g, x0, y0, b'x');
    }
    rect(&mut g, 20, 72, 27, 72, b'r');
    lying_book(&mut g, 68, 69, 20, b's');
    lying_book(&mut g, 70, 65, 16, b'c');
    rect(&mut g, 5, 55, 10, 55, b'j');
    rect(&mut g, 5, 55, 5, 60, b'j');
    for i in 0..5 {
        px(&mut g, 5 + i, 55 + i, b'j');
    }
    px(&mut g, 7, 58, b'j');
    px(&mut g, 8, 57, b'j');
    g
}

/// One `<rect>` per horizontal run of equal color.
fn rects(g: &Grid) -> String {
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

/// A small pixel "z" at the given spot, 4 or 5 pixels square.
fn z_rects(x: usize, y: usize, size: usize) -> String {
    let rows: &[&str] = if size >= 5 {
        &["#####", "...#.", "..#..", ".#...", "#####"]
    } else {
        &["####", "..#.", ".#..", "####"]
    };
    let mut out = String::new();
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch == '#' {
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"1\" height=\"1\" fill=\"#e8e8e8\"/>",
                    x + dx,
                    y + dy
                ));
            }
        }
    }
    out
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let (cx, cy) = CAT_AT;
        let at = |inner: String, class: &str| {
            format!("<g class=\"{class}\" transform=\"translate({cx} {cy})\">{inner}</g>")
        };
        let mut s = String::new();
        // The lamp's glow sits behind everything else.
        s.push_str(
            "<ellipse class=\"glow\" cx=\"79\" cy=\"14\" rx=\"15\" ry=\"11\" fill=\"#f3e6b0\"/>",
        );
        s.push_str(&rects(&scene()));
        s.push_str(&at(super::cat::rest_frame_markup(false), "r r0"));
        s.push_str(&at(super::cat::rest_frame_markup(true), "r r1"));
        s.push_str(&format!(
            "<g class=\"z z1\">{}</g><g class=\"z z2\">{}</g>",
            z_rects(cx + 15, cy + 10, 4),
            z_rects(cx + 20, cy + 5, 5)
        ));
        // Steam over the mug: three puffs that rise and fade in turn.
        for (i, (x, y)) in [(68, 42), (70, 40), (69, 38)].into_iter().enumerate() {
            s.push_str(&format!(
                "<rect class=\"steam st{i}\" x=\"{x}\" y=\"{y}\" width=\"1\" height=\"2\" fill=\"#9a9a9a\"/>"
            ));
        }
        s
    })
}

/// A cat asleep on an empty bookshelf.
#[component]
pub fn ShelfCat() -> Element {
    rsx! {
        svg {
            class: "shelf-scene",
            width: "{W}",
            height: "{H}",
            view_box: "0 0 {W} {H}",
            role: "img",
            "aria-label": "A cat sleeping on an empty bookshelf",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(part: &str) -> (usize, usize, usize) {
        let num = |key: &str| -> usize {
            let rest = &part[part.find(key).unwrap() + key.len()..];
            rest[..rest.find('"').unwrap()].parse().unwrap()
        };
        (num("x=\""), num("y=\""), num("width=\""))
    }

    #[test]
    fn everything_is_drawn_inside_the_canvas() {
        for part in markup().split("<rect ").skip(1) {
            let (x, y, w) = numbers(part);
            assert!(x + w <= W && y < H, "{part}");
        }
    }

    #[test]
    fn the_scene_has_its_parts_and_they_move() {
        let m = markup();
        for class in [
            "glow",
            "r r0",
            "r r1",
            "z z1",
            "z z2",
            "steam st0",
            "steam st2",
        ] {
            assert!(m.contains(&format!("class=\"{class}\"")), "{class}");
        }
    }

    #[test]
    fn the_cat_lies_on_the_middle_shelf_with_room_above() {
        let (_, cy) = CAT_AT;
        assert_eq!(
            cy + SPRITE_BOTTOM,
            MIDDLE_FLOOR,
            "paws and tail rest on the shelf"
        );
        // The shelf above is above the cat's ears (the sprite's top row is 12).
        let g = scene();
        let plank_above = 25;
        assert!(cy + 12 > plank_above + 2, "ears clear the plank above");
        assert_eq!(
            g[MIDDLE_FLOOR + 1][50],
            b'P',
            "the plank is right under the cat"
        );
    }

    #[test]
    fn the_bottom_shelf_is_mostly_empty() {
        let g = scene();
        // Between the yarn and the books on the bottom shelf there is nothing.
        for (y, row) in g.iter().enumerate().take(73).skip(55) {
            for (x, &cell) in row.iter().enumerate().take(66).skip(30) {
                assert_eq!(cell, b'b', "something at {x},{y}");
            }
        }
    }

    #[test]
    fn every_shelf_runs_the_full_width() {
        let g = scene();
        for y in [23, 52, 73] {
            assert!((4..=91).all(|x| g[y][x] == b'P'), "plank at row {y}");
        }
    }
}
