//! Planning a chapter's device pages before any are rendered.
//!
//! The old conversion looked at one downloaded image at a time. That cuts a
//! webtoon that arrives as many short images at the seams and gives the user
//! nothing to correct. Here a chapter is first *prepared* (cropped, and for a
//! webtoon stitched into one continuous strip), then *planned* (a list of the
//! pages it becomes), and only then *rendered*. The plan is plain data, so a
//! screen can show it next to the downloaded pages.
//!
//! * Paged chapters: one page per image. A double-page spread is split at its
//!   spine, rotated, or fitted, as the settings say.
//! * Webtoon chapters: every image is scaled to the screen width and joined.
//!   Pages are cut at the blank gutters between scenes when there is one near
//!   the bottom of the screen. When there is not, the cut is made at the
//!   quietest row near the bottom (or, in the older way, at the full screen
//!   height with the last rows repeated on the next page).
//!
//! Positions in a strip are also given in *units* (the strip scaled to 1000
//! wide), which do not depend on the device.

use crate::gray::{GrayImage, LevelImage};
use crate::process::{
    border_median, clamp_rect, crop_bounds, crop_rect, finish_guided, letterbox_top, resize, rotate_cw, show,
    tile, tile_arrow, tile_order, trim_bounds, trim_threshold, zoom_grid, Arrow, CropMode, Layout,
    Margins, ProcessOptions, SpreadMode, StripCut, ZoomMode,
};
use crate::xtc::ReadDirection;

/// The width a strip has in unit coordinates.
pub const UNIT_WIDTH: u32 = 1000;

/// A webtoon chapter is recognised when its images are, typically, at least
/// this many times taller than wide.
const STRIP_ASPECT: f32 = 2.0;

/// Which part of a paged image a page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Whole,
    /// Turned a quarter turn clockwise to use the screen's full height.
    Rotated,
    /// A physical spread half: 0 is left, 1 is right, independent of reading direction.
    Half(u8),
    /// One screen of a zoomed-in piece: its column and row counted from the
    /// left and top of the zoomed piece, whatever the reading direction.
    Tile { of: Piece, col: u16, row: u16 },
}

/// The part of an image that a zoom is applied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Piece {
    Whole,
    Rotated,
    Half(u8),
}

/// The piece of `img` that `part` shows, before it is scaled to the screen.
fn piece(img: &GrayImage, of: Piece) -> GrayImage {
    match of {
        Piece::Whole => img.clone(),
        Piece::Rotated => rotate_cw(img),
        Piece::Half(n) => {
            let mid = spine(img);
            if n == 1 {
                crop_rect(img, mid, 0, img.width - mid, img.height)
            } else {
                crop_rect(img, 0, 0, mid, img.height)
            }
        }
    }
}

/// How big a piece is, without cutting it out.
fn piece_size(img: &GrayImage, of: Piece) -> (u32, u32) {
    match of {
        Piece::Whole => (img.width, img.height),
        Piece::Rotated => (img.height, img.width),
        Piece::Half(n) => {
            let mid = spine(img);
            (if n == 1 { img.width - mid } else { mid }, img.height)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Image {
        index: usize,
        part: Part,
    },
    /// Rows of the stitched strip, in device pixels.
    Strip {
        top: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPage {
    pub source: Source,
    /// Where the page starts in the strip, in units. 0 for paged chapters.
    pub unit_top: u32,
    /// How tall the page's slice of the strip is, in units. 0 for paged
    /// chapters.
    pub unit_height: u32,
    /// Paged: the image is a landscape double page.
    pub landscape: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterPlan {
    /// A stitched webtoon strip rather than one page per image.
    pub strip: bool,
    pub pages: Vec<PlannedPage>,
    /// Total strip height in units; 0 for paged chapters.
    pub unit_height: u32,
}

/// A chapter, cropped and (for a webtoon) stitched, ready to plan and render.
pub struct Prepared {
    kind: Kind,
}

enum Kind {
    Paged(Vec<GrayImage>),
    Strip(Strip),
}

struct Strip {
    width: u32,
    /// Each image scaled to `width`, top to bottom.
    parts: Vec<GrayImage>,
    /// First row of each part.
    starts: Vec<u32>,
    total: u32,
    /// Per row: nothing but background.
    blank: Vec<bool>,
    /// Some of the source was left out to stay within the row limits.
    truncated: bool,
}

impl Prepared {
    pub fn is_strip(&self) -> bool {
        matches!(self.kind, Kind::Strip(_))
    }

    /// A webtoon strip had to leave out part of its pictures to stay within
    /// [`MAX_IMAGE_ROWS`] and [`MAX_STRIP_ROWS`].
    pub fn truncated(&self) -> bool {
        matches!(&self.kind, Kind::Strip(strip) if strip.truncated)
    }

    /// The number of source images.
    pub fn image_count(&self) -> usize {
        match &self.kind {
            Kind::Paged(images) => images.len(),
            Kind::Strip(strip) => strip.parts.len(),
        }
    }
}

/// Whether `opts.layout` makes this chapter a stitched strip.
pub fn is_strip_chapter(images: &[(u32, u32)], opts: &ProcessOptions) -> bool {
    match opts.layout {
        Layout::Webtoon => true,
        Layout::Manga => false,
        Layout::Auto => {
            if images.is_empty() {
                return false;
            }
            let mut ratios: Vec<f32> = images
                .iter()
                .map(|&(w, h)| h as f32 / w.max(1) as f32)
                .collect();
            ratios.sort_by(|a, b| a.total_cmp(b));
            ratios[ratios.len() / 2] >= STRIP_ASPECT
        }
    }
}

/// The most rows one image may add to a strip once scaled to the screen
/// width. A 100 x 1,000,000 pixel image is within the decoder's limit but
/// would become millions of rows; what lies below this is left out.
pub const MAX_IMAGE_ROWS: u32 = 150_000;

/// The most rows a whole strip may have. Images that would take it past this
/// are cut short, and later ones left out.
pub const MAX_STRIP_ROWS: u32 = 600_000;

/// A chapter's work was stopped by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the conversion was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Asked between pages and between images: has the caller given up? This
/// crate knows nothing of the app's cancel flag, so it is a plain function.
pub type CancelCheck<'a> = &'a (dyn Fn() -> bool + Sync);

fn never() -> bool {
    false
}

/// Run `work(0..count)` on every core and give the results back in order.
/// `Err` as soon as `cancel` says so; the jobs not yet begun are not run.
fn par_map<T: Send>(
    count: usize,
    cancel: CancelCheck,
    work: impl Fn(usize) -> T + Sync,
) -> Result<Vec<T>, Cancelled> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(count);
        if workers > 1 {
            let next = AtomicUsize::new(0);
            let stopped = AtomicBool::new(false);
            let mut slots: Vec<Option<T>> = (0..count).map(|_| None).collect();
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..workers)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut done = Vec::new();
                            loop {
                                if stopped.load(Ordering::Relaxed) {
                                    break;
                                }
                                if cancel() {
                                    stopped.store(true, Ordering::Relaxed);
                                    break;
                                }
                                let i = next.fetch_add(1, Ordering::Relaxed);
                                if i >= count {
                                    break;
                                }
                                done.push((i, work(i)));
                            }
                            done
                        })
                    })
                    .collect();
                for handle in handles {
                    for (i, value) in handle.join().expect("a conversion worker panicked") {
                        slots[i] = Some(value);
                    }
                }
            });
            if stopped.load(Ordering::Relaxed) {
                return Err(Cancelled);
            }
            return Ok(slots
                .into_iter()
                .map(|slot| slot.expect("every job was run"))
                .collect());
        }
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        if cancel() {
            return Err(Cancelled);
        }
        out.push(work(i));
    }
    Ok(out)
}

/// Crop and, for a webtoon, stitch. `images` are the chapter's pages in order.
/// Copies them; [`prepare_owned`] takes them instead, so a big chapter does
/// not exist twice.
pub fn prepare(images: &[GrayImage], opts: &ProcessOptions) -> Prepared {
    prepare_owned(images.to_vec(), opts, &never).expect("nothing cancels it")
}

