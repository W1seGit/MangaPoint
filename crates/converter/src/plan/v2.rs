//! Experimental layout: telling a strip from pages by looking at the seams,
//! one crop for a whole paged chapter, and cutting a strip by a search over
//! the whole chapter rather than one page at a time.

use super::*;

/// The numbers the experiment tunes.
#[derive(Clone, Copy, Debug)]
pub struct V2 {
    /// Paged: the share of pages (percent, 0..=49) whose content may lie
    /// outside the chapter-wide crop box. Those pages are shown whole, a
    /// little smaller, instead of being cut.
    pub box_percentile: u32,
    /// A seam counts as textured when a row beside it varies at least this much.
    pub seam_texture: f32,
    /// A textured seam is continuous when its row-to-row jump is at most this
    /// many times a jump inside the image.
    pub seam_ratio: f32,
    /// Share of textured seams that must be continuous for pages to be a strip.
    pub seam_share: f32,
    /// Strip: cost of a page one screen shorter than full, squared.
    pub fill_weight: f32,
    /// Strip: cost of a cut through a row as busy as `cut_scale`.
    pub cut_weight: f32,
    pub cut_scale: f32,
    /// Strip: the shortest page, as a share of the screen height.
    pub min_fill: f32,
    /// Strip: the last page's shortfall counts this much of a normal page's.
    pub last_weight: f32,
    /// Strip: how much a cut through a drawn thing counts against a cut,
    /// next to how busy the row is (0 leaves it out).
    pub ink_gain: f32,
}

impl Default for V2 {
    fn default() -> Self {
        Self {
            box_percentile: 10,
            seam_texture: 12.0,
            seam_ratio: 2.5,
            seam_share: 0.6,
            fill_weight: 1.0,
            cut_weight: 1.2,
            cut_scale: 12.0,
            min_fill: 0.4,
            last_weight: 0.3,
            ink_gain: 2.0,
        }
    }
}

// ---- Telling a strip from pages -------------------------------------------

/// How one image meets the next.
#[derive(Clone, Copy, Debug)]
pub struct Seam {
    /// A row beside the seam has something drawn on it.
    pub textured: bool,
    /// The jump across the seam, over the jump between rows inside the images.
    pub ratio: f32,
}

fn row_of(img: &GrayImage, y: u32, width: u32) -> Vec<f32> {
    (0..width)
        .map(|x| img.get((x as u64 * img.width as u64 / width as u64) as u32, y) as f32)
        .collect()
}

fn spread(row: &[f32]) -> f32 {
    let mean = row.iter().sum::<f32>() / row.len().max(1) as f32;
    (row.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / row.len().max(1) as f32).sqrt()
}

fn mean_abs(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len().max(1) as f32
}

/// The seam between the bottom of `a` and the top of `b`, if they are about
/// the same width (images of a strip are).
pub fn seam(a: &GrayImage, b: &GrayImage, texture: f32) -> Option<Seam> {
    if a.height < 2 || b.height < 2 || a.width.abs_diff(b.width) * 100 > a.width * 3 {
        return None;
    }
    let w = a.width.min(b.width);
    let (a1, a0) = (row_of(a, a.height - 2, w), row_of(a, a.height - 1, w));
    let (b0, b1) = (row_of(b, 0, w), row_of(b, 1, w));
    let within = (mean_abs(&a1, &a0) + mean_abs(&b0, &b1)) / 2.0;
    let cross = mean_abs(&a0, &b0);
    Some(Seam {
        textured: spread(&a0).max(spread(&b0)) >= texture,
        ratio: cross / (within + 2.0),
    })
}

/// The seams of a chapter's images; `None` where two neighbours differ in width.
pub fn chapter_seams(images: &[GrayImage], p: &V2) -> Vec<Option<Seam>> {
    images
        .windows(2)
        .map(|pair| seam(&pair[0], &pair[1], p.seam_texture))
        .collect()
}

/// `(seams, textured seams, textured seams that are continuous)`.
pub fn seam_evidence(images: &[GrayImage], p: &V2) -> (usize, usize, usize) {
    let seams = chapter_seams(images, p);
    let textured: Vec<&Seam> = seams.iter().flatten().filter(|s| s.textured).collect();
    let continuous = textured.iter().filter(|s| s.ratio <= p.seam_ratio).count();
    (seams.len(), textured.len(), continuous)
}

/// Whether the chapter is one long strip: the old rule on the shape of the
/// images, or (when they are cut short) the pictures running on across seams.
pub fn is_strip_chapter_v2(images: &[GrayImage], opts: &ProcessOptions, p: &V2) -> bool {
    let dims: Vec<(u32, u32)> = images.iter().map(|i| (i.width, i.height)).collect();
    match opts.layout {
        Layout::Webtoon => return true,
        Layout::Manga => return false,
        Layout::Auto => {}
    }
    if is_strip_chapter(&dims, opts) {
        return true;
    }
    if images.len() < 4 {
        return false;
    }
    let (_, textured, continuous) = seam_evidence(images, p);
    textured >= 3 && continuous as f32 >= textured as f32 * p.seam_share
}

