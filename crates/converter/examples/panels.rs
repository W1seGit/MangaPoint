//! Shows the panels found on real pages, to check the cutting by eye.
//!
//! ```sh
//! cargo run -p converter --features decode --example panels -- <pages folder> <output folder> [--ltr]
//! ```
//!
//! Every image in the folder is written to the output folder as a PNG with
//! each panel framed and numbered in reading order (a page that is not cut
//! has no frames), followed by one line per page on the console.

use converter::decode::decode_to_gray;
use converter::process::DEFAULT_TRIM_SENSITIVITY;
use converter::xtc::ReadDirection;
use converter::{find_panels, Rect};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let direction = if args.iter().any(|a| a == "--ltr") {
        ReadDirection::LeftToRight
    } else {
        ReadDirection::RightToLeft
    };
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [input, output] = paths[..] else {
        eprintln!("usage: panels <pages folder> <output folder> [--ltr]");
        std::process::exit(2);
    };
    std::fs::create_dir_all(output).expect("create the output folder");
    let mut pages: Vec<PathBuf> = std::fs::read_dir(input)
        .expect("read the pages folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                ["jpg", "jpeg", "png", "webp", "gif"].contains(&e.to_ascii_lowercase().as_str())
            })
        })
        .collect();
    pages.sort();
    for path in pages {
        let Ok(gray) = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| decode_to_gray(&b).map_err(|e| e.to_string()))
        else {
            eprintln!("{}: could not be read", path.display());
            continue;
        };
        let started = std::time::Instant::now();
        let panels = find_panels(&gray, direction, DEFAULT_TRIM_SENSITIVITY, 0);
        println!(
            "{}: {} panels in {:?}",
            path.display(),
            panels.len(),
            started.elapsed()
        );
        let mut canvas =
            image::RgbImage::from_fn(gray.width, gray.height, |x, y| {
                let v = gray.get(x, y);
                image::Rgb([v, v, v])
            });
        for (n, r) in panels.iter().enumerate() {
            frame(&mut canvas, r, n + 1);
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string() + ".png";
        canvas
            .save(PathBuf::from(output).join(name))
            .expect("write the marked page");
    }
}

/// A red frame round the panel, and its number as that many dots in the corner.
fn frame(canvas: &mut image::RgbImage, r: &Rect, number: usize) {
    let red = image::Rgb([230, 30, 30]);
    let thick = (canvas.width() / 300).max(3);
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32| {
        for y in y0..(y0 + h).min(canvas.height()) {
            for x in x0..(x0 + w).min(canvas.width()) {
                canvas.put_pixel(x, y, red);
            }
        }
    };
    fill(r.x, r.y, r.w, thick);
    fill(r.x, (r.y + r.h).saturating_sub(thick), r.w, thick);
    fill(r.x, r.y, thick, r.h);
    fill((r.x + r.w).saturating_sub(thick), r.y, thick, r.h);
    let dot = thick * 4;
    for i in 0..number as u32 {
        fill(r.x + thick * 3 + i * dot * 3 / 2, r.y + thick * 3, dot, dot);
    }
}