/// [`prepare`], consuming the images: each is freed as soon as its cropped or
/// scaled copy exists. `Err` if `cancel` turns true while it works.
pub fn prepare_owned(
    images: Vec<GrayImage>,
    opts: &ProcessOptions,
    cancel: CancelCheck,
) -> Result<Prepared, Cancelled> {
    let dims: Vec<(u32, u32)> = images.iter().map(|i| (i.width, i.height)).collect();
    if is_strip_chapter(&dims, opts) {
        Ok(Prepared {
            kind: Kind::Strip(build_strip(images, opts, cancel)?),
        })
    } else {
        let mut cropped = Vec::with_capacity(images.len());
        for img in images {
            if cancel() {
                return Err(Cancelled);
            }
            let (x, y, w, h) = crop_bounds(&img, opts);
            cropped.push(if (x, y, w, h) == (0, 0, img.width, img.height) {
                img
            } else {
                crop_rect(&img, x, y, w, h)
            });
        }
        Ok(Prepared {
            kind: Kind::Paged(cropped),
        })
    }
}

/// What to do with one image of a strip: the part of it that is kept, as a
/// rectangle of the downloaded picture, and how tall it is once scaled.
struct StripJob {
    rect: (u32, u32, u32, u32),
    scaled_h: u32,
}

fn build_strip(
    images: Vec<GrayImage>,
    opts: &ProcessOptions,
    cancel: CancelCheck,
) -> Result<Strip, Cancelled> {
    let side = |p: u8| p.min(Margins::MAX_PERCENT) as u64;
    let cut = |len: u32, p: u8| (len as u64 * side(p) / 100) as u32;
    let threshold = trim_threshold(opts.trim_sensitivity);

    // Fixed margins first. Top and bottom apply to every image (a banner on
    // each slice); left and right are the same cut everywhere. Only the
    // rectangle is worked out here; nothing is copied.
    let fixed: Vec<(u32, u32, u32, u32)> = images
        .iter()
        .map(|img| {
            let x = cut(img.width, opts.margins.left);
            let y = cut(img.height, opts.margins.top);
            let w = img
                .width
                .saturating_sub(x + cut(img.width, opts.margins.right))
                .max(1);
            let h = img
                .height
                .saturating_sub(y + cut(img.height, opts.margins.bottom))
                .max(1);
            clamp_rect((img.width, img.height), x, y, w, h)
        })
        .collect();

    // Blank side margins: one cut for the whole chapter, so every image keeps
    // the same width and the strip does not jitter sideways.
    let (left, right) = if opts.crop == CropMode::Margins && !images.is_empty() {
        let found = par_map(images.len(), cancel, |i| {
            let (x, y, w, h) = fixed[i];
            let img = &images[i];
            let bounds = if (x, y, w, h) == (0, 0, img.width, img.height) {
                trim_bounds(img, threshold, opts.trim_padding)
            } else {
                trim_bounds(&crop_rect(img, x, y, w, h), threshold, opts.trim_padding)
            };
            // The fractions of the fixed-margin picture that hold content.
            match bounds {
                Some((tx, _, tw, _)) => (tx as f32 / w as f32, (tx + tw) as f32 / w as f32),
                None => (0.0, 1.0),
            }
        })?;
        let mut left = 1.0f32;
        let mut right = 0.0f32;
        for (l, r) in found {
            left = left.min(l);
            right = right.max(r);
        }
        if right > left {
            (left, right)
        } else {
            (0.0, 1.0)
        }
    } else {
        (0.0, 1.0)
    };

    // How much of each image is kept and how tall it comes out, with the
    // limits on the rows of one image and of the whole strip. Images are
    // cut short rather than allocated at absurd sizes.
    let width = opts.width.max(1);
    let mut jobs: Vec<Option<StripJob>> = Vec::with_capacity(images.len());
    let mut budget = MAX_STRIP_ROWS as u64;
    let mut truncated = false;
    for &(fx, fy, fw, fh) in &fixed {
        let x = ((left * fw as f32).floor() as u32).min(fw - 1);
        let w = (((right - left) * fw as f32).round() as u32).clamp(1, fw - x);
        let rows = |h: u32| (h as f64 * width as f64 / w as f64).round().max(1.0);
        let allowed = (MAX_IMAGE_ROWS as u64).min(budget);
        if allowed == 0 {
            truncated = true;
            jobs.push(None);
            continue;
        }
        let mut h = fh;
        if rows(h) > allowed as f64 {
            // Keep the top of the image: as many source rows as fit.
            h = ((allowed as f64 * w as f64 / width as f64).floor() as u32).clamp(1, fh);
            truncated = true;
        }
        let scaled_h = (rows(h) as u64).min(allowed).max(1) as u32;
        budget -= scaled_h as u64;
        jobs.push(Some(StripJob {
            rect: (fx + x, fy, w, h),
            scaled_h,
        }));
    }

    // Scale the images side by side, each freed as it is taken. The order of
    // the rows does not depend on which finishes first.
    let slots: Vec<std::sync::Mutex<Option<GrayImage>>> = images
        .into_iter()
        .map(|img| std::sync::Mutex::new(Some(img)))
        .collect();
    let scaled = par_map(slots.len(), cancel, |i| {
        let job = jobs[i].as_ref()?;
        let img = slots[i].lock().expect("no panic holds the lock").take()?;
        let (x, y, w, h) = job.rect;
        let sideways = if (x, y, w, h) == (0, 0, img.width, img.height) {
            img
        } else {
            crop_rect(&img, x, y, w, h)
        };
        let scaled = if (sideways.width, sideways.height) == (width, job.scaled_h) {
            sideways
        } else {
            resize(&sideways, width, job.scaled_h)
        };
        let bg = border_median(&scaled) as i16;
        let row_limit = (width / 200).max(1) as usize;
        let blank: Vec<bool> = scaled
            .pixels
            .chunks_exact(width as usize)
            .map(|row| {
                row.iter()
                    .filter(|&&p| (p as i16 - bg).abs() > threshold)
                    .count()
                    <= row_limit
            })
            .collect();
        Some((scaled, blank))
    })?;

    let mut parts = Vec::with_capacity(scaled.len());
    let mut starts = Vec::with_capacity(scaled.len());
    let mut blank = Vec::new();
    let mut total = 0u32;
    for (scaled, rows_blank) in scaled.into_iter().flatten() {
        starts.push(total);
        total += scaled.height;
        blank.extend(rows_blank);
        parts.push(scaled);
    }
    Ok(Strip {
        width,
        parts,
        starts,
        total,
        blank,
        truncated,
    })
}

fn to_units(px: u32, width: u32) -> u32 {
    (px as u64 * UNIT_WIDTH as u64 / width.max(1) as u64) as u32
}

/// Decide the pages of a prepared chapter.
pub fn plan(prepared: &Prepared, opts: &ProcessOptions) -> ChapterPlan {
    match &prepared.kind {
        Kind::Paged(images) => plan_paged(images, opts),
        Kind::Strip(strip) => plan_strip(strip, opts),
    }
}

fn plan_paged(images: &[GrayImage], opts: &ProcessOptions) -> ChapterPlan {
    let mut pages = Vec::new();
    for (index, img) in images.iter().enumerate() {
        let landscape = img.width as f32 > img.height as f32 * 1.1;
        let parts: &[Part] = if !landscape {
            &[Part::Whole]
        } else {
            match opts.spread {
                SpreadMode::Fit => &[Part::Whole],
                SpreadMode::Rotate => &[Part::Rotated],
                SpreadMode::Split if opts.direction == ReadDirection::RightToLeft => {
                    &[Part::Half(1), Part::Half(0)]
                }
                SpreadMode::Split => &[Part::Half(0), Part::Half(1)],
            }
        };
        for &part in parts {
            // Zoomed in past Fit, a piece takes several screens, listed in
            // reading order. A piece that still fits one screen is as before.
            if !opts.zoom.is_fit() && opts.zoom_mode == ZoomMode::Tile {
                let of = match part {
                    Part::Rotated => Piece::Rotated,
                    Part::Half(n) => Piece::Half(n),
                    _ => Piece::Whole,
                };
                let (w, h) = piece_size(img, of);
                let grid = zoom_grid(w, h, opts);
                if grid.cols * grid.rows > 1 {
                    for (col, row) in tile_order(grid.cols, grid.rows, opts.direction) {
                        pages.push(PlannedPage {
                            source: Source::Image {
                                index,
                                part: Part::Tile {
                                    of,
                                    col: col as u16,
                                    row: row as u16,
                                },
                            },
                            unit_top: 0,
                            unit_height: 0,
                            landscape,
                        });
                    }
                    continue;
                }
            }
            pages.push(PlannedPage {
                source: Source::Image { index, part },
                unit_top: 0,
                unit_height: 0,
                landscape,
            });
        }
    }
    ChapterPlan {
        strip: false,
        pages,
        unit_height: 0,
    }
}

