//! Lab for comparing the chapter layout: `layout_lab <old|new> <out dir or -> <W> <H> <chapter dir>...`.
//! Prints one JSON line per chapter; with an out dir it also writes each page as a PNG.
use converter::decode::decode_to_gray;
use converter::plan::v2::{self, V2};
use converter::plan::{self, Source};
use converter::{Depth, GrayImage, ProcessOptions};
use std::path::Path;
use std::time::Instant;

fn env_f(name: &str, default: f32) -> f32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn params() -> V2 {
    let d = V2::default();
    V2 {
        box_percentile: env_f("Q", d.box_percentile as f32) as u32,
        seam_texture: env_f("SEAM_TEXTURE", d.seam_texture),
        seam_ratio: env_f("SEAM_RATIO", d.seam_ratio),
        seam_share: env_f("SEAM_SHARE", d.seam_share),
        fill_weight: env_f("FILLW", d.fill_weight),
        cut_weight: env_f("CUTW", d.cut_weight),
        cut_scale: env_f("CUTS", d.cut_scale),
        min_fill: env_f("MINFILL", d.min_fill),
        last_weight: env_f("LASTW", d.last_weight),
        ink_gain: env_f("INKG", d.ink_gain),
    }
}

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
    files
        .iter()
        .filter_map(|p| decode_to_gray(&std::fs::read(p).ok()?).ok())
        .collect()
}

/// How much a cut between row-blocks `above` and `below` (three rows each) runs through something drawn.
fn ink_cross(above: &GrayImage, below: &GrayImage) -> f32 {
    let w = above.width.min(below.width);
    let a = above.height - 1;
    let mut around: Vec<u8> = Vec::new();
    for x in 0..w {
        for dy in 0..3.min(above.height) {
            around.push(above.get(x, above.height - 1 - dy));
        }
        for dy in 0..3.min(below.height) {
            around.push(below.get(x, dy));
        }
    }
    around.sort_unstable();
    let bg = around[around.len() / 2] as i32;
    let mut n = 0;
    for x in 0..w {
        let (p, q) = (above.get(x, a) as i32, below.get(x, 0) as i32);
        if (p - bg).abs() > 30 && (q - bg).abs() > 30 && (p - q).abs() < 48 {
            n += 1;
        }
    }
    n as f32 / w as f32
}

/// How busy the rows next to a cut are: the difference between neighbouring pixels, averaged.
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
    let a = rows(above, above.height.saturating_sub(2), above.height);
    let b = rows(below, 0, 2.min(below.height));
    (a + b) / 2.0
}

