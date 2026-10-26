//! The pictures of the website's home page (`apps/site`), made from the app's own drawings so the two always
//! look alike: the mascot (Rullo) as it was drawn and delighted, its scenes as SVG files that carry their own
//! motion, and the icons the page uses as one sprite. They are kept in `apps/site/art`, and this test fails
//! when a drawing changes and they were not made again:
//!
//!     SITE_ART=write cargo test -p mangapoint-ui site_art

use super::mascot::*;
use std::path::Path;

/// The Lucide icons the home page uses.
const ICONS: [&str; 15] = [
    "download", "globe", "search", "book-open", "wifi-off", "smartphone", "hard-drive", "refresh-cw", "arrow-right",
    "crop", "layers", "lock", "check", "list-checks", "star",
];

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

fn scene(view_box: &str, label: &str, inner: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{view_box}\" shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"{label}\"><style>{}</style>{inner}</svg>\n",
        motion()
    )
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
    vec![
        // The head alone, for the name at the top of the page.
        ("mark.svg", scene("13 6 38 35", "MangaPoint", &rest_markup())),
        // The drawing just as it was drawn, eyes shut and smiling (the resting scene, without the room round it),
        // and the same delighted, for when it is pointed at. The two are the same size and lie on top of each other.
        ("mascot.svg", scene("10 6 44 57", "Rullo, the MangaPoint mascot", &rest_markup())),
        ("mascot-glad.svg", scene("10 6 44 57", "Rullo, delighted", &delighted_markup())),
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
