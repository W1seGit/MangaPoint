//! From a decoded page to device-ready pages.
//!
//! Pure functions over [`GrayImage`]: no I/O, no image-format code, so the
//! same pipeline runs natively and in WASM. Steps, in order:
//!
//! 1. cut fixed margins ([`Margins`]), then trim blank margins ([`CropMode::Margins`])
//! 2. decide the layout: normal page, double-page spread or tall webtoon strip
//! 3. scale to the device screen (area averaging)
//! 4. stretch levels and apply contrast
//! 5. dither down to 1 or 2 bits

use crate::dither::{quantize, Dither};
use crate::gray::{Depth, GrayImage, LevelImage};
use crate::xtc::ReadDirection;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Ordinary pages.
    Manga,
    /// A very tall strip, cut into screen-sized slices.
    Webtoon,
    /// Pick per image: strips taller than 2.5 times their width are webtoons.
    Auto,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropMode {
    /// Keep the whole image.
    Full,
    /// Trim blank margins around the artwork.
    Margins,
}

/// Fixed margins to cut from every page, each as a percentage (0..=40) of that
/// side's length. Useful for site banners and watermarks that sit in the same
/// place on every page.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    pub top: u8,
    pub bottom: u8,
    pub left: u8,
    pub right: u8,
}

impl Margins {
    pub const MAX_PERCENT: u8 = 40;

    pub fn is_none(&self) -> bool {
        *self == Margins::default()
    }
}

/// What to do with landscape (double-page) images.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpreadMode {
    /// Scale the whole spread to fit, leaving bars above and below.
    Fit,
    /// Cut it into two pages in reading order.
    Split,
    /// Turn it sideways so it uses the full screen height.
    Rotate,
}

/// What happens to the part of a page that no longer fits the screen once
/// it is zoomed in past Fit.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZoomMode {
    /// Cut the zoomed page into screen-sized pieces, shown in reading order.
    /// Nothing is lost; a page becomes several reader pages.
    #[default]
    Tile,
    /// Keep the middle of the zoomed page and cut the rest off.
    Crop,
}

impl ZoomMode {
    pub fn is_tile(&self) -> bool {
        *self == ZoomMode::Tile
    }
}

/// How far a paged image is scaled up from Fit.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageZoom {
    /// The whole page visible, as large as it can be.
    #[default]
    Fit,
    /// As wide as the screen; a page taller than the screen takes several.
    Width,
    /// A percentage of Fit: 100 is Fit, [`ZOOM_MAX`] the most.
    Percent(u16),
}

/// Zoom of 100 is Fit: the whole page visible. Past it the page is scaled up.
pub const ZOOM_FIT: u16 = 100;
pub const ZOOM_MAX: u16 = 300;

impl PageZoom {
    /// Whether this shows the whole page on one screen.
    pub fn is_fit(&self) -> bool {
        match self {
            PageZoom::Fit => true,
            PageZoom::Width => false,
            PageZoom::Percent(p) => *p <= ZOOM_FIT,
        }
    }

    /// Held to what the converter allows; 100% is Fit.
    pub fn clamped(self) -> PageZoom {
        match self {
            PageZoom::Percent(p) => match p.clamp(ZOOM_FIT, ZOOM_MAX) {
                ZOOM_FIT => PageZoom::Fit,
                p => PageZoom::Percent(p),
            },
            other => other,
        }
    }
}

/// How a long strip is cut into screen-sized pages where there is no blank
/// gap to cut in.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StripCut {
    /// At the quietest row near the bottom of the page, where the least is
    /// drawn, so no row appears twice.
    #[default]
    Quiet,
    /// At the full screen height, with the last rows repeated at the top of
    /// the next page (the older way).
    Overlap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessOptions {
    pub width: u32,
    pub height: u32,
    pub layout: Layout,
    pub crop: CropMode,
    /// Cut before the automatic trim.
    pub margins: Margins,
    /// How readily the automatic trim sees faint marks as content, 0..=100.
    /// Higher keeps more; 50 is the default.
    pub trim_sensitivity: u8,
    /// Pixels of the original kept around the content after trimming.
    pub trim_padding: u8,
    pub spread: SpreadMode,
    /// Decides which half of a split spread comes first.
    pub direction: ReadDirection,
    /// 0..=100, where 50 leaves contrast unchanged.
    pub contrast: u8,
    /// Stretch the tonal range so the paper becomes white and ink black.
    pub auto_levels: bool,
    /// How a long strip is cut when there is no blank gap.
    pub strip_cut: StripCut,
    /// With [`StripCut::Overlap`], slices overlap by this share of the screen
    /// height (0..=50).
    pub overlap_percent: u8,
    pub dither: Dither,
    pub depth: Depth,
    /// The pages are laid out `width` by `height` (a sideways screen) and
    /// then turned a quarter turn clockwise, so a reader that always shows
    /// its native portrait frame displays them upright when it is held
    /// turned counter-clockwise. The pages come out `height` by `width`.
    pub turn: bool,
    /// How far a paged image is scaled up from Fit. Long strips are always
    /// fitted to the screen width.
    pub zoom: PageZoom,
    pub zoom_mode: ZoomMode,
    /// Draw a small arrow in the corner of a page whose next page carries
    /// on from it (the first half of a split double page, a screen of a
    /// zoomed page), pointing the way the next one lies.
    pub guide_arrows: bool,
}

