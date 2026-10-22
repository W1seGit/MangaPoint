//! The cat with a parcel in the dialog that offers a newer version of the desktop app. The website
//! shows the same dialog when a new version of the site is ready.

use dioxus::prelude::*;
use std::sync::OnceLock;

// ---- the cat ------------------------------------------------------------------

/// The scene: a gray cat sitting beside a parcel, with an arrow over it that
/// bobs up and down. Drawn as pixels, in an original sprite.
const CAT_LEFT: [&str; 14] = [
    "..g....", "..gg...", ".gpgggg", ".gggggg", ".ggeggg", ".gggggg", ".gggggp", "..ggggg",
    "..ggggg", ".ggllll", ".ggllll", ".gggggg", ".ggggss", ".llg...",
];
const PARCEL: [&str; 9] = [
    "DDDDDDDDDDDD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DbbbbttbbbbD",
    "DDDDDDDDDDDD",
];
const ARROW: [&str; 8] = [
    "...a...", "..aaa..", ".aaaaa.", "aaaaaaa", "..aaa..", "..aaa..", "..aaa..", "..aaa..",
];

fn hex(c: char) -> Option<&'static str> {
    Some(match c {
        'g' => "#b4b4b4",
        's' => "#707070",
        'l' => "#e8e8e8",
        'p' => "#f0a3b3",
        'e' => "#050505",
        'D' => "#6e4a26",
        'b' => "#a8733f",
        't' => "#e8d9a0",
        'a' => "#7fd18b",
        'w' => "#f5f5f5",
        _ => return None,
    })
}

fn rects(rows: &[String], ox: usize, oy: usize) -> String {
    let mut out = String::new();
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if let Some(fill) = hex(c) {
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"1\" height=\"1\" fill=\"{fill}\"/>",
                    ox + x,
                    oy + y
                ));
            }
        }
    }
    out
}

fn mirrored(left: &[&str]) -> Vec<String> {
    left.iter()
        .map(|r| format!("{r}{}", r.chars().rev().collect::<String>()))
        .collect()
}

fn scene_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let cat = mirrored(&CAT_LEFT);
        let parcel: Vec<String> = PARCEL.iter().map(|s| s.to_string()).collect();
        let arrow: Vec<String> = ARROW.iter().map(|s| s.to_string()).collect();
        // The tail, curling up behind the cat.
        let tail: Vec<String> = ["..s", ".s.", ".s.", "ss.", "..."].iter().map(|s| s.to_string()).collect();
        format!(
            "<g>{}</g><g>{}</g><g>{}</g><g class=\"upd-arrow\">{}</g><g class=\"upd-spark\">{}</g>",
            rects(&tail, 0, 12),
            rects(&cat, 3, 8),
            rects(&parcel, 21, 13),
            rects(&arrow, 23, 3),
            rects(&["w".to_string()], 21, 6)
                + &rects(&["w".to_string()], 32, 4)
                + &rects(&["w".to_string()], 19, 12)
        )
    })
}

/// A cat sitting by a parcel, for whatever is being brought up to date.
#[component]
pub fn UpdateCat() -> Element {
    rsx! {
        svg {
            class: "upd-cat",
            width: "180",
            height: "110",
            view_box: "0 0 36 22",
            role: "img",
            "aria-label": "A cat with a parcel",
            dangerous_inner_html: "{scene_markup()}",
        }
    }
}
