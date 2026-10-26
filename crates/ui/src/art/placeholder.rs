//! The picture shown where a cover is missing or has not loaded: the mascot
//! under the name of the app, in the app's own colours. Drawn as pixels, so it stays sharp at any size.

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

    fn text(&mut self, word: &str, y: i32) {
        let width = word.chars().count() as i32 * 6 - 1;
        let left = (WIDTH - width) / 2;
        // The shadow first, then the letters on top of it.
        for (dx, dy, color) in [(1, 1, "#161210"), (0, 0, "#aa8d7e")] {
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
}

fn build() -> String {
    let mut c = Canvas {
        out: format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
             preserveAspectRatio=\"xMidYMid slice\" shape-rendering=\"crispEdges\">"
        ),
    };
    // A dark poster with a lighter band behind the name.
    c.rect(0, 0, WIDTH, HEIGHT, "#1e1916");
    c.rect(0, 0, WIDTH, 24, "#2a211d");
    c.rect(0, 24, WIDTH, 1, "#3b2f2a");
    // The mascot, cut off by the lower edge as in the drawing.
    c.out.push_str(&format!("<g transform=\"translate(9 31)\">{}</g>", super::mascot::half_markup()));
    // The name.
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
        assert!(svg.matches("<path").count() > 10);
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
    }
}