// ---- Paged chapters: one crop for all ---------------------------------------

fn percentile(sorted: &[f32], q: u32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * q as f32 / 100.0).round() as usize]
}

/// The rectangle each page is cut to. Pages the size of most of the chapter
/// share one box, wide enough for the content of all but a few of them; a
/// page whose content lies outside it keeps what it needs. Other sizes (a
/// cover, a spread) are cut on their own.
pub fn chapter_boxes(images: &[GrayImage], opts: &ProcessOptions, p: &V2) -> Vec<(u32, u32, u32, u32)> {
    let own: Vec<(u32, u32, u32, u32)> = images.iter().map(|i| crop_bounds(i, opts)).collect();
    if opts.crop == CropMode::Full || images.len() < 4 {
        return own;
    }
    // The commonest size, to within 8 pixels.
    let key = |i: &GrayImage| (i.width / 8, i.height / 8);
    let mut counts: std::collections::HashMap<(u32, u32), usize> = Default::default();
    for i in images {
        *counts.entry(key(i)).or_default() += 1;
    }
    let (&mode, &n) = counts.iter().max_by_key(|&(k, n)| (*n, *k)).unwrap();
    if n < 4 {
        return own;
    }
    let member = |i: &GrayImage| key(i) == mode;
    let mut edges: [Vec<f32>; 4] = Default::default();
    for (img, &(x, y, w, h)) in images.iter().zip(&own).filter(|(i, _)| member(i)) {
        edges[0].push(x as f32 / img.width as f32);
        edges[1].push(y as f32 / img.height as f32);
        edges[2].push((x + w) as f32 / img.width as f32);
        edges[3].push((y + h) as f32 / img.height as f32);
    }
    for e in &mut edges {
        e.sort_by(|a, b| a.total_cmp(b));
    }
    let q = p.box_percentile.min(49);
    let (fx0, fy0) = (percentile(&edges[0], q), percentile(&edges[1], q));
    let (fx1, fy1) = (percentile(&edges[2], 100 - q), percentile(&edges[3], 100 - q));
    images
        .iter()
        .zip(&own)
        .map(|(img, &(x, y, w, h))| {
            if !member(img) {
                return (x, y, w, h);
            }
            let bx0 = ((fx0 * img.width as f32).floor() as u32).min(x);
            let by0 = ((fy0 * img.height as f32).floor() as u32).min(y);
            let bx1 = ((fx1 * img.width as f32).ceil() as u32).max(x + w).min(img.width);
            let by1 = ((fy1 * img.height as f32).ceil() as u32).max(y + h).min(img.height);
            (bx0, by0, (bx1 - bx0).max(1), (by1 - by0).max(1))
        })
        .collect()
}

/// Like [`prepare`], with the seam test and the shared crop.
pub fn prepare_v2(images: &[GrayImage], opts: &ProcessOptions, p: &V2) -> Prepared {
    if is_strip_chapter_v2(images, opts, p) {
        return Prepared {
            kind: Kind::Strip(build_strip(images, opts)),
        };
    }
    let cropped = images
        .iter()
        .zip(chapter_boxes(images, opts, p))
        .map(|(img, (x, y, w, h))| {
            if (x, y, w, h) == (0, 0, img.width, img.height) {
                img.clone()
            } else {
                crop_rect(img, x, y, w, h)
            }
        })
        .collect();
    Prepared {
        kind: Kind::Paged(cropped),
    }
}

/// The sizes of a paged chapter's images after cutting.
pub fn paged_sizes(prepared: &Prepared) -> Option<Vec<(u32, u32)>> {
    match &prepared.kind {
        Kind::Paged(images) => Some(images.iter().map(|i| (i.width, i.height)).collect()),
        Kind::Strip(_) => None,
    }
}

// ---- Strips: the best cuts for the whole chapter ----------------------------

struct Cut {
    /// Where the page before this cut ends.
    ends: u32,
    /// Where the page after it starts. Past `ends` when a gap is skipped.
    starts: u32,
    cost: f32,
}

