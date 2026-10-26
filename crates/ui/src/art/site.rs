//! The pictures of the website's home page (`apps/site`), made from the app's own drawings so the two always
//! look alike: the mascot's scenes as SVG files that carry their own motion, the picture at the top of the
//! page, and the icons the page uses as one sprite. They are kept in `apps/site/art`, and this test fails when
//! a drawing changes and they were not made again:
//!
//!     SITE_ART=write cargo test -p mangapoint-ui site_art

use super::mascot::*;
use std::fmt::Write;
use std::path::Path;

/// The Lucide icons the home page uses.
const ICONS: [&str; 14] = [
    "download", "globe", "search", "book-open", "wifi-off", "smartphone", "hard-drive", "refresh-cw", "arrow-right",
    "crop", "layers", "lock", "check", "list-checks",
];

/// The colours of the drawings, and the paper and ink of a page on an e-ink screen.
const LINE: &str = "#000000";
const FUR: &str = "#aa8d7e";
const CHEEK: &str = "#3b2f2a";
const THIRD: &str = "#45362e";
const WOOD: &str = "#2a211d";
const DARK: &str = "#161210";
const PAPER: &str = "#d6cdc4";
const GRAY: &str = "#968b82";
const INK: &str = "#2a211d";

/// The motion of `art.css` (not how the scenes sit in a page), without comments or extra spaces.
fn motion() -> String {
    let css = include_str!("../../css/art.css");
    let css = css.split("/* ---- the scenes in a page ---- */").next().unwrap_or(css);
    let mut out = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start..].find("*/").map_or("", |end| &rest[start + end + 2..]);
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn scene_with(view_box: &str, label: &str, inner: &str, css: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{view_box}\" shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"{label}\"><style>{}{css}</style>{inner}</svg>\n",
        motion()
    )
}

fn scene(view_box: &str, label: &str, inner: &str) -> String {
    scene_with(view_box, label, inner, "")
}

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    let _ = write!(out, "<path fill=\"{fill}\" d=\"M{x} {y}h{w}v{h}h-{w}z\"/>");
}

/// A box with a one-pixel outline and its corners cut, filled with `fill`.
fn outlined(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    rect(out, x + 1, y, w - 2, h, LINE);
    rect(out, x, y + 1, w, h - 2, LINE);
    rect(out, x + 1, y + 1, w - 2, h - 2, fill);
}

/// A manga page at (`x`, `y`), `w` by `h`: panels with ink borders, people drawn as simple shapes, and a
/// speech bubble. `layout` picks one of two ways of cutting the page into panels.
fn manga_page(out: &mut String, x: i32, y: i32, w: i32, h: i32, layout: u8) {
    rect(out, x, y, w, h, PAPER);
    let panel = |out: &mut String, px: i32, py: i32, pw: i32, ph: i32| {
        rect(out, px, py, pw, ph, INK);
        rect(out, px + 1, py + 1, pw - 2, ph - 2, PAPER);
    };
    // Somebody: a round head and shoulders, in gray with an ink outline.
    let person = |out: &mut String, cx: i32, by: i32, size: i32| {
        let head = size / 2 + 1;
        rect(out, cx - head / 2, by - size - head, head, head, INK);
        rect(out, cx - head / 2 + 1, by - size - head + 1, head - 2, head - 2, GRAY);
        rect(out, cx - size / 2, by - size, size, size, INK);
        rect(out, cx - size / 2 + 1, by - size + 1, size - 2, size - 1, GRAY);
    };
    let bubble = |out: &mut String, bx: i32, by: i32, bw: i32| {
        rect(out, bx + 1, by, bw - 2, 5, INK);
        rect(out, bx, by + 1, bw, 3, INK);
        rect(out, bx + 1, by + 1, bw - 2, 3, "#ffffff");
        rect(out, bx + 2, by + 2, bw - 4, 1, GRAY);
        rect(out, bx + 2, by + 5, 1, 1, INK);
    };
    let (m, g) = (2, 1);
    let (iw, ih) = (w - 2 * m, h - 2 * m);
    if layout == 0 {
        // A wide panel on top, two side by side under it.
        let top = ih * 2 / 5;
        panel(out, x + m, y + m, iw, top);
        person(out, x + m + iw / 3, y + m + top - 1, 6);
        person(out, x + m + iw * 2 / 3 + 2, y + m + top - 1, 5);
        bubble(out, x + m + 3, y + m + 2, 11);
        let half = (iw - g) / 2;
        let low = y + m + top + g;
        let lh = ih - top - g;
        panel(out, x + m, low, half, lh);
        person(out, x + m + half / 2, low + lh - 1, 8);
        panel(out, x + m + half + g, low, iw - half - g, lh);
        // Speed lines.
        for i in 0..4 {
            rect(out, x + m + half + g + 3, low + 4 + i * 5, iw - half - g - 6 - i * 2, 1, GRAY);
        }
        bubble(out, x + m + half + g + 2, low + lh - 8, 10);
    } else {
        // A tall panel on the left, two stacked on the right.
        let left = (iw - g) / 2;
        panel(out, x + m, y + m, left, ih);
        person(out, x + m + left / 2, y + m + ih - 1, 10);
        bubble(out, x + m + 2, y + m + 3, left - 4);
        let rx = x + m + left + g;
        let rw = iw - left - g;
        let th = (ih - g) / 2;
        panel(out, rx, y + m, rw, th);
        person(out, rx + rw / 2, y + m + th - 1, 6);
        panel(out, rx, y + m + th + g, rw, ih - th - g);
        bubble(out, rx + 2, y + m + th + g + 3, rw - 4);
        rect(out, rx + 3, y + m + th + g + 12, rw - 6, 1, GRAY);
        rect(out, rx + 3, y + m + th + g + 15, rw - 9, 1, GRAY);
    }
}

