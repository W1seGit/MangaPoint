//! A page with every scene on it, to look at. `MASCOT_GALLERY=gallery.html cargo test -p mangapoint-ui gallery -- --ignored`.

use super::mascot::*;

#[test]
#[ignore]
fn gallery() {
    let Ok(path) = std::env::var("MASCOT_GALLERY") else { return };
    let scene = |class: &str, vb: &str, inner: String| format!("<svg class=\"mascot scene {class}\" viewBox=\"{vb}\">{inner}</svg>");
    let loader = |m: u32| {
        format!("<span class=\"mloader\" style=\"--m:{m}px\"><svg class=\"mascot\" viewBox=\"0 0 44 57\">{}</svg><span class=\"mdots\"><span></span><span></span><span></span></span></span>", loader_markup())
    };
    let toast = |look: &str, mark: &str| format!("<span class=\"toast-mascot {look}\"><svg class=\"mascot\" viewBox=\"0 0 22 29\">{}</svg><span class=\"toast-badge\">{mark}</span></span> ", half_markup());
    let cards = [
        ("Asleep: every other empty page (reader, series, chapters, downloads, sources, export preview; web book and reader)", scene("rest", "0 0 64 66", rest_markup())),
        ("Paws on the shelf: an empty Library and Plan (desktop and web)", scene("shelf", "0 0 88 76", shelf_markup())),
        ("Magnifying glass: Search with no results", scene("Magnifying glass: Search with no results", "0 0 84 72", search_markup())),
        ("Tea: Tasks with nothing to do", scene("waiting", "0 0 76 72", waiting_markup())),
        ("Glass over websites: first start, Choose your sources", scene("browse", "0 0 126 66", browsing_markup())),
        ("Paw on the parcel: the update dialog (desktop and web)", scene("Paw on the parcel: the update dialog (desktop and web)", "0 0 82 68", updating_markup())),
        ("Walking: the splash while the app opens, and the first start", format!("<div class=\"walker\">{}</div>", scene("walk", "0 0 90 68", walking_markup(false)))),
        ("Walking by day: the first start, last step", format!("<div class=\"walker\">{}</div>", scene("walk", "0 0 90 68", walking_markup(true)))),
        ("Nodding with dots: every loading spinner (search, reader, sources, chapters, import)", loader(120)),
        ("Small: a cover still loading, Loading chapters", loader(56)),
        ("Toasts: done, warning, failed", format!("{}{}{}", toast("success", "✓"), toast("warning", "!"), toast("error", "×"))),
        ("A series with no cover", format!("<div style=\"width:150px\">{}</div>", super::placeholder::svg())),
    ];
    let mut html = format!(
        "<!doctype html><meta charset=utf-8><style>{}</style><body style=\"background:#1e1916;color:#aa8d7e;font:14px sans-serif;display:flex;flex-wrap:wrap;gap:18px;padding:18px\">",
        crate::CSS
    );
    for (name, body) in cards {
        html.push_str(&format!(
            "<div style=\"width:300px;background:#2a211d;border-radius:20px;padding:16px;text-align:center\"><div style=\"height:200px;display:flex;align-items:center;justify-content:center\"><div class=\"empty\" style=\"padding:0\">{body}</div></div><div>{name}</div></div>"
        ));
    }
    std::fs::write(path, html).unwrap();
}