fn plan_strip(strip: &Strip, opts: &ProcessOptions) -> ChapterPlan {
    let screen = opts.height;
    let overlap = screen * opts.overlap_percent.min(50) as u32 / 100;
    let min_fill = screen * 55 / 100;
    let min_gutter = (screen / 60).max(4);
    let pad = (screen / 100).clamp(2, 10);

    let mut spans: Vec<(u32, u32)> = Vec::new(); // top, height
    let mut start = 0u32;
    while start < strip.total {
        let remaining = strip.total - start;
        if remaining <= screen {
            spans.push((start, remaining));
            break;
        }
        let ideal_end = start + screen;
        // The latest blank gutter in the lower part of the screen.
        let (end, next) = match latest_gutter(strip, start + min_fill, ideal_end, min_gutter) {
            Some((a, b)) => {
                let end = (a + pad).min(ideal_end);
                (end, b.saturating_sub(pad).max(end))
            }
            None => match opts.strip_cut {
                // The quietest row near the bottom; nothing is repeated.
                StripCut::Quiet => {
                    let cut = quietest_row(strip, start + min_fill, ideal_end);
                    (cut, cut)
                }
                StripCut::Overlap => (ideal_end, ideal_end - overlap),
            },
        };
        spans.push((start, end - start));
        start = next.max(start + 1);
    }

    let pages = spans
        .into_iter()
        .filter(|&(top, height)| {
            // Pages that are only background are dropped: trailing whitespace.
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

/// One row of the strip.
fn strip_row(strip: &Strip, y: u32) -> &[u8] {
    let part = strip.starts.partition_point(|&first| first <= y).max(1) - 1;
    let image = &strip.parts[part];
    let width = strip.width as usize;
    let row = (y - strip.starts[part]) as usize;
    &image.pixels[row * width..(row + 1) * width]
}

/// How much is drawn on row `y`: the difference between neighbouring pixels,
/// added up. A flat row, a straight panel border or a blank gap scores near
/// nothing; a row through lettering or a face scores a lot.
fn row_activity(strip: &Strip, y: u32) -> u32 {
    strip_row(strip, y)
        .windows(2)
        .map(|pair| (pair[0] as i32 - pair[1] as i32).unsigned_abs())
        .sum()
}

/// Where to cut a page that has no blank gap: the quietest row in
/// `[from, to)`, which is the row the next page starts with. Rows nearly as
/// quiet as the quietest are all as good, so the latest of them is taken, to
/// keep the page as full as it can be.
fn quietest_row(strip: &Strip, from: u32, to: u32) -> u32 {
    let to = to.min(strip.total);
    if from + 1 >= to {
        return to;
    }
    // Each row's own activity and its neighbours', so a single quiet row
    // between two lines of lettering does not win.
    let activity: Vec<f32> = (from.saturating_sub(1)..=to.min(strip.total - 1))
        .map(|y| row_activity(strip, y) as f32 / strip.width.max(1) as f32)
        .collect();
    let base = from.saturating_sub(1);
    let at = |y: u32| (y - base) as usize;
    let cost = |y: u32| {
        let (a, b, c) = (activity[at(y) - 1], activity[at(y)], activity[at(y) + 1]);
        (a + b + c) / 3.0
    };
    let lowest = (from..to - 1).map(cost).fold(f32::MAX, f32::min);
    let allowed = lowest + 0.5 + lowest * 0.25;
    (from..to - 1)
        .rev()
        .find(|&y| cost(y) <= allowed)
        .unwrap_or(to)
}

/// The last run of at least `min_len` blank rows that lies inside
/// `[from, to)`, as `(first_row, one_past_last_row)`.
fn latest_gutter(strip: &Strip, from: u32, to: u32, min_len: u32) -> Option<(u32, u32)> {
    let to = to.min(strip.total);
    let mut best = None;
    let mut run_start: Option<u32> = None;
    for y in from..=to {
        let blank = y < to && strip.blank.get(y as usize).copied().unwrap_or(false);
        match (blank, run_start) {
            (true, None) => run_start = Some(y),
            (false, Some(a)) => {
                if y - a >= min_len {
                    best = Some((a, y));
                }
                run_start = None;
            }
            _ => {}
        }
    }
    best
}

/// A piece of a source image: the image itself, or one cut out of it that
/// the screens of a zoomed page share.
enum PieceRef<'a> {
    Borrowed(&'a GrayImage),
    Shared(std::sync::Arc<GrayImage>),
}

impl std::ops::Deref for PieceRef<'_> {
    type Target = GrayImage;
    fn deref(&self) -> &GrayImage {
        match self {
            PieceRef::Borrowed(img) => img,
            PieceRef::Shared(img) => img,
        }
    }
}

impl PieceRef<'_> {
    fn into_owned(self) -> GrayImage {
        match self {
            PieceRef::Borrowed(img) => img.clone(),
            PieceRef::Shared(img) => std::sync::Arc::try_unwrap(img).unwrap_or_else(|img| (*img).clone()),
        }
    }
}

struct CachedPiece {
    piece: std::sync::OnceLock<std::sync::Arc<GrayImage>>,
    /// Screens that still need it; the last one lets it go.
    left: std::sync::atomic::AtomicUsize,
}

/// The rotated or half pieces that several screens of one zoomed page are cut
/// from, made once however many screens (and threads) use them, and dropped
/// when the last screen has.
struct PieceCache {
    pieces: std::sync::Mutex<std::collections::HashMap<(usize, Piece), std::sync::Arc<CachedPiece>>>,
}

impl PieceCache {
    /// For the pages about to be rendered; `None` if none shares a piece.
    fn for_pages(pages: &[PlannedPage]) -> Option<PieceCache> {
        let mut counts: std::collections::HashMap<(usize, Piece), usize> = Default::default();
        for page in pages {
            if let Source::Image {
                index,
                part: Part::Tile { of, .. },
            } = page.source
            {
                if of != Piece::Whole {
                    *counts.entry((index, of)).or_default() += 1;
                }
            }
        }
        counts.retain(|_, n| *n > 1);
        (!counts.is_empty()).then(|| PieceCache {
            pieces: std::sync::Mutex::new(
                counts
                    .into_iter()
                    .map(|(key, n)| {
                        (
                            key,
                            std::sync::Arc::new(CachedPiece {
                                piece: Default::default(),
                                left: std::sync::atomic::AtomicUsize::new(n),
                            }),
                        )
                    })
                    .collect(),
            ),
        })
    }

    fn get(&self, img: &GrayImage, index: usize, of: Piece) -> Option<std::sync::Arc<GrayImage>> {
        let entry = self.pieces.lock().expect("no panic holds the lock").get(&(index, of))?.clone();
        let piece = entry.piece.get_or_init(|| std::sync::Arc::new(piece(img, of))).clone();
        if entry.left.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
            self.pieces.lock().expect("no panic holds the lock").remove(&(index, of));
        }
        Some(piece)
    }
}

/// The piece of source image `index` that screens of a zoomed page are cut from.
fn piece_of<'a>(
    img: &'a GrayImage,
    index: usize,
    of: Piece,
    cache: Option<&PieceCache>,
) -> PieceRef<'a> {
    if of == Piece::Whole {
        return PieceRef::Borrowed(img);
    }
    PieceRef::Shared(
        cache
            .and_then(|c| c.get(img, index, of))
            .unwrap_or_else(|| std::sync::Arc::new(piece(img, of))),
    )
}

