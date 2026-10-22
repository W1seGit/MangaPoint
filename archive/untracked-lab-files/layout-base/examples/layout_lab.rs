//! Throwaway lab (not committed): `layout_lab <out dir or -> <W> <H> <strip_cut: quiet|overlap> <chapter dir>...`.
//! Prints one JSON line per chapter with the layout of the converter in this tree.
use converter::decode::decode_to_gray;
use converter::plan::{self, ChapterPlan, PlannedPage, Source};
use converter::{Depth, GrayImage, ProcessOptions, StripCut};
use std::path::Path;
use std::time::Instant;

fn load(dir: &Path) -> Vec<GrayImage> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "img" | "jpg" | "jpeg" | "png" | "webp"))
        })
        .collect();
    files.sort();
    files.iter().filter_map(|p| decode_to_gray(&std::fs::read(p).ok()?).ok()).collect()
}

fn busy(above: &GrayImage, below: &GrayImage) -> f32 {
    let rows = |img: &GrayImage, from: u32, to: u32| -> f32 {
        let mut total = 0.0;
        for y in from..to {
            for x in 1..img.width {
                total += (img.get(x, y) as f32 - img.get(x - 1, y) as f32).abs();
            }
        }
        total / (img.width.max(2) - 1) as f32 / (to - from).max(1) as f32
    };
    (rows(above, above.height.saturating_sub(2), above.height) + rows(below, 0, 2.min(below.height))) / 2.0
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = (args[1] != "-").then(|| args[1].clone());
    let (w, h): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let mut opts = ProcessOptions::manga_default(w, h, Depth::Two);
    opts.guide_arrows = false;
    opts.strip_cut = if args[4] == "overlap" { StripCut::Overlap } else { StripCut::Quiet };
    for dir in &args[5..] {
        let images = load(Path::new(dir));
        if images.is_empty() {
            continue;
        }
        let t = Instant::now();
        let prepared = plan::prepare(&images, &opts);
        let chapter: ChapterPlan = plan::plan(&prepared, &opts);
        let millis = t.elapsed().as_secs_f32() * 1000.0;
        let mut line = format!(
            "{{\"dir\":{:?},\"images\":{},\"strip\":{},\"pages\":{},\"ms\":{:.0}",
            dir.replace('\\', "/"),
            images.len(),
            chapter.strip,
            chapter.pages.len(),
            millis
        );
        let whole = PlannedPage { source: Source::Strip { top: 0, height: u32::MAX / 4 }, unit_top: 0, unit_height: 0, landscape: false };
        let strip = chapter.strip.then(|| plan::original_rows(&prepared, &whole)).flatten();
        if chapter.strip {
            let spans: Vec<(u32, u32)> = chapter
                .pages
                .iter()
                .filter_map(|p| match p.source {
                    Source::Strip { top, height } => Some((top, height)),
                    _ => None,
                })
                .collect();
            let fills: Vec<f32> = spans.iter().map(|&(_, hh)| hh as f32 / h as f32).collect();
            let strip = strip.as_ref().unwrap();
            let mut busys = Vec::new();
            let mut gutters = 0;
            for pair in spans.windows(2) {
                let end = pair[0].0 + pair[0].1;
                if pair[1].0 > end {
                    gutters += 1;
                } else if end >= 3 && end + 3 <= strip.height {
                    let above = converter::process::crop_rect(strip, 0, end - 3, strip.width, 3);
                    let below = converter::process::crop_rect(strip, 0, end, strip.width, 3);
                    busys.push(busy(&above, &below));
                }
            }
            line += &format!(
                ",\"fill_mean\":{:.3},\"short_pages\":{},\"gutter_cuts\":{},\"cuts\":{},\"cut_busy_mean\":{:.2}",
                mean(&fills),
                fills.iter().filter(|&&f| f < 0.6).count(),
                gutters,
                busys.len(),
                mean(&busys)
            );
            let list: Vec<String> = spans.iter().map(|(t, hh)| format!("[{t},{hh}]")).collect();
            line += &format!(",\"spans\":[{}]", list.join(","));

            if let Some(out) = &out {
                let name = Path::new(dir).file_name().unwrap().to_string_lossy().to_string();
                let folder = Path::new(out).join(name);
                std::fs::create_dir_all(&folder).unwrap();
                image::GrayImage::from_raw(strip.width, strip.height, strip.pixels.clone())
                    .unwrap()
                    .save(folder.join("strip.png"))
                    .unwrap();
            }
        }
        line += "}";
        println!("{line}");
    }
}
