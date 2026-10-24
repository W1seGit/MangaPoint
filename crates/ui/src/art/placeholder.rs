//! The picture shown where a cover is missing or has not loaded: a cat in a
//! moonlit forest, with the name of the app across the sky. Drawn as pixels,
//! like the other cats, so it stays sharp at any size.

use std::fmt::Write;
use std::sync::OnceLock;

/// The picture is 40 pixels wide and 60 tall, the shape of a poster.
const WIDTH: i32 = 40;
const HEIGHT: i32 = 60;

/// Letters of the name in a 5 by 7 pixel font.
pub(super) fn glyph(c: char) -> [&'static str; 7] {
    match c {
        'M' => [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
        'A' => [
            "01110", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'N' => [
            "10001", "11001", "10101", "10011", "10001", "10001", "10001",
        ],
        'G' => [
            "01110", "10001", "10000", "10111", "10001", "10001", "01110",
        ],
        'P' => [
            "11110", "10001", "10001", "11110", "10000", "10000", "10000",
        ],
        'O' => [
            "01110", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'I' => [
            "01110", "00100", "00100", "00100", "00100", "00100", "01110",
        ],
        _ => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ], // T
    }
}

struct Canvas {
    out: String,
}

impl Canvas {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: &str) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(WIDTH), (y + h).min(HEIGHT));
        if x1 > x0 && y1 > y0 {
            let _ = write!(
                self.out,
                "<rect x=\"{x0}\" y=\"{y0}\" width=\"{}\" height=\"{}\" fill=\"{color}\"/>",
                x1 - x0,
                y1 - y0
            );
        }
    }

    /// A pine tree: a trunk and a widening cone, `base` being its lowest row.
    fn pine(&mut self, x: i32, base: i32, height: i32, color: &str) {
        self.rect(x, base - 2, 1, 2, "#3b2a1c");
        for i in 0..height {
            // Each tier steps in a little at the top, like a real pine.
            let half = (i as f32 * 0.5) as i32 - i32::from(i % 4 == 0 && i > 0);
            self.rect(x - half, base - height - 2 + i, half * 2 + 1, 1, color);
        }
    }

    fn text(&mut self, word: &str, y: i32) {
        let width = word.chars().count() as i32 * 6 - 1;
        let left = (WIDTH - width) / 2;
        // The shadow first, then the letters on top of it.
        for (dx, dy, color) in [(1, 1, "#05070d"), (0, 0, "#f2f0e6")] {
            for (n, c) in word.chars().enumerate() {
                for (row, bits) in glyph(c).iter().enumerate() {
                    for (col, bit) in bits.chars().enumerate() {
                        if bit == '1' {
                            self.rect(
                                left + n as i32 * 6 + col as i32 + dx,
                                y + row as i32 + dy,
                                1,
                                1,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }

    /// The mascot, as it is in the logo, sitting on the ground.
    fn cat(&mut self, left: i32, top: i32) {
        for (x, y, color) in super::cat::mascot_pixels() {
            self.rect(left + x as i32, top + y as i32, 1, 1, color);
        }
    }
}

fn build() -> String {
    let mut c = Canvas {
        out: format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
             preserveAspectRatio=\"xMidYMid slice\" shape-rendering=\"crispEdges\">"
        ),
    };
    // The night sky, in bands that grow lighter toward the trees.
    let sky = [
        "#070b16", "#0a1020", "#0d162b", "#111d38", "#162545", "#1b2d52",
    ];
    for (i, color) in sky.iter().enumerate() {
        c.rect(0, i as i32 * 7, WIDTH, 7, color);
    }
    // Stars.
    for (x, y) in [
        (3, 25),
        (9, 27),
        (17, 24),
        (35, 22),
        (22, 29),
        (6, 31),
        (33, 31),
    ] {
        c.rect(x, y, 1, 1, "#cfd6ee");
    }
    // The moon, with a few craters.
    c.rect(29, 24, 6, 6, "#ece7c9");
    c.rect(28, 25, 8, 4, "#ece7c9");
    c.rect(30, 25, 1, 1, "#cfc9a5");
    c.rect(33, 27, 1, 1, "#cfc9a5");
    c.rect(31, 28, 1, 1, "#cfc9a5");
    // Three rows of trees, each nearer and darker than the one behind.
    for x in [2, 8, 15, 22, 30, 37] {
        c.pine(x, 44, 11, "#14263d");
    }
    for x in [0, 6, 12, 19, 27, 34, 40] {
        c.pine(x, 48, 14, "#0f3328");
    }
    for x in [4, 13, 25, 36] {
        c.pine(x, 54, 18, "#082318");
    }
    // The ground, with tufts of grass.
    c.rect(0, 50, WIDTH, 10, "#0a1c14");
    c.rect(0, 50, WIDTH, 1, "#12301f");
    for x in [3, 9, 16, 30, 36] {
        c.rect(x, 51, 1, 2, "#1c4a2e");
    }
    // Fireflies among the trees.
    for (x, y) in [(5, 41), (33, 38), (9, 46), (36, 47)] {
        c.rect(x, y, 1, 1, "#e6d96a");
    }
    c.cat(2, 22);
    // The name, over the sky.
    c.text("MANGA", 4);
    c.text("POINT", 13);
    c.out.push_str("</svg>");
    c.out
}

/// The picture as SVG markup. Made once.
pub fn svg() -> &'static str {
    static SVG: OnceLock<String> = OnceLock::new();
    SVG.get_or_init(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_a_poster_with_detail_in_it() {
        let svg = svg();
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 40 60\""));
        assert!(svg.matches("<rect").count() > 300);
    }

    #[test]
    fn every_pixel_stays_inside_the_picture() {
        let svg = svg();
        for part in svg.split("<rect ").skip(1) {
            let number = |name: &str| -> i32 {
                let at = part.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
                part[at..].split('"').next().unwrap().parse().unwrap()
            };
            assert!(
                number("x") >= 0 && number("x") + number("width") <= WIDTH,
                "{part}"
            );
            assert!(
                number("y") >= 0 && number("y") + number("height") <= HEIGHT,
                "{part}"
            );
        }
    }

    #[test]
    fn the_name_is_spelled_with_known_letters() {
        for c in "MANGAPOINT".chars() {
            assert!(glyph(c).iter().all(|row| row.len() == 5), "{c}");
        }
        assert!(super::super::cat::mascot_pixels().len() > 200);
    }
}