impl ProcessOptions {
    /// The size of the pages that come out: `width` by `height`, or the
    /// other way round when they are turned.
    pub fn output_size(&self) -> (u32, u32) {
        if self.turn {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// The defaults from the plan: manga, smart crop, right-to-left,
    /// contrast 55, 8% overlap, Floyd-Steinberg.
    pub fn manga_default(width: u32, height: u32, depth: Depth) -> Self {
        Self {
            width,
            height,
            layout: Layout::Auto,
            crop: CropMode::Margins,
            margins: Margins::default(),
            trim_sensitivity: DEFAULT_TRIM_SENSITIVITY,
            trim_padding: DEFAULT_TRIM_PADDING,
            spread: SpreadMode::Split,
            direction: ReadDirection::RightToLeft,
            contrast: 55,
            auto_levels: true,
            strip_cut: StripCut::Quiet,
            overlap_percent: 8,
            dither: Dither::FloydSteinberg,
            depth,
            turn: false,
            zoom: PageZoom::Fit,
            zoom_mode: ZoomMode::Tile,
            guide_arrows: false,
        }
    }
}

pub const DEFAULT_TRIM_SENSITIVITY: u8 = 50;
pub const DEFAULT_TRIM_PADDING: u8 = 4;

/// The part of `img` that is kept: `(x, y, width, height)` in source pixels.
/// Fixed margins come off first, then blank borders if automatic trim is on.
/// The preview draws this rectangle over the original.
pub fn crop_bounds(img: &GrayImage, opts: &ProcessOptions) -> (u32, u32, u32, u32) {
    let side = |p: u8| p.min(Margins::MAX_PERCENT) as u64;
    let cut = |len: u32, p: u8| (len as u64 * side(p) / 100) as u32;
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
    if opts.crop == CropMode::Full {
        return (x, y, w, h);
    }
    let inner = crop_rect(img, x, y, w, h);
    match trim_bounds(
        &inner,
        trim_threshold(opts.trim_sensitivity),
        opts.trim_padding,
    ) {
        Some((tx, ty, tw, th)) => (x + tx, y + ty, tw, th),
        None => (x, y, w, h),
    }
}

/// Convert one source image into one or more device pages.
pub fn process_page(img: &GrayImage, opts: &ProcessOptions) -> Vec<LevelImage> {
    let (x, y, w, h) = crop_bounds(img, opts);
    let cropped = if (x, y, w, h) == (0, 0, img.width, img.height) {
        img.clone()
    } else {
        crop_rect(img, x, y, w, h)
    };
    let layout = match opts.layout {
        Layout::Auto if cropped.height as f32 > cropped.width as f32 * 2.5 => Layout::Webtoon,
        Layout::Auto => Layout::Manga,
        other => other,
    };
    let screens: Vec<(GrayImage, Option<Arrow>)> = match layout {
        Layout::Webtoon => webtoon_slices(&cropped, opts)
            .into_iter()
            .map(|screen| (screen, None))
            .collect(),
        _ => page_screens(&cropped, opts),
    };
    screens
        .into_iter()
        .map(|(screen, arrow)| finish_guided(screen, opts, arrow))
        .collect()
}

fn page_screens(img: &GrayImage, opts: &ProcessOptions) -> Vec<(GrayImage, Option<Arrow>)> {
    let landscape = img.width as f32 > img.height as f32 * 1.1;
    if !landscape {
        return screens(img, opts, false);
    }
    match opts.spread {
        SpreadMode::Fit => screens(img, opts, false),
        SpreadMode::Rotate => screens(&rotate_cw(img), opts, false),
        SpreadMode::Split => {
            let mid = img.width / 2;
            let left = crop_rect(img, 0, 0, mid, img.height);
            let right = crop_rect(img, mid, 0, img.width - mid, img.height);
            let (first, second) = if opts.direction == ReadDirection::RightToLeft {
                (right, left)
            } else {
                (left, right)
            };
            let mut out = screens(&first, opts, true);
            out.extend(screens(&second, opts, false));
            out
        }
    }
}

/// How a page looks once zoomed: its scale, its size in screen pixels, and
/// how many screens it takes in each direction.
pub(crate) struct Grid {
    pub scale: f64,
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
}

/// The zoom of a `w` x `h` page. At Fit the page is one screen; past it, the
/// scaled page is as many screens wide and tall as it needs.
pub(crate) fn zoom_grid(w: u32, h: u32, opts: &ProcessOptions) -> Grid {
    let wide = opts.width as f64 / w.max(1) as f64;
    let fit_scale = wide.min(opts.height as f64 / h.max(1) as f64);
    let scale = match opts.zoom.clamped() {
        PageZoom::Fit => fit_scale,
        PageZoom::Width => wide.max(fit_scale),
        PageZoom::Percent(p) => fit_scale * p as f64 / ZOOM_FIT as f64,
    };
    let width = ((w as f64 * scale).round() as u32).max(1);
    let height = ((h as f64 * scale).round() as u32).max(1);
    Grid {
        scale,
        width,
        height,
        cols: width.div_ceil(opts.width).max(1),
        rows: height.div_ceil(opts.height).max(1),
    }
}

/// The screens of a tiled page in reading order, as `(column, row)` from the
/// left and top: row by row, each row right to left for a right-to-left book.
pub(crate) fn tile_order(cols: u32, rows: u32, direction: ReadDirection) -> Vec<(u32, u32)> {
    let mut order = Vec::new();
    for row in 0..rows {
        for step in 0..cols {
            let col = if direction == ReadDirection::RightToLeft {
                cols - 1 - step
            } else {
                step
            };
            order.push((col, row));
        }
    }
    order
}

/// Which way the page after this one lies, for a guide arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Down,
}

/// The way the next page is read from this one across a row: against the
/// direction of the book's pages' reading order.
pub(crate) fn across(direction: ReadDirection) -> Arrow {
    if direction == ReadDirection::RightToLeft {
        Arrow::Left
    } else {
        Arrow::Right
    }
}

/// The arrow on screen `(col, row)` of a piece shown as `cols` x `rows`
/// screens. It points to the next screen: along the row, or down when the next
/// one starts a new row. The last screen of a piece has one only when the next
/// page is the other half of the same double page (`continues`).
pub(crate) fn tile_arrow(
    cols: u32,
    rows: u32,
    (col, row): (u32, u32),
    continues: bool,
    direction: ReadDirection,
) -> Option<Arrow> {
    let order = tile_order(cols, rows, direction);
    let at = order.iter().position(|&p| p == (col, row))?;
    match order.get(at + 1) {
        Some(&(_, next_row)) if next_row > row => Some(Arrow::Down),
        Some(_) => Some(across(direction)),
        None => continues.then(|| across(direction)),
    }
}

/// Every screen a page makes: one at Fit or when cropping, and in tiling
/// mode as many as the zoomed page takes. With each, the guide arrow it gets
/// when they are on. `continues`: the next page is more of the same double
/// page.
pub(crate) fn screens(
    img: &GrayImage,
    opts: &ProcessOptions,
    continues: bool,
) -> Vec<(GrayImage, Option<Arrow>)> {
    let guide = |arrow: Option<Arrow>| arrow.filter(|_| opts.guide_arrows);
    let single = || guide(continues.then(|| across(opts.direction)));
    if opts.zoom.is_fit() || opts.zoom_mode != ZoomMode::Tile {
        return vec![(show(img, opts), single())];
    }
    let grid = zoom_grid(img.width, img.height, opts);
    if grid.cols * grid.rows == 1 {
        return vec![(fit(img, opts.width, opts.height), single())];
    }
    tile_order(grid.cols, grid.rows, opts.direction)
        .into_iter()
        .map(|(col, row)| {
            let arrow = tile_arrow(grid.cols, grid.rows, (col, row), continues, opts.direction);
            (tile(img, opts, col, row), guide(arrow))
        })
        .collect()
}

/// A page on one screen: fitted, or at its zoom with the middle kept.
pub(crate) fn show(img: &GrayImage, opts: &ProcessOptions) -> GrayImage {
    if !opts.zoom.is_fit() && opts.zoom_mode == ZoomMode::Crop {
        centre(img, opts)
    } else {
        fit(img, opts.width, opts.height)
    }
}

/// The part of a zoomed page that lies in screen `(col, row)`. A screen that
/// is smaller than the display (the pieces of a page are equal, so none is
/// a sliver) is centred on white.
pub(crate) fn tile(img: &GrayImage, opts: &ProcessOptions, col: u32, row: u32) -> GrayImage {
    let grid = zoom_grid(img.width, img.height, opts);
    let (col, row) = (col.min(grid.cols - 1), row.min(grid.rows - 1));
    let x0 = grid.width * col / grid.cols;
    let x1 = grid.width * (col + 1) / grid.cols;
    let y0 = grid.height * row / grid.rows;
    let y1 = grid.height * (row + 1) / grid.rows;
    zoomed_region(img, opts, &grid, (x0, y0, x1, y1))
}

/// The middle of a zoomed page, as large as the screen shows.
fn centre(img: &GrayImage, opts: &ProcessOptions) -> GrayImage {
    let grid = zoom_grid(img.width, img.height, opts);
    let (w, h) = (grid.width.min(opts.width), grid.height.min(opts.height));
    let x0 = (grid.width - w) / 2;
    let y0 = (grid.height - h) / 2;
    zoomed_region(img, opts, &grid, (x0, y0, x0 + w, y0 + h))
}

/// The rectangle `(x0, y0, x1, y1)` of the zoomed page, drawn on a screen.
/// Only the source pixels under it are resampled.
fn zoomed_region(
    img: &GrayImage,
    opts: &ProcessOptions,
    grid: &Grid,
    (x0, y0, x1, y1): (u32, u32, u32, u32),
) -> GrayImage {
    let source = |a: u32, b: u32, limit: u32| {
        let from = ((a as f64 / grid.scale).floor() as u32).min(limit - 1);
        let to = ((b as f64 / grid.scale).ceil() as u32).clamp(from + 1, limit);
        (from, to - from)
    };
    let (sx, sw) = source(x0, x1, img.width);
    let (sy, sh) = source(y0, y1, img.height);
    let region = crop_rect(img, sx, sy, sw, sh);
    let piece = resize(&region, (x1 - x0).max(1), (y1 - y0).max(1));
    letterbox(&piece, opts.width, opts.height)
}

/// Scale a tall strip to the screen width and cut it into overlapping slices.
fn webtoon_slices(img: &GrayImage, opts: &ProcessOptions) -> Vec<GrayImage> {
    let scaled_h = (img.height as f64 * opts.width as f64 / img.width as f64).round() as u32;
    let scaled = resize(img, opts.width, scaled_h.max(1));
    if scaled.height <= opts.height {
        return vec![letterbox(&scaled, opts.width, opts.height)];
    }
    let overlap = opts.height * opts.overlap_percent.min(50) as u32 / 100;
    let step = (opts.height - overlap).max(1);
    let mut out = Vec::new();
    let mut top = 0;
    loop {
        let h = opts.height.min(scaled.height - top);
        out.push(letterbox_top(
            &crop_rect(&scaled, 0, top, opts.width, h),
            opts.width,
            opts.height,
        ));
        if top + opts.height >= scaled.height {
            break;
        }
        top += step;
    }
    out
}

#[cfg(test)]
pub(crate) fn finish(screen: GrayImage, opts: &ProcessOptions) -> LevelImage {
    finish_guided(screen, opts, None)
}

/// [`finish`], with a guide arrow drawn on the page once its tones are set:
/// pure black with a white edge, so it stays plain on any art and in any
/// number of grays.
pub(crate) fn finish_guided(
    mut screen: GrayImage,
    opts: &ProcessOptions,
    arrow: Option<Arrow>,
) -> LevelImage {
    if opts.auto_levels {
        stretch_levels(&mut screen);
    }
    apply_contrast(&mut screen, opts.contrast);
    if let Some(arrow) = arrow {
        draw_arrow(&mut screen, arrow);
    }
    let levels = quantize(&screen, opts.depth, opts.dither);
    if opts.turn {
        levels.rotated_cw()
    } else {
        levels
    }
}

/// Draw a guide arrow in the bottom corner the next page lies toward (the
/// middle for "down"), a tenth of the screen across, with a white edge.
fn draw_arrow(screen: &mut GrayImage, arrow: Arrow) {
    let unit = (screen.width.min(screen.height) / 10).max(12);
    let (w, h) = match arrow {
        Arrow::Down => (unit, unit * 8 / 5),
        _ => (unit * 8 / 5, unit),
    };
    let margin = (unit / 3).max(4);
    let x0 = match arrow {
        Arrow::Left => margin,
        Arrow::Right => screen.width.saturating_sub(w + margin),
        Arrow::Down => screen.width.saturating_sub(w) / 2,
    };
    let y0 = screen.height.saturating_sub(h + margin);
    // Along the arrow from tail (0) to tip (1), and across it from -1 to 1.
    let inside = |x: i64, y: i64| -> bool {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return false;
        }
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let (along, across) = match arrow {
            Arrow::Right => (px / w as f64, (py - h as f64 / 2.0) / (h as f64 / 2.0)),
            Arrow::Left => (1.0 - px / w as f64, (py - h as f64 / 2.0) / (h as f64 / 2.0)),
            Arrow::Down => (py / h as f64, (px - w as f64 / 2.0) / (w as f64 / 2.0)),
        };
        let shaft = along < 0.55 && across.abs() <= 0.36;
        let head = along >= 0.45 && across.abs() <= (1.0 - along) / 0.55;
        shaft || head
    };
    let edge = (unit / 12).max(2) as i64;
    let put = |screen: &mut GrayImage, x: i64, y: i64, value: u8| {
        let (sx, sy) = (x0 as i64 + x, y0 as i64 + y);
        if sx >= 0 && sy >= 0 && (sx as u32) < screen.width && (sy as u32) < screen.height {
            screen.set(sx as u32, sy as u32, value);
        }
    };
    for y in -edge..h as i64 + edge {
        for x in -edge..w as i64 + edge {
            let near = (-edge..=edge)
                .any(|dy| (-edge..=edge).any(|dx| inside(x + dx, y + dy)));
            if near {
                put(screen, x, y, 255);
            }
        }
    }
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if inside(x, y) {
                put(screen, x, y, 0);
            }
        }
    }
}

