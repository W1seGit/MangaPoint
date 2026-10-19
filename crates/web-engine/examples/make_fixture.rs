//! Writes the fixture the Node smoke test (`tests/node/smoke.mjs`) compares the
//! WebAssembly build against: a few generated pages, the options of four exports,
//! and the files the native `Exporter` makes from them.
//!
//! usage: cargo run -p web-engine --example make_fixture -- <out dir>
//!
//! Everything is deterministic, so running it twice gives the same bytes.

use image::{ImageEncoder, Rgb, RgbImage};
use std::fs;
use std::path::Path;
use web_engine::chapters;
use web_engine::export::Exporter;
use web_engine::fingerprint;

fn png_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .expect("PNG encoding works");
    out
}

fn jpeg_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .expect("JPEG encoding works");
    out
}

/// A gray manga-like page: paper margin, panels and lines.
fn gray_page(w: u32, h: u32, seed: u8) -> Vec<u8> {
    png_of(&RgbImage::from_fn(w, h, |x, y| {
        let inside = x > w / 10 && x < w - w / 10 && y > h / 12 && y < h - h / 12;
        let v = if !inside {
            255
        } else if (x / 40 + y / 40 + u32::from(seed)) % 3 == 0 {
            30
        } else if y % 97 < 3 {
            120
        } else {
            235
        };
        Rgb([v, v, v])
    }))
}

/// A colour page: a red-to-blue gradient with a green band.
fn colour_page(w: u32, h: u32, seed: u8) -> Vec<u8> {
    jpeg_of(&RgbImage::from_fn(w, h, |x, y| {
        let inside = x > w / 12 && x < w - w / 12 && y > h / 14 && y < h - h / 14;
        if !inside {
            return Rgb([255, 255, 255]);
        }
        Rgb([(x * 255 / w) as u8, if y % 80 < 40 { 200 } else { seed }, (y * 255 / h) as u8])
    }))
}

type Book = Vec<(&'static str, f64, Vec<Vec<u8>>)>;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: make_fixture <out dir>");
    let dir = Path::new(&dir);
    fs::create_dir_all(dir.join("files")).unwrap();

    let gray: Book = vec![
        ("Chapter 1", 1.0, vec![gray_page(300, 450, 1), gray_page(300, 450, 2), gray_page(640, 450, 3)]),
        ("Chapter 2", 2.0, vec![gray_page(300, 450, 4)]),
    ];
    let colour: Book = vec![
        ("Chapter 1", 1.0, vec![colour_page(300, 450, 10), colour_page(300, 450, 60)]),
        ("Chapter 2", 2.0, vec![colour_page(320, 480, 120)]),
    ];
    let cases: Vec<(&str, &str, &Book)> = vec![
        ("epub", r#"{"device":"xteink-x4-pro","format":"EPUB","title":"Smoke Series","created":1781000000}"#, &gray),
        ("xtc", r#"{"device":"xteink-x4","format":"XTC","title":"Smoke Series","created":1781000000}"#, &gray),
        (
            "cbz-colour",
            r#"{"device":"kobo-libra-colour","format":"CBZ","title":"Smoke Series","created":1781000000,"info":{"author":"A Writer"}}"#,
            &colour,
        ),
        ("phone-pdf", r#"{"device":"phone-original","format":"PDF","title":"Smoke Series","created":1781000000}"#, &colour),
    ];

    let mut manifest_cases = Vec::new();
    let mut page_files = 0usize;
    let case_count = cases.len();
    for (name, options, book) in cases {
        let mut exporter = Exporter::new(options).expect("options are valid");
        let mut chapters_json = Vec::new();
        for (ci, (chapter, number, pages)) in book.iter().enumerate() {
            let mut names = Vec::new();
            for (pi, page) in pages.iter().enumerate() {
                let file = format!("files/{name}-page-{ci}-{pi}.bin");
                fs::write(dir.join(&file), page).unwrap();
                names.push(file);
                page_files += 1;
            }
            exporter.add_chapter(chapter, Some(*number), pages).expect("the chapter converts");
            chapters_json.push(serde_json::json!({ "name": chapter, "number": number, "pages": names }));
        }
        exporter.finish().expect("the export finishes");
        let mut expected = Vec::new();
        for i in 0..exporter.file_count() {
            let file = exporter.take_file(i).unwrap();
            let path = format!("files/{name}-expected-{i}.bin");
            fs::write(dir.join(&path), &file.bytes).unwrap();
            expected.push(serde_json::json!({ "name": file.name, "file": path }));
        }
        manifest_cases.push(serde_json::json!({
            "name": name,
            "options": options,
            "chapters": chapters_json,
            "expected": expected,
        }));
    }

    // Other functions of the engine, answered natively, for the module to repeat.
    let head = gray_page(50, 60, 9);
    let tail = colour_page(40, 30, 7);
    fs::write(dir.join("files/head.bin"), &head).unwrap();
    fs::write(dir.join("files/tail.bin"), &tail).unwrap();
    let entries = r#"[{"path":"Chapter 10/02.jpg","size":5},{"path":"Chapter 10/01.jpg","size":5},{"path":"Chapter 2/01.jpg","size":5}]"#;
    let state = r#"{"format":"mangapoint-state","version":1,"savedAt":"2026-10-19T12:00:00.000Z","books":[{"fingerprint":"0101010101010101010101010101010101010101010101010101010101010101","title":"X","position":{"page":3},"lastReadAt":5}],"settings":{"device":"xteink-x4"}}"#;
    let mut hasher = fingerprint::Fingerprinter::new();
    hasher.update(&head);
    hasher.update(&tail);
    let manifest = serde_json::json!({
        "cases": manifest_cases,
        "fingerprint": {
            "head": "files/head.bin",
            "tail": "files/tail.bin",
            "size": 123_456,
            "quick": fingerprint::book_fingerprint_head_tail(123_456, &head, &tail),
            "full": hasher.finish(),
        },
        "chapters": { "entries": entries, "expected": chapters::guess_chapters_json(entries).unwrap() },
        "state": { "input": state, "expected": web_engine::state::parse_json(state.as_bytes()).unwrap() },
        "devices": web_engine::devices::devices_json(),
        "options": { "expected": web_engine::options::default_options_json("kobo-libra-colour", "manhwa").unwrap() },
    });
    fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    println!("wrote {case_count} cases and {page_files} pages to {}", dir.display());
}