fn pct(v: &mut Vec<f32>, q: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * q).round() as usize]
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let new = args[1] == "new";
    let out = (args[2] != "-").then(|| args[2].clone());
    let (w, h): (u32, u32) = (args[3].parse().unwrap(), args[4].parse().unwrap());
    let p = params();
    let mut opts = ProcessOptions::manga_default(w, h, Depth::Two);
    opts.guide_arrows = false;
    for dir in &args[5..] {
        let images = load(Path::new(dir));
        if images.is_empty() {
            continue;
        }
        let t = Instant::now();
        let prepared = if new {
            v2::prepare_v2(&images, &opts, &p)
        } else {
            plan::prepare(&images, &opts)
        };
        let chapter = if new { v2::plan_v2(&prepared, &opts, &p) } else { plan::plan(&prepared, &opts) };
        let millis = t.elapsed().as_secs_f32() * 1000.0;

        let mut aspects: Vec<f32> = images.iter().map(|i| i.height as f32 / i.width as f32).collect();
        let aspect = pct(&mut aspects, 0.5);
        let (seams, textured, continuous) = v2::seam_evidence(&images, &p);
        let mut line = format!(
            "{{\"dir\":{:?},\"images\":{},\"strip\":{},\"aspect\":{:.2},\"seams\":{},\"textured\":{},\"continuous\":{},\"pages\":{},\"ms\":{:.0}",
            dir.replace('\\', "/"),
            images.len(),
            chapter.strip,
            aspect,
            seams,
            textured,
            continuous,
            chapter.pages.len(),
            millis
        );

        if chapter.strip {
            let mut fills = Vec::new();
            let mut cuts = Vec::new();
            let mut busys = Vec::new();
            let mut rows = Vec::new();
            for page in &chapter.pages {
                if let Source::Strip { height, .. } = page.source {
                    fills.push(height as f32 / h as f32);
                }
                rows.push(plan::original_rows(&prepared, page).unwrap());
            }
            for (i, pair) in chapter.pages.windows(2).enumerate() {
                if let (Source::Strip { top: t0, height: h0 }, Source::Strip { top: t1, .. }) =
                    (pair[0].source, pair[1].source)
                {
                    if t0 + h0 == t1 {
                        cuts.push(ink_cross(&rows[i], &rows[i + 1]));
                        busys.push(busy(&rows[i], &rows[i + 1]));
                    }
                }
            }
            let gutter = chapter
                .pages
                .windows(2)
                .filter(|pair| match (pair[0].source, pair[1].source) {
                    (Source::Strip { top: t0, height: h0 }, Source::Strip { top: t1, .. }) => t0 + h0 != t1,
                    _ => false,
                })
                .count();
            let bad = cuts.iter().filter(|&&c| c > 0.25).count();
            let low = fills.iter().filter(|&&f| f < 0.6).count();
            line += &format!(
                ",\"fill_mean\":{:.3},\"fill_min\":{:.3},\"short_pages\":{},\"gutter_cuts\":{},\"cuts\":{},\"cut_ink_mean\":{:.3},\"cut_ink_p90\":{:.3},\"bad_cuts\":{},\"cut_busy_mean\":{:.2}",
                mean(&fills),
                fills.iter().cloned().fold(1.0, f32::min),
                low,
                gutter,
                cuts.len(),
                mean(&cuts),
                pct(&mut cuts.clone(), 0.9),
                bad,
                mean(&busys)
            );
        } else {
            let sizes = v2::paged_sizes(&prepared).unwrap();
            let mut scales = Vec::new();
            let mut fills = Vec::new();
            let mut seam_ink = Vec::new();
            let key = |i: &GrayImage| (i.width / 8, i.height / 8);
            let mut counts: std::collections::HashMap<(u32, u32), usize> = Default::default();
            for i in &images {
                *counts.entry(key(i)).or_default() += 1;
            }
            let mode = *counts.iter().max_by_key(|&(k, n)| (*n, *k)).unwrap().0;
            for (img, &(cw, ch)) in images.iter().zip(&sizes) {
                if key(img) != mode {
                    continue;
                }
                let s = (w as f32 / cw as f32).min(h as f32 / ch as f32);
                scales.push(s);
                fills.push(cw as f32 * s * ch as f32 * s / (w as f32 * h as f32));
            }
            for pair in images.windows(2) {
                if pair[0].width.abs_diff(pair[1].width) * 100 <= pair[0].width * 3 {
                    let a = &pair[0];
                    let b = &pair[1];
                    let above = converter::process::crop_rect(a, 0, a.height.saturating_sub(3), a.width, 3.min(a.height));
                    let below = converter::process::crop_rect(b, 0, 0, b.width, 3.min(b.height));
                    seam_ink.push(ink_cross(&above, &below));
                }
            }
            let m = mean(&scales);
            let sd = (scales.iter().map(|s| (s - m).powi(2)).sum::<f32>() / scales.len().max(1) as f32).sqrt();
            line += &format!(
                ",\"scale_mean\":{:.4},\"scale_cv\":{:.4},\"fill_mean\":{:.3},\"seam_ink_mean\":{:.3},\"seam_ink_p90\":{:.3}",
                m,
                sd / m.max(1e-6),
                mean(&fills),
                mean(&seam_ink),
                pct(&mut seam_ink.clone(), 0.9)
            );
        }
        if out.is_some() && chapter.strip {
            let spans: Vec<String> = chapter
                .pages
                .iter()
                .filter_map(|p| match p.source {
                    Source::Strip { top, height } => Some(format!("[{top},{height}]")),
                    _ => None,
                })
                .collect();
            line += &format!(",\"spans\":[{}]", spans.join(","));
            if let Some((_, starts)) = v2::strip_image(&prepared) {
                let starts: Vec<String> = starts.iter().map(|s| s.to_string()).collect();
                line += &format!(",\"slice_starts\":[{}]", starts.join(","));
            }
        }
        line += "}";
        println!("{line}");

        if let Some(out) = &out {
            let name = Path::new(dir).file_name().unwrap().to_string_lossy().to_string();
            let folder = Path::new(out).join(name);
            std::fs::create_dir_all(&folder).unwrap();
            if let Some((strip, _)) = v2::strip_image(&prepared) {
                image::GrayImage::from_raw(strip.width, strip.height, strip.pixels.clone())
                    .unwrap()
                    .save(folder.join("strip.png"))
                    .unwrap();
            }
            for (i, page) in chapter.pages.iter().enumerate() {
                let level = plan::render_page(&prepared, page, &opts);
                let max = level.depth.max_level() as u32;
                let pixels: Vec<u8> = level.levels.iter().map(|&l| (l as u32 * 255 / max) as u8).collect();
                image::GrayImage::from_raw(level.width, level.height, pixels)
                    .unwrap()
                    .save(folder.join(format!("{i:04}.png")))
                    .unwrap();
            }
        }
    }
}