/// Render one planned page to device pixels and levels.
pub fn render_page(prepared: &Prepared, page: &PlannedPage, opts: &ProcessOptions) -> LevelImage {
    render_page_with(prepared, page, opts, None)
}

fn render_page_with(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
    cache: Option<&PieceCache>,
) -> LevelImage {
    let screen = match (&prepared.kind, page.source) {
        (Kind::Strip(strip), Source::Strip { top, height }) => {
            letterbox_top(&strip_rows(strip, top, height), opts.width, opts.height)
        }
        (Kind::Paged(images), Source::Image { index, part }) => {
            paged_screen(&images[index], index, part, opts, cache)
        }
        _ => unreachable!("a plan is only rendered with the chapter it was made from"),
    };
    finish_guided(screen, opts, page_arrow(prepared, page, opts))
}

/// The guide arrow this page gets, when they are on: the first half of a
/// split double page points to the other half, and a screen of a zoomed page
/// to the next screen. The last page of a source page has none.
fn page_arrow(prepared: &Prepared, page: &PlannedPage, opts: &ProcessOptions) -> Option<Arrow> {
    if !opts.guide_arrows {
        return None;
    }
    let (Kind::Paged(images), Source::Image { index, part }) = (&prepared.kind, page.source) else {
        return None;
    };
    // The half of a double page that is read first.
    let first_half = |n: u8| (n == 1) == (opts.direction == ReadDirection::RightToLeft);
    match part {
        Part::Whole | Part::Rotated => None,
        Part::Half(n) => first_half(n).then(|| crate::process::across(opts.direction)),
        Part::Tile { of, col, row } => {
            let (w, h) = piece_size(&images[index], of);
            let grid = zoom_grid(w, h, opts);
            let continues = matches!(of, Piece::Half(n) if first_half(n));
            tile_arrow(grid.cols, grid.rows, (col as u32, row as u32), continues, opts.direction)
        }
    }
}

fn paged_screen(
    img: &GrayImage,
    index: usize,
    part: Part,
    opts: &ProcessOptions,
    cache: Option<&PieceCache>,
) -> GrayImage {
    match part {
        Part::Whole => show(img, opts),
        Part::Rotated => show(&rotate_cw(img), opts),
        Part::Half(_) => show(&piece(img, half_of(part)), opts),
        Part::Tile { of, col, row } => {
            tile(&piece_of(img, index, of, cache), opts, col as u32, row as u32)
        }
    }
}

fn half_of(part: Part) -> Piece {
    match part {
        Part::Half(n) => Piece::Half(n),
        _ => Piece::Whole,
    }
}

/// Where to cut a double page: the blank column nearest the middle, or the
/// middle itself when the art runs across the fold.
fn spine(img: &GrayImage) -> u32 {
    let (w, h) = (img.width, img.height);
    let centre = w / 2;
    let bg = border_median(img);
    let threshold = trim_threshold(crate::process::DEFAULT_TRIM_SENSITIVITY);
    let limit = (h / 200).max(1);
    let blank = |x: u32| {
        (0..h)
            .filter(|&y| (img.get(x, y) as i16 - bg as i16).abs() > threshold)
            .count() as u32
            <= limit
    };
    let lo = w * 2 / 5;
    let hi = (w * 3 / 5).min(w - 1);
    let mut best: Option<(u32, u32)> = None; // (distance of run middle, middle)
    let mut run: Option<u32> = None;
    for x in lo..=hi + 1 {
        let b = x <= hi && blank(x);
        match (b, run) {
            (true, None) => run = Some(x),
            (false, Some(a)) => {
                let mid = (a + x) / 2;
                let d = mid.abs_diff(centre);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, mid));
                }
                run = None;
            }
            _ => {}
        }
    }
    best.map_or(centre, |(_, m)| m)
        .clamp(1, w.saturating_sub(1).max(1))
}

/// Rows `[top, top + height)` of the strip.
fn strip_rows(strip: &Strip, top: u32, height: u32) -> GrayImage {
    let height = height.min(strip.total.saturating_sub(top)).max(1);
    let mut pixels = Vec::with_capacity(strip.width as usize * height as usize);
    let end = top + height;
    for (part, &first) in strip.parts.iter().zip(&strip.starts) {
        let last = first + part.height;
        if last <= top || first >= end {
            continue;
        }
        let from = top.max(first) - first;
        let to = end.min(last) - first;
        let w = strip.width as usize;
        pixels.extend_from_slice(&part.pixels[from as usize * w..to as usize * w]);
    }
    GrayImage::from_pixels(strip.width, height, pixels).expect("rows from the strip fit its size")
}

/// A strip page's rows as downloaded (scaled to the screen width, tones
/// untouched), to show beside the converted page. `None` for a paged chapter.
pub fn original_rows(prepared: &Prepared, page: &PlannedPage) -> Option<GrayImage> {
    match (&prepared.kind, page.source) {
        (Kind::Strip(strip), Source::Strip { top, height }) => Some(strip_rows(strip, top, height)),
        _ => None,
    }
}

/// Plan and render a whole chapter.
pub fn convert_chapter(images: &[GrayImage], opts: &ProcessOptions) -> Vec<LevelImage> {
    convert_chapter_owned(images.to_vec(), opts, &never).expect("nothing cancels it")
}

/// [`convert_chapter`], consuming the images (see [`prepare_owned`]) and
/// giving up with `Err` soon after `cancel` turns true.
pub fn convert_chapter_owned(
    images: Vec<GrayImage>,
    opts: &ProcessOptions,
    cancel: CancelCheck,
) -> Result<Vec<LevelImage>, Cancelled> {
    let prepared = prepare_owned(images, opts, cancel)?;
    let plan = plan(&prepared, opts);
    render_all_cropped_cancellable(&prepared, &plan.pages, opts, &[], cancel)
}

/// Render pages, using every core of the machine: each page is independent,
/// and resizing, contrast and dithering are most of an export's time. The
/// pages come back in order.
pub fn render_all_cropped(
    prepared: &Prepared,
    pages: &[PlannedPage],
    opts: &ProcessOptions,
    crops: &[[f32; 4]],
) -> Vec<LevelImage> {
    render_all_cropped_cancellable(prepared, pages, opts, crops, &never)
        .expect("nothing cancels it")
}