/// The picture at the top of the home page: manga sites on the left, the mascot in the middle, and pages flying
/// over its head onto an e-reader on the right, whose screen turns to each new page as it lands.
fn hero() -> (String, String) {
    let mut s = String::new();
    // The sites, coming in from the left, their lines lighting up in turn.
    s.push_str("<g class=\"in-l\">");
    for (i, y) in [14, 32, 50].into_iter().enumerate() {
        let x = 4;
        let _ = write!(s, "<g class=\"m-window w{i}\">");
        rect(&mut s, x, y, 46, 16, LINE);
        rect(&mut s, x + 1, y + 1, 44, 14, WOOD);
        rect(&mut s, x + 1, y + 1, 44, 3, CHEEK);
        rect(&mut s, x + 3, y + 2, 1, 1, FUR);
        rect(&mut s, x + 5, y + 2, 1, 1, FUR);
        // A cover, and the lines of the page next to it.
        rect(&mut s, x + 3, y + 6, 7, 8, CHEEK);
        rect(&mut s, x + 4, y + 7, 5, 3, THIRD);
        rect(&mut s, x + 12, y + 7, 22, 1, CHEEK);
        rect(&mut s, x + 12, y + 10, 30, 1, CHEEK);
        s.push_str("<g class=\"m-lit\">");
        rect(&mut s, x + 12, y + 7, 22, 1, FUR);
        rect(&mut s, x + 12, y + 10, 30, 1, FUR);
        s.push_str("</g></g>");
    }
    s.push_str("</g>");

    // The e-reader, coming in from the right: a dark body, an e-ink screen showing a page, a button.
    s.push_str("<g class=\"in-r\">");
    let (rx, ry, rw, rh) = (156, 6, 52, 66);
    outlined(&mut s, rx, ry, rw, rh, CHEEK);
    rect(&mut s, rx + 2, ry + 1, rw - 4, 1, THIRD);
    let (sx, sy, sw, sh) = (rx + 4, ry + 4, rw - 8, rh - 14);
    rect(&mut s, sx - 1, sy - 1, sw + 2, sh + 2, LINE);
    s.push_str("<g class=\"pg a\">");
    manga_page(&mut s, sx, sy, sw, sh, 0);
    s.push_str("</g><g class=\"pg b\">");
    manga_page(&mut s, sx, sy, sw, sh, 1);
    s.push_str("</g>");
    rect(&mut s, rx + rw / 2 - 5, ry + rh - 6, 10, 2, DARK);
    s.push_str("</g>");

    // The mascot, as it was drawn, coming up from below, halfway between the sites and the reader (x 81 to 125).
    let _ = write!(s, "<g class=\"in-up\"><g transform=\"translate(71 9)\">{}</g></g>", rest_markup());

    // Pages on their way, from the sites over the mascot's head (its ears start at y 15) onto the reader's screen.
    for i in 0..2 {
        let _ = write!(s, "<g transform=\"translate(52 36)\"><g class=\"fly f{i}\">");
        rect(&mut s, 0, 0, 11, 14, LINE);
        rect(&mut s, 1, 1, 9, 12, PAPER);
        rect(&mut s, 2, 2, 7, 4, GRAY);
        rect(&mut s, 2, 7, 3, 5, GRAY);
        rect(&mut s, 6, 7, 3, 5, GRAY);
        s.push_str("</g></g>");
    }

    // A page lands every 1.8 s, the first at 2.4 + 0.96 * 3.6 = 5.856 s; the screen turns then and every 1.8 s
    // after (its animation starts 1.8 s before that, on the page it already shows).
    let css = "\
.in-l,.in-r,.in-up{animation:in-l .9s cubic-bezier(.2,.8,.2,1) 1s backwards}\
.in-r{animation-name:in-r;animation-delay:1.1s}\
.in-up{animation-name:in-up;animation-delay:1.25s}\
@keyframes in-l{from{transform:translateX(-40px);opacity:0}to{transform:none;opacity:1}}\
@keyframes in-r{from{transform:translateX(40px);opacity:0}to{transform:none;opacity:1}}\
@keyframes in-up{from{transform:translateY(24px);opacity:0}to{transform:none;opacity:1}}\
.fly{opacity:0;animation:fly 3.6s 2.4s infinite}\
.fly.f1{animation-delay:4.2s}\
@keyframes fly{\
0%{transform:translate(0,0);opacity:0;animation-timing-function:steps(4)}\
6%{opacity:1}\
15%{transform:translate(19px,-30px);animation-timing-function:steps(3)}\
30%{transform:translate(37px,-35px);animation-timing-function:steps(4)}\
50%{transform:translate(62px,-36px);animation-timing-function:steps(4)}\
70%{transform:translate(87px,-33px);animation-timing-function:steps(3)}\
85%{transform:translate(106px,-22px);animation-timing-function:steps(3)}\
90%{opacity:1}\
96%{transform:translate(124px,-6px);opacity:0}\
100%{transform:translate(124px,-6px);opacity:0}}\
.pg.b{opacity:0}\
.pg.a{animation:pg-a 3.6s steps(1,end) 4.056s infinite}\
.pg.b{animation:pg-b 3.6s steps(1,end) 4.056s infinite}\
@keyframes pg-a{0%,50%{opacity:1}50.01%,100%{opacity:0}}\
@keyframes pg-b{0%,50%{opacity:0}50.01%,100%{opacity:1}}\
@media (prefers-reduced-motion:reduce){.in-l,.in-r,.in-up,.fly,.pg.a,.pg.b{animation:none}.pg.b{opacity:0}}";
    (s, css.to_string())
}