/// Cut a strip into pages of at most one screen, choosing all the cuts
/// together: each page should be nearly a full screen, and each cut should
/// fall in a blank gap or a quiet row. The best set is found by dynamic
/// programming over the cut positions.
fn plan_strip_v2(strip: &Strip, opts: &ProcessOptions, p: &V2) -> ChapterPlan {
    let screen = opts.height;
    let n = strip.total;
    let min_gutter = (screen / 60).max(4);
    let pad = (screen / 100).clamp(2, 10);

    let width = strip.width.max(1) as f32;
    let activity: Vec<f32> = (0..n)
        .map(|y| {
            if strip.blank.get(y as usize).copied().unwrap_or(false) {
                0.0
            } else {
                row_activity(strip, y) as f32 / width
            }
        })
        .collect();
    // How much of the width a cut above row `y` would run through something
    // drawn, that is continuous across the cut and not the row's own paper.
    let ink: Vec<f32> = (0..n)
        .map(|y| {
            if y == 0 || p.ink_gain <= 0.0 || strip.blank[y as usize] {
                return 0.0;
            }
            let (above, here) = (strip_row(strip, y - 1), strip_row(strip, y));
            let mut hist = [0u32; 256];
            for &v in here {
                hist[v as usize] += 1;
            }
            let (mut seen, mut paper) = (0u32, 0i32);
            for (level, &count) in hist.iter().enumerate() {
                seen += count;
                if seen * 2 >= here.len() as u32 {
                    paper = level as i32;
                    break;
                }
            }
            let through = above
                .iter()
                .zip(here)
                .filter(|&(&a, &b)| {
                    (a as i32 - paper).abs() > 30 && (b as i32 - paper).abs() > 30 && (a as i32 - b as i32).abs() < 48
                })
                .count();
            through as f32 / here.len() as f32
        })
        .collect();
    let smooth = |y: u32| {
        let lo = y.saturating_sub(1) as usize;
        let hi = (y + 1).min(n - 1) as usize;
        activity[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
    };

    // Every row is a place to cut, except inside a gap, where one cut stands
    // for the whole gap and the middle of it is left out.
    let mut cuts = vec![Cut { ends: 0, starts: 0, cost: 0.0 }];
    let mut y = 1;
    while y < n {
        if strip.blank[y as usize] {
            let a = y;
            while y < n && strip.blank[y as usize] {
                y += 1;
            }
            let b = y;
            if b - a >= min_gutter && b < n {
                let ends = (a + pad).min(b);
                cuts.push(Cut { ends, starts: b.saturating_sub(pad).max(ends), cost: 0.0 });
            } else {
                for r in a..b {
                    cuts.push(Cut { ends: r, starts: r, cost: 0.0 });
                }
            }
            continue;
        }
        let harm = (smooth(y) / p.cut_scale).max(ink[y as usize] * p.ink_gain);
        let cost = p.cut_weight * harm.min(1.0);
        cuts.push(Cut { ends: y, starts: y, cost });
        y += 1;
    }
    let last = cuts.len();
    cuts.push(Cut { ends: n, starts: n, cost: 0.0 });

    let full = screen as f32;
    let min_h = (full * p.min_fill) as i64;
    let mut best = vec![f32::INFINITY; cuts.len()];
    let mut from = vec![usize::MAX; cuts.len()];
    best[0] = 0.0;
    for i in 1..cuts.len() {
        let ends = cuts[i].ends as i64;
        let terminal = i == last;
        for j in (0..i).rev() {
            let h = ends - cuts[j].starts as i64;
            if h > screen as i64 {
                break;
            }
            if h <= 0 || best[j].is_infinite() || (h < min_h && !terminal) {
                continue;
            }
            let short = (full - h as f32) / full;
            let weight = if terminal { p.last_weight } else { 1.0 };
            let cost = best[j] + p.fill_weight * weight * short * short + cuts[i].cost;
            if cost < best[i] {
                best[i] = cost;
                from[i] = j;
            }
        }
    }
    if best[last].is_infinite() {
        return plan_strip(strip, opts);
    }

    let mut chain = vec![last];
    while let Some(&i) = chain.last() {
        if i == 0 {
            break;
        }
        chain.push(from[i]);
    }
    chain.reverse();
    let pages = chain
        .windows(2)
        .map(|w| (cuts[w[0]].starts, cuts[w[1]].ends - cuts[w[0]].starts))
        .filter(|&(top, height)| {
            (top..top + height).any(|y| !strip.blank.get(y as usize).copied().unwrap_or(true))
        })
        .map(|(top, height)| PlannedPage {
            source: Source::Strip { top, height },
            unit_top: to_units(top, strip.width),
            unit_height: to_units(height, strip.width),
            landscape: false,
        })
        .collect();
    ChapterPlan {
        strip: true,
        pages,
        unit_height: to_units(strip.total, strip.width),
    }
}

/// Like [`plan`], with the whole-chapter cut for strips.
pub fn plan_v2(prepared: &Prepared, opts: &ProcessOptions, p: &V2) -> ChapterPlan {
    match &prepared.kind {
        Kind::Paged(images) => plan_paged(images, opts),
        Kind::Strip(strip) => plan_strip_v2(strip, opts, p),
    }
}