/// [`render_all_cropped`] that looks at `cancel` before each page and gives
/// up with `Err` once it is true.
pub fn render_all_cropped_cancellable(
    prepared: &Prepared,
    pages: &[PlannedPage],
    opts: &ProcessOptions,
    crops: &[[f32; 4]],
    cancel: CancelCheck,
) -> Result<Vec<LevelImage>, Cancelled> {
    let cache = PieceCache::for_pages(pages);
    par_map(pages.len(), cancel, |i| {
        render_page_cropped_with(
            prepared,
            &pages[i],
            opts,
            crops.get(i).copied().unwrap_or([0.0; 4]),
            cache.as_ref(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dither::Dither;
    use crate::gray::Depth;

    fn opts() -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o
    }

    /// `n` images 480 wide: dark content with a white gutter every `scene`
    /// rows, `height` rows tall each.
    fn scenes(n: usize, height: u32, scene: u32, gutter: u32) -> Vec<GrayImage> {
        (0..n)
            .map(|i| {
                GrayImage::from_fn(480, height, |x, y| {
                    let row = i as u32 * height + y;
                    let in_gutter = row % scene >= scene - gutter;
                    if in_gutter || x < 20 {
                        255
                    } else {
                        40
                    }
                })
                .unwrap()
            })
            .collect()
    }

    fn tops(plan: &ChapterPlan) -> Vec<u32> {
        plan.pages
            .iter()
            .map(|p| match p.source {
                Source::Strip { top, .. } => top,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn many_short_images_are_stitched_into_one_strip() {
        // Six 700-row images: no image alone fills a page, together 4200 rows.
        let images = scenes(6, 700, 100_000, 0);
        let mut o = opts();
        o.strip_cut = StripCut::Overlap;
        let prepared = prepare(&images, &o);
        assert!(prepared.is_strip());
        let plan = plan(&prepared, &o);
        assert!(plan.strip);
        // No gutters: 800-row pages overlapping by 64: tops 0, 736, 1472, ...
        assert_eq!(&tops(&plan)[..3], &[0, 736, 1472]);
        // Crossing an image seam changes nothing: the page at 736 starts
        // inside image 1 and is a single continuous slice.
        let rows = original_rows(&prepared, &plan.pages[1]).unwrap();
        assert_eq!(rows.height, 800);
    }

    /// Busy pictures (stripes) with one flat black band. The band differs from
    /// the page's background enough to count as drawing, so it is not a gap.
    fn busy_with_a_quiet_band(rows: u32, band: std::ops::Range<u32>) -> GrayImage {
        GrayImage::from_fn(480, rows, |x, y| {
            if band.contains(&y) {
                0
            } else if (x / 3) % 2 == 0 {
                40
            } else {
                220
            }
        })
        .unwrap()
    }

    #[test]
    fn without_a_gap_the_page_is_cut_at_the_quietest_row_and_nothing_repeats() {
        // Rows 700..740 are flat, everything else is busy: the page has to end
        // in the band, not at the 800-row screen limit, and the next page
        // starts exactly where it ends.
        let images = [busy_with_a_quiet_band(5000, 700..740)];
        let mut o = opts();
        o.strip_cut = StripCut::Quiet;
        let plan = plan(&prepare(&images, &o), &o);
        let tops = tops(&plan);
        let Source::Strip { height, .. } = plan.pages[0].source else {
            unreachable!()
        };
        assert!((700..=740).contains(&height), "cut at {height}");
        assert_eq!(tops[1], height, "the next page starts where this one ends");
        // No page repeats any row of the one before it.
        for pair in plan.pages.windows(2) {
            let (Source::Strip { top, height }, Source::Strip { top: next, .. }) =
                (pair[0].source, pair[1].source)
            else {
                unreachable!()
            };
            assert!(
                next >= top + height,
                "rows repeated: {top}+{height} then {next}"
            );
        }
    }

    #[test]
    fn the_older_cut_repeats_the_last_rows_on_the_next_page() {
        let images = [busy_with_a_quiet_band(5000, 700..740)];
        let mut o = opts();
        o.strip_cut = StripCut::Overlap;
        let tops = tops(&plan(&prepare(&images, &o), &o));
        // Full 800-row pages, each starting 64 rows (8%) before the last ended.
        assert_eq!(&tops[..3], &[0, 736, 1472]);
    }

    #[test]
    fn a_blank_gap_is_still_used_first() {
        let images = scenes(1, 6000, 600, 90);
        let mut o = opts();
        o.strip_cut = StripCut::Quiet;
        let plan = plan(&prepare(&images, &o), &o);
        let Source::Strip { height, .. } = plan.pages[0].source else {
            unreachable!()
        };
        assert!((510..=530).contains(&height), "height {height}");
    }

    #[test]
    fn pages_are_cut_in_the_gutters_between_scenes() {
        // A 90-row white gutter ends every 600-row scene.
        let images = scenes(1, 6000, 600, 90);
        let o = opts();
        let prepared = prepare(&images, &o);
        let plan = plan(&prepared, &o);
        let Source::Strip { top, height } = plan.pages[0].source else {
            unreachable!()
        };
        assert_eq!(top, 0);
        // The first page ends where scene 1's gutter begins (rows 510..600),
        // a little into it, not at the 800-row screen limit.
        assert!((510..=530).contains(&height), "height {height}");
        // The second page starts in or just before the next scene's content,
        // after the gutter.
        assert!((590..=600).contains(&tops(&plan)[1]), "{:?}", tops(&plan));
    }

    #[test]
    fn trailing_whitespace_does_not_become_a_page() {
        let mut img = GrayImage::new(480, 5000, 255).unwrap();
        img.fill_rect(20, 0, 440, 1500, 40);
        let o = opts();
        let prepared = prepare(&[img], &o);
        let plan = plan(&prepared, &o);
        assert!(plan.pages.len() <= 3, "{} pages", plan.pages.len());
        assert!(plan.pages.len() >= 2);
    }

    #[test]
    fn every_page_is_exactly_the_device_size_for_any_device() {
        for (w, h) in [(480, 800), (528, 792), (758, 1024), (200, 300)] {
            let mut o = opts();
            o.width = w;
            o.height = h;
            let images = scenes(3, 1200, 700, 80);
            for page in convert_chapter(&images, &o) {
                assert_eq!((page.width, page.height), (w, h));
            }
        }
    }

    #[test]
    fn auto_layout_stitches_tall_chapters_and_pages_wide_ones() {
        let mut o = opts();
        o.layout = Layout::Auto;
        let tall = scenes(2, 1500, 100_000, 0); // 480x1500 = 3.1x
        assert!(prepare(&tall, &o).is_strip());
        let pages: Vec<GrayImage> = (0..2)
            .map(|_| GrayImage::new(1000, 1500, 200).unwrap())
            .collect();
        assert!(!prepare(&pages, &o).is_strip());
    }

    #[test]
    fn a_paged_chapter_is_one_page_per_image_and_spreads_split_in_reading_order() {
        let mut o = opts();
        o.layout = Layout::Manga;
        o.direction = ReadDirection::RightToLeft;
        let page = GrayImage::new(1000, 1500, 200).unwrap();
        let spread = GrayImage::from_fn(1600, 1000, |x, _| if x < 800 { 0 } else { 255 }).unwrap();
        let prepared = prepare(&[page, spread], &o);
        let p = plan(&prepared, &o);
        assert_eq!(p.pages.len(), 3);
        let first = render_page(&prepared, &p.pages[1], &o);
        assert_eq!(first.get(240, 400), 3, "right half (white) is read first");
        let second = render_page(&prepared, &p.pages[2], &o);
        assert_eq!(second.get(240, 400), 0);
    }

    #[test]
    fn spread_fit_and_turn_give_one_page() {
        let spread = GrayImage::new(1600, 1000, 128).unwrap();
        for mode in [SpreadMode::Fit, SpreadMode::Rotate] {
            let mut o = opts();
            o.layout = Layout::Manga;
            o.spread = mode;
            let prepared = prepare(std::slice::from_ref(&spread), &o);
            assert_eq!(plan(&prepared, &o).pages.len(), 1, "{mode:?}");
        }
    }

    #[test]
    fn a_spread_is_cut_at_the_spine_when_there_is_a_blank_one() {
        // Art on both sides with a blank band a little left of centre.
        let img = GrayImage::from_fn(1000, 800, |x, y| {
            let margin = !(30..970).contains(&x) || !(30..770).contains(&y);
            if margin || (440..470).contains(&x) {
                255
            } else {
                30
            }
        })
        .unwrap();
        assert!((450..=460).contains(&spine(&img)), "{}", spine(&img));
        let solid = GrayImage::from_fn(1000, 800, |x, y| {
            if !(30..970).contains(&x) || !(30..770).contains(&y) {
                255
            } else {
                30
            }
        })
        .unwrap();
        assert_eq!(spine(&solid), 500, "no gap: the middle");
    }
}

#[cfg(test)]
mod manual_crop_tests {
    use super::*;
    use crate::process::{finish, fit};
    use crate::{Depth, Dither};
    #[test]
    fn manual_crop_uses_source_before_fit_and_zero_preserves_pixels() {
        for layout in [Layout::Manga, Layout::Webtoon] {
            let mut o = ProcessOptions::manga_default(40, 60, Depth::Two);
            o.layout = layout;
            o.crop = CropMode::Full;
            o.auto_levels = false;
            o.contrast = 50;
            o.dither = Dither::Threshold;
            let source = GrayImage::from_fn(40, 100, |x, _| if x < 20 { 0 } else { 170 }).unwrap();
            let prepared = prepare(&[source], &o);
            let p = plan(&prepared, &o);
            let page = &p.pages[0];
            assert_eq!(
                render_page_cropped(&prepared, page, &o, [0.0; 4]),
                render_page(&prepared, page, &o)
            );
            let section = page_source(&prepared, page, &o);
            let expected = crop_rect(&section, 20, section.height / 2, 20, section.height / 2);
            let screen = if layout == Layout::Webtoon {
                let scale = (40.0 / expected.width as f64).min(60.0 / expected.height as f64);
                let fitted = resize(
                    &expected,
                    (expected.width as f64 * scale).round() as u32,
                    (expected.height as f64 * scale).round() as u32,
                );
                letterbox_top(&fitted, 40, 60)
            } else {
                fit(&expected, 40, 60)
            };
            assert_eq!(
                render_page_cropped(&prepared, page, &o, [0.5, 0.5, 0.0, 0.0]),
                finish(screen, &o)
            );
        }
    }
    #[test]
    fn spread_source_sections_and_rotation_match_displayed_parts() {
        let mut o = ProcessOptions::manga_default(40, 60, Depth::Two);
        o.layout = Layout::Manga;
        o.crop = CropMode::Full;
        let source = GrayImage::from_fn(80, 40, |x, _| if x < 40 { 0 } else { 255 }).unwrap();
        let prepared = prepare(&[source], &o);
        let p = plan(&prepared, &o);
        assert_eq!(page_source(&prepared, &p.pages[0], &o).get(10, 10), 255);
        assert_eq!(page_source(&prepared, &p.pages[1], &o).get(10, 10), 0);
        let right = p.pages[0].clone();
        o.direction = ReadDirection::LeftToRight;
        let reversed = plan(&prepared, &o);
        assert_eq!(
            right.source, reversed.pages[1].source,
            "source identity follows artwork when direction changes"
        );
        assert_eq!(page_source(&prepared, &right, &o).get(10, 10), 255);
        o.spread = SpreadMode::Rotate;
        let p = plan(&prepared, &o);
        let rotated = page_source(&prepared, &p.pages[0], &o);
        assert_eq!((rotated.width, rotated.height), (40, 80));
        assert_eq!(rotated.get(10, 10), 0);
        for mode in [SpreadMode::Split, SpreadMode::Rotate, SpreadMode::Fit] {
            o.spread = mode;
            let pages = plan(&prepared, &o);
            for page in &pages.pages {
                let source = page_source(&prepared, page, &o);
                let x = source.width / 4;
                let y = source.height / 4;
                let cut = crop_rect(&source, x, y, source.width - x, source.height - y);
                assert_eq!(
                    render_page_cropped(&prepared, page, &o, [0.25, 0.25, 0.0, 0.0]),
                    finish(fit(&cut, o.width, o.height), &o)
                );
            }
        }
    }
}

/// Prepared section before fitting or tone adjustment, including the selected spread part.
pub fn page_source(prepared: &Prepared, page: &PlannedPage, opts: &ProcessOptions) -> GrayImage {
    source_of(prepared, page, opts, None).into_owned()
}

fn source_of<'a>(
    prepared: &'a Prepared,
    page: &PlannedPage,
    _opts: &ProcessOptions,
    cache: Option<&PieceCache>,
) -> PieceRef<'a> {
    match (&prepared.kind, page.source) {
        (Kind::Strip(strip), Source::Strip { top, height }) => {
            PieceRef::Shared(std::sync::Arc::new(strip_rows(strip, top, height)))
        }
        (Kind::Paged(images), Source::Image { index, part }) => {
            let img = &images[index];
            let of = match part {
                Part::Whole => Piece::Whole,
                Part::Rotated => Piece::Rotated,
                Part::Half(n) => Piece::Half(n),
                Part::Tile { of, .. } => of,
            };
            piece_of(img, index, of, cache)
        }
        _ => unreachable!("a plan belongs to its prepared chapter"),
    }
}

/// The downloaded picture a page comes from, whole: for a split double page
/// both halves, for a zoomed page all of it. A strip page has no picture of
/// its own, so it is its rows, as in [`page_source`].
pub fn whole_source(prepared: &Prepared, page: &PlannedPage, opts: &ProcessOptions) -> GrayImage {
    match (&prepared.kind, page.source) {
        (Kind::Paged(images), Source::Image { index, .. }) => images[index].clone(),
        _ => page_source(prepared, page, opts),
    }
}

/// Where a page lies on [`whole_source`], as fractions `[left, top, width,
/// height]` of the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceRegion {
    /// What the page shows of the picture: the half of a split double page,
    /// the piece of a zoomed page, the middle that a crop keeps. All of it
    /// (`[0, 0, 1, 1]`) when the page is the whole picture.
    pub page: [f32; 4],
    /// The part of the picture a manual crop is measured on: the half of a
    /// split double page, else the whole picture (a zoom piece is cropped
    /// before it is zoomed).
    pub piece: [f32; 4],
}

impl SourceRegion {
    /// The page is the whole picture.
    pub fn is_whole(&self) -> bool {
        self.page[0] <= 0.0005
            && self.page[1] <= 0.0005
            && self.page[2] >= 0.999
            && self.page[3] >= 0.999
    }
}

/// Where this page lies on [`whole_source`]. `None` for a turned page (its
/// region would not be a rectangle on the upright picture) and for strip rows.
pub fn source_region(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
) -> Option<SourceRegion> {
    let (Kind::Paged(images), Source::Image { index, part }) = (&prepared.kind, page.source) else {
        return None;
    };
    let img = &images[index];
    let (of, tile_at) = match part {
        Part::Whole => (Piece::Whole, None),
        Part::Rotated => return None,
        Part::Half(n) => (Piece::Half(n), None),
        Part::Tile { of, col, row } => (of, Some((col as u32, row as u32))),
    };
    let (w, h) = (img.width as f64, img.height as f64);
    let (px, pw) = match of {
        Piece::Rotated => return None,
        Piece::Whole => (0.0, w),
        Piece::Half(n) => {
            let mid = spine(img) as f64;
            if n == 1 {
                (mid, w - mid)
            } else {
                (0.0, mid)
            }
        }
    };
    // Where the page lies inside its piece, as fractions of the piece.
    let (mut fx, mut fy, mut fw, mut fh) = (0.0, 0.0, 1.0, 1.0);
    if let Some((col, row)) = tile_at {
        let grid = zoom_grid(pw as u32, h as u32, opts);
        let (col, row) = (col.min(grid.cols - 1), row.min(grid.rows - 1));
        let (gw, gh) = (grid.width as f64, grid.height as f64);
        fx = (grid.width * col / grid.cols) as f64 / gw;
        fw = (grid.width * (col + 1) / grid.cols) as f64 / gw - fx;
        fy = (grid.height * row / grid.rows) as f64 / gh;
        fh = (grid.height * (row + 1) / grid.rows) as f64 / gh - fy;
    } else if !opts.zoom.is_fit() && opts.zoom_mode == ZoomMode::Crop {
        let grid = zoom_grid(pw as u32, h as u32, opts);
        let (sw, sh) = (grid.width.min(opts.width), grid.height.min(opts.height));
        fw = sw as f64 / grid.width as f64;
        fh = sh as f64 / grid.height as f64;
        fx = ((grid.width - sw) / 2) as f64 / grid.width as f64;
        fy = ((grid.height - sh) / 2) as f64 / grid.height as f64;
    }
    Some(SourceRegion {
        page: [
            ((px + fx * pw) / w) as f32,
            fy as f32,
            (fw * pw / w) as f32,
            fh as f32,
        ],
        piece: [(px / w) as f32, 0.0, (pw / w) as f32, 1.0],
    })
}

/// Remove source-edge fractions before fitting and finishing. Zero retains the fast original path.
pub fn render_page_cropped(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
    crop: [f32; 4],
) -> LevelImage {
    render_page_cropped_with(prepared, page, opts, crop, None)
}

fn render_page_cropped_with(
    prepared: &Prepared,
    page: &PlannedPage,
    opts: &ProcessOptions,
    crop: [f32; 4],
    cache: Option<&PieceCache>,
) -> LevelImage {
    let mut c = crop.map(|v| {
        if v.is_finite() {
            v.clamp(0.0, 0.99)
        } else {
            0.0
        }
    });
    for (a, b) in [(0, 2), (1, 3)] {
        let sum = c[a] + c[b];
        if sum > 0.99 {
            c[a] *= 0.99 / sum;
            c[b] *= 0.99 / sum;
        }
    }
    if c == [0.0; 4] {
        return render_page_with(prepared, page, opts, cache);
    }
    let source = source_of(prepared, page, opts, cache);
    let x = ((source.width as f32 * c[0]).floor() as u32).min(source.width - 1);
    let y = ((source.height as f32 * c[1]).floor() as u32).min(source.height - 1);
    let w = source
        .width
        .saturating_sub(x + (source.width as f32 * c[2]).floor() as u32)
        .max(1);
    let h = source
        .height
        .saturating_sub(y + (source.height as f32 * c[3]).floor() as u32)
        .max(1);
    let cut = crop_rect(&source, x, y, w, h);
    drop(source);
    let screen = if matches!(page.source, Source::Strip { .. }) {
        let scale =
            (opts.width as f64 / cut.width as f64).min(opts.height as f64 / cut.height as f64);
        let w = (cut.width as f64 * scale).round().max(1.0) as u32;
        let h = (cut.height as f64 * scale).round().max(1.0) as u32;
        letterbox_top(&resize(&cut, w, h), opts.width, opts.height)
    } else if let Source::Image {
        part: Part::Tile { col, row, .. },
        ..
    } = page.source
    {
        tile(&cut, opts, col as u32, row as u32)
    } else {
        show(&cut, opts)
    };
    finish_guided(screen, opts, page_arrow(prepared, page, opts))
}

#[cfg(test)]
mod zoom_tests {
    use super::*;
    use crate::process::PageZoom;
    use crate::{Depth, Dither};

    /// A 480 x 800 page of four flat quadrants: top left black, top right
    /// 85, bottom left 170, bottom right white. At zoom 200 each quadrant
    /// fills one screen exactly.
    fn quadrants() -> GrayImage {
        GrayImage::from_fn(480, 800, |x, y| match (x >= 240, y >= 400) {
            (false, false) => 0,
            (true, false) => 85,
            (false, true) => 170,
            (true, true) => 255,
        })
        .unwrap()
    }

    fn opts(zoom: u16, mode: ZoomMode, direction: ReadDirection) -> ProcessOptions {
        opts_with(PageZoom::Percent(zoom), mode, direction)
    }

    fn opts_with(zoom: PageZoom, mode: ZoomMode, direction: ReadDirection) -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Eight);
        o.layout = Layout::Manga;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o.spread = SpreadMode::Split;
        o.zoom = zoom;
        o.zoom_mode = mode;
        o.direction = direction;
        o
    }

    fn middle(page: &LevelImage) -> u8 {
        page.get(page.width / 2, page.height / 2)
    }

    #[test]
    fn a_page_zoomed_to_twice_fit_takes_four_screens() {
        let o = opts(200, ZoomMode::Tile, ReadDirection::LeftToRight);
        let grid = zoom_grid(480, 800, &o);
        assert_eq!((grid.width, grid.height, grid.cols, grid.rows), (960, 1600, 2, 2));
        // At Fit, and a little past it on a page that still fits, one screen.
        let fit_grid = zoom_grid(480, 800, &opts(100, ZoomMode::Tile, ReadDirection::LeftToRight));
        assert_eq!((fit_grid.cols, fit_grid.rows), (1, 1));
    }

    #[test]
    fn zoomed_screens_follow_the_reading_direction_and_show_each_part_once() {
        for (direction, want) in [
            // Left to right: top row left then right, then the bottom row.
            (ReadDirection::LeftToRight, [0u8, 85, 170, 255]),
            // Right to left: each row starts at its right.
            (ReadDirection::RightToLeft, [85, 0, 255, 170]),
        ] {
            let o = opts(200, ZoomMode::Tile, direction);
            let pages = convert_chapter(&[quadrants()], &o);
            assert_eq!(pages.len(), 4, "{direction:?}");
            for page in &pages {
                assert_eq!((page.width, page.height), (480, 800));
            }
            let got: Vec<u8> = pages.iter().map(middle).collect();
            assert_eq!(got, want, "{direction:?}");
            // Every screen is one quadrant and nothing else: no repeats.
            for page in &pages {
                let first = page.levels[0];
                assert!(page.levels.iter().all(|&l| l == first));
            }
        }
    }

    #[test]
    fn the_plan_lists_each_screen_so_previews_and_exports_agree() {
        let images = [quadrants(), quadrants()];
        let o = opts(200, ZoomMode::Tile, ReadDirection::RightToLeft);
        let prepared = prepare(&images, &o);
        let plan = plan(&prepared, &o);
        assert_eq!(plan.pages.len(), 8);
        let first = plan.pages[0].source;
        assert_eq!(
            first,
            Source::Image {
                index: 0,
                part: Part::Tile { of: Piece::Whole, col: 1, row: 0 }
            }
        );
        let rendered = render_all_cropped(&prepared, &plan.pages, &o, &[]);
        assert_eq!(rendered, convert_chapter(&images, &o));
    }

    #[test]
    fn fit_width_makes_a_tall_page_for_a_wide_screen_several_screens_top_to_bottom() {
        // A sideways screen (800 x 480) and a 480 x 800 page of four quadrants:
        // at Fit it is a small page in the middle, at Fit width it fills the
        // width, 1333 rows tall, and takes three screens.
        let mut o = opts_with(PageZoom::Width, ZoomMode::Tile, ReadDirection::LeftToRight);
        o.width = 800;
        o.height = 480;
        let pages = convert_chapter(&[quadrants()], &o);
        assert_eq!(pages.len(), 3);
        for page in &pages {
            assert_eq!((page.width, page.height), (800, 480));
        }
        // Left half of the page is darker than its right half, top to bottom.
        assert_eq!(pages[0].get(100, 100), 0);
        assert_eq!(pages[0].get(700, 100), 85);
        assert_eq!(pages[2].get(100, 400), 170);
        assert_eq!(pages[2].get(700, 400), 255);
        // The page already fills a portrait screen's width: Fit width is Fit.
        let upright = opts_with(PageZoom::Width, ZoomMode::Tile, ReadDirection::LeftToRight);
        assert_eq!(convert_chapter(&[quadrants()], &upright).len(), 1);
    }

    #[test]
    fn cropping_keeps_the_middle_and_loses_the_edges() {
        let o = opts(200, ZoomMode::Crop, ReadDirection::LeftToRight);
        let pages = convert_chapter(&[quadrants()], &o);
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        // The window straddles all four quadrants, one in each corner.
        assert_eq!(page.get(10, 10), 0);
        assert_eq!(page.get(470, 10), 85);
        assert_eq!(page.get(10, 790), 170);
        assert_eq!(page.get(470, 790), 255);
    }

    #[test]
    fn fit_is_unchanged_whatever_the_zoom_mode() {
        let plain = convert_chapter(&[quadrants()], &opts(100, ZoomMode::Tile, ReadDirection::RightToLeft));
        let crop = convert_chapter(&[quadrants()], &opts(100, ZoomMode::Crop, ReadDirection::RightToLeft));
        assert_eq!(plain.len(), 1);
        assert_eq!(plain, crop);
    }

    #[test]
    fn each_half_of_a_split_spread_is_zoomed_on_its_own() {
        // 900 x 450: two 450 x 450 halves, each 1.07 x the screen width at Fit
        // and 1.6 x at 150%, which is two screens wide and one tall.
        let spread = GrayImage::new(900, 450, 200).unwrap();
        let o = opts(150, ZoomMode::Tile, ReadDirection::RightToLeft);
        let prepared = prepare(std::slice::from_ref(&spread), &o);
        let plan = plan(&prepared, &o);
        let parts: Vec<Part> = plan
            .pages
            .iter()
            .map(|p| match p.source {
                Source::Image { part, .. } => part,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            parts,
            vec![
                Part::Tile { of: Piece::Half(1), col: 1, row: 0 },
                Part::Tile { of: Piece::Half(1), col: 0, row: 0 },
                Part::Tile { of: Piece::Half(0), col: 1, row: 0 },
                Part::Tile { of: Piece::Half(0), col: 0, row: 0 },
            ]
        );
        assert_eq!(convert_chapter(&[spread], &o).len(), 4);
    }

    #[test]
    fn a_manual_crop_of_a_zoomed_screen_still_gives_a_full_screen() {
        let o = opts(200, ZoomMode::Tile, ReadDirection::LeftToRight);
        let prepared = prepare(&[quadrants()], &o);
        let plan = plan(&prepared, &o);
        let shown = render_page_cropped(&prepared, &plan.pages[0], &o, [0.0, 0.0, 0.25, 0.25]);
        assert_eq!((shown.width, shown.height), (480, 800));
    }
}

#[cfg(test)]
mod region_tests {
    use super::*;
    use crate::process::PageZoom;
    use crate::{Depth, Dither};

    fn opts() -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Eight);
        o.layout = Layout::Manga;
        o.crop = CropMode::Full;
        o.dither = Dither::Threshold;
        o.spread = SpreadMode::Split;
        o.direction = ReadDirection::RightToLeft;
        o
    }

    /// A double page 1600 x 1000: art on both sides of a blank fold in the middle.
    fn spread() -> GrayImage {
        GrayImage::from_fn(1600, 1000, |x, _| {
            if (100..700).contains(&x) || (900..1500).contains(&x) {
                0
            } else {
                255
            }
        })
        .unwrap()
    }

    fn near(a: [f32; 4], b: [f32; 4]) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 0.01, "{a:?} is not {b:?}");
        }
    }

    #[test]
    fn a_split_spread_shows_both_halves_on_the_one_picture() {
        let o = opts();
        let prepared = prepare(&[spread()], &o);
        let pages = plan(&prepared, &o).pages;
        assert_eq!(pages.len(), 2);
        // Right to left: the right half comes first.
        let first = source_region(&prepared, &pages[0], &o).unwrap();
        near(first.page, [0.5, 0.0, 0.5, 1.0]);
        near(first.piece, [0.5, 0.0, 0.5, 1.0]);
        near(
            source_region(&prepared, &pages[1], &o).unwrap().page,
            [0.0, 0.0, 0.5, 1.0],
        );
        for page in &pages {
            let whole = whole_source(&prepared, page, &o);
            assert_eq!(
                (whole.width, whole.height),
                (1600, 1000),
                "the picture is never cut"
            );
        }
    }

    #[test]
    fn fit_and_rotate_leave_the_whole_picture() {
        for mode in [SpreadMode::Fit, SpreadMode::Rotate] {
            let mut o = opts();
            o.spread = mode;
            let prepared = prepare(&[spread()], &o);
            let pages = plan(&prepared, &o).pages;
            assert_eq!(pages.len(), 1);
            let region = source_region(&prepared, &pages[0], &o);
            if mode == SpreadMode::Rotate {
                assert_eq!(
                    region, None,
                    "a turned page is not a rectangle on the picture"
                );
            } else {
                assert!(region.unwrap().is_whole());
            }
        }
    }

    #[test]
    fn zoomed_pieces_and_crops_mark_their_part_of_the_picture() {
        let mut o = opts();
        o.spread = SpreadMode::Fit;
        o.zoom = PageZoom::Percent(200);
        let page = GrayImage::new(480, 800, 255).unwrap();
        let prepared = prepare(std::slice::from_ref(&page), &o);
        let pages = plan(&prepared, &o).pages;
        assert_eq!(pages.len(), 4);
        // Right to left: the top right quarter first.
        near(
            source_region(&prepared, &pages[0], &o).unwrap().page,
            [0.5, 0.0, 0.5, 0.5],
        );
        near(
            source_region(&prepared, &pages[3], &o).unwrap().page,
            [0.0, 0.5, 0.5, 0.5],
        );
        o.zoom_mode = ZoomMode::Crop;
        let pages = plan(&prepared, &o).pages;
        assert_eq!(pages.len(), 1);
        near(
            source_region(&prepared, &pages[0], &o).unwrap().page,
            [0.25, 0.25, 0.5, 0.5],
        );
    }
}