fn icons() -> String {
    let mut out = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for name in ICONS {
        let body = super::icons::body(name).unwrap_or_else(|| panic!("no Lucide icon {name}"));
        out.push_str(&format!(
            "<symbol id=\"{name}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{body}</symbol>"
        ));
    }
    out.push_str("</svg>\n");
    out
}

fn files() -> Vec<(&'static str, String)> {
    let (hero, hero_css) = hero();
    vec![
        // The head alone, for the name at the top of the page.
        ("mark.svg", scene("13 6 38 35", "MangaPoint", &rest_markup())),
        // The drawing just as it was drawn, eyes shut and smiling (the resting scene, without the room round it).
        ("mascot.svg", scene("10 6 44 57", "The MangaPoint mascot", &rest_markup())),
        (
            "hero.svg",
            scene_with("0 0 212 72", "Manga from the web going to an e-reader, past the MangaPoint mascot", &hero, &hero_css),
        ),
        ("search.svg", scene("0 0 84 72", "The mascot looking through a magnifying glass", &search_markup())),
        ("waiting.svg", scene("0 0 76 72", "The mascot, waiting with a cup of tea", &waiting_markup())),
        ("shelf.svg", scene("0 0 88 76", "The mascot, asleep on a bookcase", &shelf_markup())),
        ("update.svg", scene("0 0 82 68", "The mascot with a parcel", &updating_markup())),
        ("icons.svg", icons()),
    ]
}

#[test]
fn site_art() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/site/art");
    let write = std::env::var("SITE_ART").is_ok_and(|v| v == "write");
    let mut stale = Vec::new();
    for (name, body) in files() {
        let path = dir.join(name);
        if write {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &body).unwrap();
        } else if std::fs::read_to_string(&path).map(|s| s.replace("\r\n", "\n")).ok().as_deref() != Some(body.as_str()) {
            stale.push(name);
        }
    }
    assert!(stale.is_empty(), "apps/site/art is out of date ({stale:?}): SITE_ART=write cargo test -p mangapoint-ui site_art");
}
