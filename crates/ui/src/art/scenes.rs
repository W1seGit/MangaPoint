//! A small pixel-art scene for an empty screen starring the gray cat: it
//! searches with a magnifying glass. Original art drawn in code as SVG rectangles; the
//! motion is CSS (see `css/art.css`).

use super::cat::sitting_markup;
use dioxus::prelude::*;
use std::sync::OnceLock;

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    out.push_str(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>"
    ));
}

/// The pixels of a ring around (`cx`, `cy`) between two radii.
fn ring(out: &mut String, cx: f32, cy: f32, inner: f32, outer: f32, fill: &str) {
    for y in 0..32 {
        for x in 0..44 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d >= inner && d <= outer {
                rect(out, x, y, 1, 1, fill);
            }
        }
    }
}

/// The cat, blinking now and then: an open-eyed layer and a blinking one,
/// moved `dx` pixels to the left to make room.
fn blinking_cat(paw_top: usize, dx: i32) -> String {
    format!(
        "<g transform=\"translate({},0)\"><g class=\"look-open\">{}</g><g class=\"look-blink\">{}</g></g>",
        -dx,
        sitting_markup(false, Some(paw_top)),
        sitting_markup(true, Some(paw_top))
    )
}

fn search_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        // The cat sits left of centre and holds the glass out to its right.
        let mut s = blinking_cat(19, 4);
        // The handle runs from the paw (about 17,20) up to the lens.
        for x in 17..=27 {
            let y = 20 - (x - 17) * 4 / 10;
            rect(&mut s, x, y, 1, 2, "#a8826a");
        }
        // The lens: a pale glass inside an amber rim, with a glint.
        let mut glass = String::new();
        ring(&mut glass, 31.0, 12.0, 0.0, 3.8, "#cfdcf5");
        s.push_str(&format!("<g opacity=\"0.45\">{glass}</g>"));
        ring(&mut s, 31.0, 12.0, 3.8, 5.6, "#d4b26a");
        for (x, y) in [(28, 9), (29, 8), (28, 10)] {
            rect(&mut s, x, y, 1, 1, "#e8e8e8");
        }
        // A sparkle that comes and goes on the glass.
        s.push_str("<g class=\"sparkle\">");
        rect(&mut s, 33, 9, 1, 1, "#ffffff");
        rect(&mut s, 32, 10, 3, 1, "#ffffff");
        rect(&mut s, 33, 11, 1, 1, "#ffffff");
        s.push_str("</g>");
        s
    })
}

/// The cat looking through a magnifying glass, for a search with nothing to show.
#[component]
pub fn SearchCat() -> Element {
    rsx! {
        svg {
            class: "scene-cat", width: "200", height: "160", view_box: "0 0 40 32",
            role: "img", "aria-label": "A cat searching with a magnifying glass",
            dangerous_inner_html: "{search_markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(markup: &str, width: i32) {
        for part in markup.split("<rect ").skip(1) {
            let num = |key: &str| -> i32 {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x >= 0 && x + w <= width && (0..32).contains(&y), "{part}");
        }
    }

    #[test]
    fn the_searching_cat_holds_a_moving_glass() {
        let m = search_markup();
        assert!(m.contains("class=\"sparkle\"") && m.contains("look-blink"));
        inside(m, 40);
    }
}