// ---- Margins ---------------------------------------------------------------

/// Gray-level difference from the background that counts as content.
/// Sensitivity 50 is the long-standing default (28); lower ignores faint
/// marks, higher notices them.
pub(crate) fn trim_threshold(sensitivity: u8) -> i16 {
    let s = sensitivity.min(100) as i16;
    if s <= 50 {
        80 - (80 - 28) * s / 50
    } else {
        28 - (28 - 6) * (s - 50) / 50
    }
}

/// Cut away uniform margins. The background is taken from the image border;
/// content is anything noticeably different. Never trims below 20% of either
/// dimension, so a nearly blank page is left alone.
pub fn trim_margins(img: &GrayImage) -> GrayImage {
    match trim_bounds(
        img,
        trim_threshold(DEFAULT_TRIM_SENSITIVITY),
        DEFAULT_TRIM_PADDING,
    ) {
        Some((x, y, w, h)) => crop_rect(img, x, y, w, h),
        None => img.clone(),
    }
}

/// Where the content is, or `None` to leave the image alone.
pub(crate) fn trim_bounds(
    img: &GrayImage,
    threshold: i16,
    pad: u8,
) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (img.width, img.height);
    let bg = border_median(img);
    let differs = |x: u32, y: u32| (img.get(x, y) as i16 - bg as i16).abs() > threshold;
    let row_has = |y: u32| (0..w).filter(|&x| differs(x, y)).count() as u32 > (w / 200).max(1);
    let col_has = |x: u32| (0..h).filter(|&y| differs(x, y)).count() as u32 > (h / 200).max(1);

    let top = (0..h).find(|&y| row_has(y))?;
    let bottom = (0..h).rev().find(|&y| row_has(y))?;
    let left = (0..w).find(|&x| col_has(x))?;
    let right = (0..w).rev().find(|&x| col_has(x))?;
    let pad = pad as u32;
    let x0 = left.saturating_sub(pad);
    let y0 = top.saturating_sub(pad);
    let x1 = (right + pad + 1).min(w);
    let y1 = (bottom + pad + 1).min(h);
    if (x1 - x0) * 5 < w || (y1 - y0) * 5 < h {
        return None;
    }
    if (x0, y0, x1, y1) == (0, 0, w, h) {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

pub(crate) fn border_median(img: &GrayImage) -> u8 {
    let (w, h) = (img.width, img.height);
    let mut samples = Vec::new();
    for x in 0..w {
        samples.push(img.get(x, 0));
        samples.push(img.get(x, h - 1));
    }
    for y in 0..h {
        samples.push(img.get(0, y));
        samples.push(img.get(w - 1, y));
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

// ---- Geometry --------------------------------------------------------------

pub fn crop_rect(img: &GrayImage, x: u32, y: u32, w: u32, h: u32) -> GrayImage {
    let w = w.min(img.width.saturating_sub(x)).max(1);
    let h = h.min(img.height.saturating_sub(y)).max(1);
    let mut out = Vec::with_capacity(w as usize * h as usize);
    for yy in y..y + h {
        let start = yy as usize * img.width as usize + x as usize;
        out.extend_from_slice(&img.pixels[start..start + w as usize]);
    }
    GrayImage {
        width: w,
        height: h,
        pixels: out,
    }
}

/// Rotate 90 degrees clockwise.
pub fn rotate_cw(img: &GrayImage) -> GrayImage {
    let (w, h) = (img.width, img.height);
    GrayImage::from_fn(h, w, |x, y| img.get(y, h - 1 - x)).expect("non-zero size")
}

/// Scale to fit inside `w` x `h`, keep the aspect ratio, centre on white.
pub fn fit(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    let scale = (w as f64 / img.width as f64).min(h as f64 / img.height as f64);
    let nw = ((img.width as f64 * scale).round() as u32).clamp(1, w);
    let nh = ((img.height as f64 * scale).round() as u32).clamp(1, h);
    letterbox(&resize(img, nw, nh), w, h)
}

fn letterbox(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    let mut canvas = GrayImage::new(w, h, 255).expect("non-zero size");
    blit(
        &mut canvas,
        img,
        (w - img.width.min(w)) / 2,
        (h - img.height.min(h)) / 2,
    );
    canvas
}

pub(crate) fn letterbox_top(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    let mut canvas = GrayImage::new(w, h, 255).expect("non-zero size");
    blit(&mut canvas, img, (w - img.width.min(w)) / 2, 0);
    canvas
}

fn blit(dst: &mut GrayImage, src: &GrayImage, x0: u32, y0: u32) {
    for y in 0..src.height.min(dst.height.saturating_sub(y0)) {
        for x in 0..src.width.min(dst.width.saturating_sub(x0)) {
            dst.set(x0 + x, y0 + y, src.get(x, y));
        }
    }
}

/// Resample to `w` x `h`: area averaging when shrinking (sharp, alias-free
/// line art) and linear interpolation when enlarging.
pub fn resize(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    if (img.width, img.height) == (w, h) {
        return img.clone();
    }
    // Horizontal pass into f32, then vertical pass.
    let horizontal: Vec<f32> = {
        let weights = axis_weights(img.width, w);
        let mut out = vec![0f32; w as usize * img.height as usize];
        for y in 0..img.height as usize {
            let row = &img.pixels[y * img.width as usize..(y + 1) * img.width as usize];
            for (x, taps) in weights.iter().enumerate() {
                out[y * w as usize + x] = taps.iter().map(|&(i, k)| row[i] as f32 * k).sum();
            }
        }
        out
    };
    let weights = axis_weights(img.height, h);
    let mut out = vec![0u8; w as usize * h as usize];
    for (y, taps) in weights.iter().enumerate() {
        for x in 0..w as usize {
            let v: f32 = taps
                .iter()
                .map(|&(i, k)| horizontal[i * w as usize + x] * k)
                .sum();
            out[y * w as usize + x] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    GrayImage {
        width: w,
        height: h,
        pixels: out,
    }
}

/// For each output index, the contributing input indices and their weights
/// (summing to 1).
fn axis_weights(src: u32, dst: u32) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f64 / dst as f64;
    (0..dst)
        .map(|o| {
            if scale > 1.0 {
                // Shrinking: integrate over [o*scale, (o+1)*scale).
                let (start, end) = (o as f64 * scale, (o as f64 + 1.0) * scale);
                let first = start.floor() as usize;
                let last = (end.ceil() as usize).min(src as usize);
                let mut taps = Vec::with_capacity(last - first);
                for i in first..last {
                    let overlap = (end.min(i as f64 + 1.0) - start.max(i as f64)).max(0.0);
                    if overlap > 0.0 {
                        taps.push((i, (overlap / scale) as f32));
                    }
                }
                taps
            } else {
                // Enlarging: interpolate between the two nearest samples.
                let centre = (o as f64 + 0.5) * scale - 0.5;
                let lo = centre.floor().clamp(0.0, src as f64 - 1.0);
                let hi = (lo + 1.0).min(src as f64 - 1.0);
                let t = (centre - lo).clamp(0.0, 1.0) as f32;
                if lo == hi {
                    vec![(lo as usize, 1.0)]
                } else {
                    vec![(lo as usize, 1.0 - t), (hi as usize, t)]
                }
            }
        })
        .collect()
}

// ---- Tone ------------------------------------------------------------------

/// Map the 1st..99th percentile of the page onto black..white, so grayish
/// paper becomes white. Leaves low-range images (e.g. flat color) alone.
pub fn stretch_levels(img: &mut GrayImage) {
    let mut hist = [0u32; 256];
    for &p in &img.pixels {
        hist[p as usize] += 1;
    }
    let total = img.pixels.len() as u32;
    let percentile = |share: f32| {
        let target = (total as f32 * share) as u32;
        let mut acc = 0;
        for (v, &n) in hist.iter().enumerate() {
            acc += n;
            if acc > target {
                return v as u8;
            }
        }
        255
    };
    let (lo, hi) = (percentile(0.01), percentile(0.99));
    if hi.saturating_sub(lo) < 64 {
        return;
    }
    let range = (hi - lo) as f32;
    let lookup: [u8; 256] = std::array::from_fn(|value| {
        ((value as f32 - lo as f32) / range * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    });
    for p in &mut img.pixels {
        *p = lookup[*p as usize];
    }
}

/// `contrast` 50 is neutral; 0 flattens to mid gray; 100 doubles the slope.
pub fn apply_contrast(img: &mut GrayImage, contrast: u8) {
    if contrast == 50 {
        return;
    }
    let factor = contrast.min(100) as f32 / 50.0;
    let lookup: [u8; 256] = std::array::from_fn(|value| {
        ((value as f32 - 128.0) * factor + 128.0)
            .round()
            .clamp(0.0, 255.0) as u8
    });
    for p in &mut img.pixels {
        *p = lookup[*p as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ProcessOptions {
        ProcessOptions::manga_default(480, 800, Depth::Two)
    }

    #[test]
    fn a_turned_page_is_a_sideways_layout_in_the_readers_own_frame() {
        // A wide page, black along its top edge, laid out on the sideways
        // screen (800 wide, 480 tall) and then turned clockwise.
        let mut o = ProcessOptions::manga_default(800, 480, Depth::Two);
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o.spread = SpreadMode::Fit;
        o.turn = true;
        assert_eq!(o.output_size(), (480, 800));
        let source = GrayImage::from_fn(800, 480, |_, y| if y < 40 { 0 } else { 255 }).unwrap();
        let pages = process_page(&source, &o);
        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!((page.width, page.height), (480, 800));
        // The top edge of the picture is the right edge of the frame.
        assert_eq!(page.get(479, 400), 0);
        assert_eq!(page.get(440, 400), 0);
        assert_eq!(page.get(0, 400), Depth::Two.max_level());
    }

    #[test]
    fn the_single_page_path_zooms_like_the_chapter_path() {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Eight);
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        let page = GrayImage::from_fn(480, 800, |x, _| if x < 240 { 0 } else { 255 }).unwrap();
        o.zoom = PageZoom::Percent(200);
        // Right to left: the right half first.
        let pages = process_page(&page, &o);
        assert_eq!(pages.len(), 4);
        assert_eq!(pages[0].get(240, 400), 255);
        assert_eq!(pages[1].get(240, 400), 0);
        o.zoom_mode = ZoomMode::Crop;
        assert_eq!(process_page(&page, &o).len(), 1);
    }

    #[test]
    fn turning_levels_moves_each_one_a_quarter_turn_clockwise() {
        let image = LevelImage {
            width: 3,
            height: 2,
            depth: Depth::Eight,
            levels: vec![1, 2, 3, 4, 5, 6],
        };
        let turned = image.rotated_cw();
        assert_eq!((turned.width, turned.height), (2, 3));
        // Row 0 (1 2 3) becomes the right column, read downwards.
        assert_eq!(turned.levels, vec![4, 1, 5, 2, 6, 3]);
        assert_eq!(turned.rotated_cw().rotated_cw().rotated_cw(), image);
    }

    #[test]
    #[ignore = "manual throughput check on the export benchmark machine; run in release"]
    fn tone_reader_pages_throughput() {
        let image = GrayImage::from_fn(480, 800, |x, y| ((x * 71 + y * 29) % 256) as u8).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..250 {
            let mut page = image.clone();
            stretch_levels(&mut page);
            apply_contrast(&mut page, 55);
            std::hint::black_box(page);
        }
        let elapsed = started.elapsed();
        eprintln!("250 reader pages tone adjustment: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "first-export tone throughput target missed: {elapsed:?}"
        );
    }

    #[test]
    fn tone_adjustment_matches_scalar_math_for_all_byte_values() {
        let ramp = GrayImage::from_pixels(256, 1, (0..=255).collect()).unwrap();
        for contrast in [0, 1, 49, 50, 55, 99, 100, 255] {
            let mut actual = ramp.clone();
            apply_contrast(&mut actual, contrast);
            let factor = contrast.min(100) as f32 / 50.0;
            let expected: Vec<_> = (0..=255)
                .map(|v| {
                    ((v as f32 - 128.0) * factor + 128.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                })
                .collect();
            assert_eq!(actual.pixels, expected);
        }
        // The ramp's 1st and 99th percentile bins are 2 and 253.
        let mut stretched = ramp;
        stretch_levels(&mut stretched);
        let expected: Vec<_> = (0..=255)
            .map(|v| ((v as f32 - 2.0) / 251.0 * 255.0).round().clamp(0.0, 255.0) as u8)
            .collect();
        assert_eq!(stretched.pixels, expected);
    }

    #[test]
    fn resize_keeps_flat_images_flat_and_hits_the_target_size() {
        for (sw, sh, dw, dh) in [(1200, 1800, 480, 720), (100, 100, 480, 480), (7, 9, 3, 4)] {
            let flat = GrayImage::new(sw, sh, 200).unwrap();
            let r = resize(&flat, dw, dh);
            assert_eq!((r.width, r.height), (dw, dh));
            assert!(r.pixels.iter().all(|&p| p == 200), "{sw}x{sh}->{dw}x{dh}");
        }
    }

    #[test]
    fn shrinking_averages_instead_of_dropping_pixels() {
        // A 1px checkerboard shrunk 2x must become mid gray, not black or white.
        let checker =
            GrayImage::from_fn(8, 8, |x, y| if (x + y) % 2 == 0 { 0 } else { 255 }).unwrap();
        let r = resize(&checker, 4, 4);
        assert!(
            r.pixels.iter().all(|&p| (126..=129).contains(&p)),
            "{:?}",
            r.pixels
        );
    }

    #[test]
    fn fit_letterboxes_on_white_and_keeps_aspect() {
        let img = GrayImage::new(100, 100, 0).unwrap(); // square, black
        let out = fit(&img, 480, 800);
        assert_eq!((out.width, out.height), (480, 800));
        assert_eq!(out.get(240, 400), 0); // centre is the image
        assert_eq!(out.get(240, 10), 255); // bars above and below
        assert_eq!(out.get(240, 790), 255);
        assert_eq!(out.get(0, 400), 0); // full width used
    }

    #[test]
    fn trims_white_margins_but_not_content() {
        let mut img = GrayImage::new(400, 600, 255).unwrap();
        img.fill_rect(100, 150, 200, 300, 0);
        let t = trim_margins(&img);
        assert_eq!((t.width, t.height), (200 + 8, 300 + 8));
        assert_eq!(t.get(4, 4), 0);
        assert_eq!(t.get(0, 0), 255);
    }

    #[test]
    fn trimming_works_on_dark_margins_too() {
        let mut img = GrayImage::new(400, 600, 0).unwrap();
        img.fill_rect(100, 150, 200, 300, 255);
        let t = trim_margins(&img);
        assert_eq!((t.width, t.height), (208, 308));
    }

    #[test]
    fn trimming_leaves_blank_and_tiny_content_pages_alone() {
        let blank = GrayImage::new(300, 400, 255).unwrap();
        assert_eq!(trim_margins(&blank), blank);
        let mut dot = GrayImage::new(300, 400, 255).unwrap();
        dot.fill_rect(150, 200, 20, 20, 0);
        assert_eq!(trim_margins(&dot), dot, "must not zoom into a speck");
    }

    #[test]
    fn fixed_margins_cut_before_trimming_and_crop_bounds_reports_the_result() {
        let mut o = opts();
        o.crop = CropMode::Full;
        let img = GrayImage::new(1000, 2000, 255).unwrap();
        assert_eq!(crop_bounds(&img, &o), (0, 0, 1000, 2000));
        o.margins = Margins {
            top: 10,
            bottom: 5,
            left: 20,
            right: 0,
        };
        assert_eq!(crop_bounds(&img, &o), (200, 200, 800, 1700));
        // A huge value is held to the maximum, so something always remains.
        o.margins = Margins {
            top: 90,
            bottom: 90,
            left: 90,
            right: 90,
        };
        let (_, _, w, h) = crop_bounds(&img, &o);
        assert!(w >= 200 && h >= 400);
    }

    #[test]
    fn a_watermark_strip_can_be_cut_away_by_fixed_margins() {
        // Art in the middle, a dark banner across the bottom 10%.
        let mut img = GrayImage::new(400, 600, 255).unwrap();
        img.fill_rect(100, 100, 200, 300, 0);
        img.fill_rect(0, 540, 400, 60, 0);
        let mut o = opts();
        o.margins.bottom = 10;
        let (_, y, _, h) = crop_bounds(&img, &o);
        assert!(y + h <= 540, "the banner is gone: {y}+{h}");
    }

    #[test]
    fn sensitivity_decides_whether_faint_marks_count_as_content() {
        // A faint smudge far from the art: ignored when insensitive, kept when sensitive.
        let mut img = GrayImage::new(400, 600, 255).unwrap();
        img.fill_rect(150, 200, 100, 200, 0);
        img.fill_rect(20, 20, 40, 40, 215);
        let mut o = opts();
        o.trim_sensitivity = 10;
        let calm = crop_bounds(&img, &o);
        o.trim_sensitivity = 90;
        let keen = crop_bounds(&img, &o);
        assert!(calm.2 < keen.2 && calm.3 < keen.3, "{calm:?} vs {keen:?}");
    }

    #[test]
    fn padding_keeps_a_border_around_the_content() {
        let mut img = GrayImage::new(400, 600, 255).unwrap();
        img.fill_rect(100, 150, 200, 300, 0);
        let mut o = opts();
        o.trim_padding = 0;
        assert_eq!(crop_bounds(&img, &o), (100, 150, 200, 300));
        o.trim_padding = 20;
        assert_eq!(crop_bounds(&img, &o), (80, 130, 240, 340));
    }

    #[test]
    fn rotate_cw_moves_the_top_left_to_the_top_right() {
        let mut img = GrayImage::new(4, 2, 255).unwrap();
        img.set(0, 0, 0);
        let r = rotate_cw(&img);
        assert_eq!((r.width, r.height), (2, 4));
        assert_eq!(r.get(1, 0), 0);
    }

    #[test]
    fn a_normal_page_becomes_exactly_one_device_page() {
        let page =
            GrayImage::from_fn(1000, 1500, |x, y| ((x / 50 + y / 50) % 2 * 255) as u8).unwrap();
        let out = process_page(&page, &opts());
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].width, out[0].height), (480, 800));
        assert_eq!(out[0].depth, Depth::Two);
    }

    #[test]
    fn spreads_split_in_reading_order() {
        // Left half black, right half white.
        let spread = GrayImage::from_fn(1600, 1000, |x, _| if x < 800 { 0 } else { 255 }).unwrap();
        let mut o = opts();
        o.crop = CropMode::Full;
        o.auto_levels = false;

        o.direction = ReadDirection::RightToLeft;
        let rtl = process_page(&spread, &o);
        assert_eq!(rtl.len(), 2);
        // Right half (white) comes first for right-to-left reading.
        assert_eq!(rtl[0].get(240, 400), 3);
        assert_eq!(rtl[1].get(240, 400), 0);

        o.direction = ReadDirection::LeftToRight;
        let ltr = process_page(&spread, &o);
        assert_eq!(ltr[0].get(240, 400), 0);
    }

    #[test]
    fn spread_fit_and_rotate_give_one_page() {
        let spread = GrayImage::new(1600, 1000, 128).unwrap();
        for mode in [SpreadMode::Fit, SpreadMode::Rotate] {
            let mut o = opts();
            o.spread = mode;
            assert_eq!(process_page(&spread, &o).len(), 1, "{mode:?}");
        }
    }

    #[test]
    fn tall_strips_become_overlapping_slices() {
        // 480 wide, 3000 tall: webtoon under Auto.
        let strip = GrayImage::from_fn(480, 3000, |_, y| (y / 10 % 256) as u8).unwrap();
        let mut o = opts();
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o.depth = Depth::Two;
        let slices = process_page(&strip, &o);
        // step = 800 - 64 = 736; tops 0, 736, 1472, 2208 (2208 + 800 >= 3000 ends it)
        assert_eq!(slices.len(), 4);
        for s in &slices {
            assert_eq!((s.width, s.height), (480, 800));
        }
        // Overlap: the last 64 rows of one slice equal the first 64 of the next.
        for pair in slices.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            for y in 0..64 {
                assert_eq!(a.get(100, 736 + y), b.get(100, y), "row {y}");
            }
        }
    }

    #[test]
    fn short_webtoon_strips_stay_a_single_page() {
        let mut o = opts();
        o.layout = Layout::Webtoon;
        let strip = GrayImage::new(480, 500, 100).unwrap();
        assert_eq!(process_page(&strip, &o).len(), 1);
    }

    #[test]
    fn contrast_is_neutral_at_50_and_steeper_above() {
        let mut a = GrayImage::from_pixels(3, 1, vec![64, 128, 192]).unwrap();
        let before = a.clone();
        apply_contrast(&mut a, 50);
        assert_eq!(a, before);
        apply_contrast(&mut a, 100);
        assert_eq!(a.pixels, vec![0, 128, 255]);
        let mut flat = before.clone();
        apply_contrast(&mut flat, 0);
        assert!(flat.pixels.iter().all(|&p| p == 128));
    }

    #[test]
    fn auto_levels_whitens_gray_paper_but_skips_flat_images() {
        // Paper at 200, ink at 40, ink covers 5% of the page.
        let mut page = GrayImage::from_fn(100, 100, |x, _| if x < 5 { 40 } else { 200 }).unwrap();
        stretch_levels(&mut page);
        assert_eq!(page.get(50, 50), 255);
        assert_eq!(page.get(0, 0), 0);

        let mut flat = GrayImage::from_fn(100, 100, |x, _| 100 + (x % 10) as u8).unwrap();
        let before = flat.clone();
        stretch_levels(&mut flat);
        assert_eq!(flat, before);
    }
}
