//! Shows the panels the trained model finds on real pages.
//!
//! ```sh
//! cargo run --release -p panel-model --example panels_ai -- <model.onnx> <pages folder> <output folder> [--ltr]
//! ```
//!
//! Each page is written as a PNG with the model's raw frames in blue and the
//! panels made from them (cleaned up, in reading order) in red, numbered by
//! that many dots.

use converter::decode::decode_to_gray;
use converter::process::DEFAULT_TRIM_SENSITIVITY;
use converter::xtc::ReadDirection;
use converter::{panels_from_frames, Rect};
use panel_model::YoloPanels;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let direction = if args.iter().any(|a| a == "--ltr") {
        ReadDirection::LeftToRight
    } else {
        ReadDirection::RightToLeft
    };
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [model, input, output] = paths[..] else {
        eprintln!("usage: panels_ai <model.onnx> <pages folder> <output folder> [--ltr]");
        std::process::exit(2);
    };
    let started = std::time::Instant::now();
    let model = YoloPanels::load(Path::new(model)).expect("load the model");
    println!("model loaded in {:?}", started.elapsed());
    std::fs::create_dir_all(output).expect("create the output folder");
    let mut pages: Vec<PathBuf> = std::fs::read_dir(input)
        .expect("read the pages folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    pages.sort();
    for path in pages {
        let Ok(gray) = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| decode_to_gray(&b).map_err(|e| e.to_string()))
        else {
            continue;
        };
        let started = std::time::Instant::now();
        let frames = model.detect(&gray).expect("run the model");
        let took = started.elapsed();
        let panels = panels_from_frames(&gray, &frames, direction, DEFAULT_TRIM_SENSITIVITY, 0);
        println!(
            "{}: {} frames -> {} panels in {took:?}",
            path.file_name().unwrap().to_string_lossy(),
            frames.len(),
            panels.len()
        );
        let mut canvas = image::RgbImage::from_fn(gray.width, gray.height, |x, y| {
            let v = gray.get(x, y);
            image::Rgb([v, v, v])
        });
        let thick = (gray.width / 300).max(3);
        for r in &frames {
            frame(&mut canvas, r, 0, thick / 2 + 1, image::Rgb([40, 90, 230]));
        }
        for (n, r) in panels.iter().enumerate() {
            frame(&mut canvas, r, n + 1, thick, image::Rgb([230, 30, 30]));
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string() + ".png";
        canvas
            .save(PathBuf::from(output).join(name))
            .expect("write the marked page");
    }
}

fn frame(canvas: &mut image::RgbImage, r: &Rect, number: usize, thick: u32, color: image::Rgb<u8>) {
    let (cw, ch) = (canvas.width(), canvas.height());
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32| {
        for y in y0..(y0 + h).min(ch) {
            for x in x0..(x0 + w).min(cw) {
                canvas.put_pixel(x, y, color);
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
