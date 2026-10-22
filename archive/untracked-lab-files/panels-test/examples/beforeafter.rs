//! Scratch: converts a folder three ways and writes one contact sheet per page.
//! usage: beforeafter <pages folder> <output folder> [--ltr]
use converter::decode::decode_to_gray;
use converter::gray::Depth;
use converter::process::{PanelView, ProcessOptions};
use converter::xtc::ReadDirection;
use converter::plan::convert_chapter;
use std::path::PathBuf;

fn to_rgb(p: &converter::gray::LevelImage) -> image::RgbImage {
    let max = p.depth.max_level() as u32;
    image::RgbImage::from_fn(p.width, p.height, |x, y| {
        let v = (p.get(x, y) as u32 * 255 / max) as u8;
        image::Rgb([v, v, v])
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ltr = args.iter().any(|a| a == "--ltr");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let (input, output) = (paths[0], paths[1]);
    std::fs::create_dir_all(output).unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(input)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jpg" || e == "png"))
        .collect();
    files.sort();
    for f in files {
        let gray = decode_to_gray(&std::fs::read(&f).unwrap()).unwrap();
        let mut cols: Vec<Vec<image::RgbImage>> = Vec::new();
        let mut counts = Vec::new();
        for view in [PanelView::Off, PanelView::Panels, PanelView::PageThenPanels] {
            let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
            o.direction = if ltr { ReadDirection::LeftToRight } else { ReadDirection::RightToLeft };
            o.panel_view = view;
            let pages = convert_chapter(std::slice::from_ref(&gray), &o);
            counts.push(pages.len());
            cols.push(pages.iter().map(to_rgb).collect());
        }
        // sheet: one row per mode, pages side by side at 240x400 with a gap
        let (cw, ch, gap) = (240u32, 400u32, 10u32);
        let maxn = cols.iter().map(|c| c.len()).max().unwrap() as u32;
        let mut sheet = image::RgbImage::from_pixel(
            maxn * (cw + gap) + gap, 3 * (ch + gap) + gap, image::Rgb([200, 60, 60]));
        for (r, row) in cols.iter().enumerate() {
            for (i, p) in row.iter().enumerate() {
                let small = image::imageops::resize(p, cw, ch, image::imageops::FilterType::Triangle);
                image::imageops::replace(&mut sheet,
                    &small, (gap + i as u32 * (cw + gap)) as i64, (gap + r as u32 * (ch + gap)) as i64);
            }
        }
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        sheet.save(format!("{output}/{name}.png")).unwrap();
        println!("{name}: whole={} panels={} page+panels={}", counts[0], counts[1], counts[2]);
    }
}
