//! The logo cat asleep upside down, hanging by its paws from a tree branch,
//! for the empty Downloads page. It is the same sprite as the logo (see
//! `cat.rs`), turned over, on a 48x38 grid with the same big pixels as the
//! other cats; the motion is CSS (see `css/art.css`).

use super::cat::body_markup;
use super::ocean::{group_plain, Layer};
use dioxus::prelude::*;
use std::sync::OnceLock;

pub const W: i32 = 48;
pub const H: i32 = 38;

const SHADE: &str = "#7d7d7d";
const LIGHT: &str = "#e8e8e8";
const BARK: &str = "#6b4f3a";
const BARK_LIGHT: &str = "#86644a";
const BARK_DARK: &str = "#47321f";
const LEAF: &str = "#6f9472";
const LEAF_DARK: &str = "#4f6f52";

fn tree() -> String {
    let mut out = String::new();
    let mut t = Layer::default();
    // The trunk on the left, and the branch growing out of it.
    t.rect(0, 0, 5, 14, BARK);
    t.rect(0, 0, 1, 14, BARK_DARK);
    t.rect(3, 0, 1, 14, BARK_LIGHT);
    t.rect(4, 3, 39, 1, BARK_LIGHT);
    t.rect(4, 4, 40, 2, BARK);
    t.rect(4, 6, 38, 1, BARK_DARK);
    t.pixels(&[(44, 4), (44, 5)], BARK_DARK);
    // A knot, and a couple of twigs.
    t.pixels(&[(12, 4), (13, 4), (12, 5)], BARK_DARK);
    t.pixels(&[(36, 2), (37, 1), (38, 1)], BARK);
    t.pixels(&[(40, 7), (41, 8)], BARK_DARK);
    out.push_str(&format!(
        "<g transform=\"translate(0,1)\">{}</g>",
        group_plain("", 1.0, &t)
    ));

    // Leaves that stir in two poses.
    for (class, dy) in [("leaves leaves-a", 0), ("leaves leaves-b", 1)] {
        let mut l = Layer::default();
        for (x, y) in [(38, 0), (39, 0), (40, 1), (38, 1), (39, 2)] {
            l.px(x, y + dy, LEAF);
        }
        l.pixels(&[(40, dy), (41, 2 + dy)], LEAF_DARK);
        for (x, y) in [(41, 8), (42, 8), (42, 9), (43, 9), (41, 9)] {
            l.px(x, y - dy, LEAF);
        }
        l.pixels(&[(43, 8 - dy), (42, 10 - dy)], LEAF_DARK);
        for (x, y) in [(7, 7), (8, 7), (8, 8), (9, 8)] {
            l.px(x, y + dy, LEAF);
        }
        l.px(7, 8 + dy, LEAF_DARK);
        out.push_str(&format!(
            "<g transform=\"translate(0,1)\">{}</g>",
            group_plain(class, 1.0, &l)
        ));
    }
    out
}

fn cat() -> String {
    let mut out = String::new();

    // The tail hangs down beside the body and swings; two poses.
    for (class, dx) in [("tail-a", 0), ("tail-b", 1)] {
        let mut tail = Layer::default();
        for (x, y) in [
            (31, 11),
            (32, 12),
            (32, 13),
            (33, 14),
            (33, 15),
            (33, 16),
            (33, 17),
        ] {
            tail.px(x + dx, y, SHADE);
        }
        tail.rect(33 + dx, 18, 1, 3, "#4a4a4a");
        out.push_str(&group_plain(class, 1.0, &tail));
    }

    // The logo cat, turned upside down: its paws are at the top, over the
    // branch, and its head hangs at the bottom with its eyes shut.
    out.push_str(&format!(
        "<g transform=\"translate(8,36) scale(1,-1)\">{}</g>",
        body_markup(true)
    ));
    out
}

fn zzz() -> String {
    let mut out = String::new();
    for (i, (x, y, big)) in [(35, 24, false), (38, 18, true)].into_iter().enumerate() {
        let rows: &[&str] = if big {
            &["###", "..#", ".#.", "###"]
        } else {
            &["##", ".#", "##"]
        };
        let mut z = Layer::default();
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c == '#' {
                    z.px(x + dx as i32, y + dy as i32, LIGHT);
                }
            }
        }
        out.push_str(&group_plain(&format!("zz z{i}"), 1.0, &z));
    }
    out
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| format!("{}<g class=\"swing\">{}</g>{}", tree(), cat(), zzz()))
}

/// The cat sleeping upside down on a branch, while nothing is downloading.
#[component]
pub fn HangingCat() -> Element {
    rsx! {
        svg {
            class: "hang",
            view_box: "0 0 {W} {H}",
            role: "img",
            "aria-label": "A cat sleeping upside down, hanging from a tree branch",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_drawn_inside_the_canvas() {
        for part in markup().split("<rect ").skip(1) {
            let num = |key: &str| -> i32 {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x >= 0 && x + w <= W && (0..H).contains(&y), "{part}");
        }
    }

    #[test]
    fn it_sleeps_upside_down_with_its_parts_that_move() {
        let m = markup();
        for class in ["swing", "tail-a", "tail-b", "leaves-a", "leaves-b", "zz z1"] {
            assert!(m.contains(class), "{class}");
        }
        assert!(
            m.contains(BARK) && m.contains("scale(1,-1)"),
            "the logo cat, upside down"
        );
    }
}