#[cfg(test)]
mod arrow_tests {
    use super::*;
    use crate::process::PageZoom;
    use crate::{Depth, Dither};

    fn opts(direction: ReadDirection) -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Eight);
        o.layout = Layout::Manga;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o.spread = SpreadMode::Split;
        o.direction = direction;
        o.guide_arrows = true;
        o
    }

    /// Where the black of a page is, on an otherwise white one: the corner
    /// the arrow sits in.
    fn arrow_at(page: &LevelImage) -> Option<&'static str> {
        let dark: Vec<u32> = (0..page.height)
            .flat_map(|y| (0..page.width).map(move |x| (x, y)))
            .filter(|&(x, y)| page.get(x, y) == 0)
            .map(|(x, _)| x)
            .collect();
        let mean = dark.iter().map(|&x| x as f64).sum::<f64>() / dark.len().max(1) as f64;
        match (dark.is_empty(), mean / page.width as f64) {
            (true, _) => None,
            (_, m) if m < 0.33 => Some("left"),
            (_, m) if m > 0.67 => Some("right"),
            _ => Some("down"),
        }
    }

    fn arrows(img: &GrayImage, o: &ProcessOptions) -> Vec<Option<&'static str>> {
        let prepared = prepare(std::slice::from_ref(img), o);
        plan(&prepared, o)
            .pages
            .iter()
            .map(|p| arrow_at(&render_page(&prepared, p, o)))
            .collect()
    }

    fn white(w: u32, h: u32) -> GrayImage {
        GrayImage::new(w, h, 255).unwrap()
    }

    #[test]
    fn the_first_half_of_a_split_spread_points_to_the_second() {
        // Right to left: the right half is read first, so the arrow points left.
        assert_eq!(
            arrows(&white(1600, 1000), &opts(ReadDirection::RightToLeft)),
            vec![Some("left"), None]
        );
        assert_eq!(
            arrows(&white(1600, 1000), &opts(ReadDirection::LeftToRight)),
            vec![Some("right"), None]
        );
    }

    #[test]
    fn arrows_are_off_unless_asked_for_and_never_on_a_whole_page() {
        let mut o = opts(ReadDirection::RightToLeft);
        o.guide_arrows = false;
        assert_eq!(arrows(&white(1600, 1000), &o), vec![None, None]);
        o.guide_arrows = true;
        assert_eq!(arrows(&white(480, 800), &o), vec![None]);
        o.spread = SpreadMode::Fit;
        assert_eq!(arrows(&white(1600, 1000), &o), vec![None]);
    }

    #[test]
    fn a_zoomed_page_points_along_its_row_then_down() {
        let mut o = opts(ReadDirection::LeftToRight);
        o.zoom = PageZoom::Percent(200);
        // Two by two: right, down to the next row, right, and the last has none.
        assert_eq!(
            arrows(&white(480, 800), &o),
            vec![Some("right"), Some("down"), Some("right"), None]
        );
        o.direction = ReadDirection::RightToLeft;
        assert_eq!(
            arrows(&white(480, 800), &o),
            vec![Some("left"), Some("down"), Some("left"), None]
        );
    }

    #[test]
    fn the_last_screen_of_the_first_half_points_on_to_the_other_half() {
        let mut o = opts(ReadDirection::RightToLeft);
        o.zoom = PageZoom::Percent(200);
        let all = arrows(&white(1600, 1000), &o);
        let half = all.len() / 2;
        assert!(all.len() >= 2 && all.len().is_multiple_of(2), "{all:?}");
        assert_eq!(all[half - 1], Some("left"), "the first half hands on to the second");
        assert_eq!(all[all.len() - 1], None, "the book of this page ends here");
    }

    #[test]
    fn the_arrow_is_drawn_the_same_by_the_single_page_path() {
        let o = opts(ReadDirection::RightToLeft);
        let pages = crate::process::process_page(&white(1600, 1000), &o);
        let got: Vec<_> = pages.iter().map(arrow_at).collect();
        assert_eq!(got, vec![Some("left"), None]);
    }
}

#[cfg(test)]
#[path = "plan_audit_tests.rs"]
mod audit_tests;
