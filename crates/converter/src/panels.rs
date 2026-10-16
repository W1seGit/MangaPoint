//! Finding the panels of a page, so a reader can show them one at a time.
//!
//! A panel is drawn inside a frame, and the frame is one unbroken line, so
//! everything drawn on the page falls apart into connected pieces: one large
//! piece per framed panel, and small ones for page numbers, stray lettering
//! and specks. The gutters between panels only have to separate them, not run
//! straight: a page scanned a little crooked, or one with slanted gutters, is
//! cut just as well.
//!
//! The panels are then put in reading order by cutting the set of them into
//! rows (top to bottom) and each row into columns (in the reading direction),
//! again and again, as a reader's eye goes over a page.
//!
//! Lettering and art outside every frame (a story's opening caption, a banner)
//! is gathered into blocks of its own, so nothing on the page is lost; a page
//! number or a speck is too small to count.
//!
//! Panels joined by a speech bubble or a drawing that crosses the gutter stay
//! together as one. A page that cannot be cut with confidence (a splash page,
//! panels without frames) is left whole.
//!
//! A trained model finds panels these lines cannot (no frame, a drawing across
//! the gutter). It lives outside this crate, which does no I/O: the app
//! registers one with [`set_panel_model`], and its frames are then cleaned up
//! and put in reading order here, the same way as the ones found above.

use crate::gray::GrayImage;
use crate::process::{border_median, resize, trim_threshold, PanelFinder, ProcessOptions};
use crate::xtc::ReadDirection;
use std::sync::{Arc, RwLock};

/// Something that finds the panel frames on a page: a trained model.
pub trait PanelModel: Send + Sync {
    /// The frames on `img`, in its pixels and in no particular order, or
    /// `None` when the model could not run.
    fn frames(&self, img: &GrayImage) -> Option<Vec<Rect>>;
}

static MODEL: RwLock<Option<Arc<dyn PanelModel>>> = RwLock::new(None);

/// Make `model` the one used with [`PanelFinder::Model`], or remove it.
pub fn set_panel_model(model: Option<Arc<dyn PanelModel>>) {
    *MODEL.write().unwrap_or_else(|e| e.into_inner()) = model;
}

/// Whether a model is registered, so [`PanelFinder::Model`] uses it.
pub fn panel_model_ready() -> bool {
    MODEL.read().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// The panels of `img` the way `opts` says to find them. Without a model,
/// or when it fails, the gutters are used.
pub fn panels_for(img: &GrayImage, opts: &ProcessOptions) -> Vec<Rect> {
    if opts.panel_finder == PanelFinder::Model {
        let model = MODEL.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(frames) = model.and_then(|m| m.frames(img)) {
            return panels_from_frames(
                img,
                &frames,
                opts.direction,
                opts.trim_sensitivity,
                opts.trim_padding,
            );
        }
    }
    find_panels(img, opts.direction, opts.trim_sensitivity, opts.trim_padding)
}

/// A part of an image, in its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    pub fn right(&self) -> u32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }

    fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect {
            x,
            y,
            w: self.right().max(o.right()) - x,
            h: self.bottom().max(o.bottom()) - y,
        }
    }

    fn overlap(&self, o: &Rect) -> u64 {
        let w = self.right().min(o.right()).saturating_sub(self.x.max(o.x));
        let h = self.bottom().min(o.bottom()).saturating_sub(self.y.max(o.y));
        w as u64 * h as u64
    }
}

/// The page is looked at with its longer side at most this many pixels:
/// gutters are many pixels wide, so nothing is lost, and it is quick.
const WORK_SIDE: u32 = 1200;

/// A piece whose outline covers less than this share of the page (in
/// thousandths) is a page number, a speck or a stray sound effect, not a
/// panel.
const MIN_PANEL_PERMILLE: u64 = 10;

/// A panel thinner than this share of the page (in percent) is a caption box,
/// and is shown with the panel it is attached to.
const THIN_PERCENT: u32 = 6;

/// The panels must hold at least this share (in percent) of everything drawn
/// on the page, or the cut is not trusted and the page is shown whole.
const MIN_INK_COVERAGE: u64 = 92;

/// The panels of `img` in reading order, in its pixels, each with `padding`
/// pixels of the page kept around it. Fewer than two means the page could
/// not be cut and is best shown whole.
pub fn find_panels(
    img: &GrayImage,
    direction: ReadDirection,
    sensitivity: u8,
    padding: u8,
) -> Vec<Rect> {
    layout(img, None, direction, sensitivity, padding)
}

/// The same, from frames a model found on `img` (in its pixels, any order).
/// Duplicates are merged, captions joined to their panels and anything drawn
/// outside every frame gathered into blocks, as for the gutters' panels.
pub fn panels_from_frames(
    img: &GrayImage,
    frames: &[Rect],
    direction: ReadDirection,
    sensitivity: u8,
    padding: u8,
) -> Vec<Rect> {
    layout(img, Some(frames), direction, sensitivity, padding)
}

fn layout(
    img: &GrayImage,
    frames: Option<&[Rect]>,
    direction: ReadDirection,
    sensitivity: u8,
    padding: u8,
) -> Vec<Rect> {
    if img.width < 16 || img.height < 16 {
        return Vec::new();
    }
    let longest = img.width.max(img.height);
    let small = if longest > WORK_SIDE {
        let w = (img.width as u64 * WORK_SIDE as u64 / longest as u64).max(1) as u32;
        let h = (img.height as u64 * WORK_SIDE as u64 / longest as u64).max(1) as u32;
        resize(img, w, h)
    } else {
        img.clone()
    };
    let page = Rect {
        x: 0,
        y: 0,
        w: small.width,
        h: small.height,
    };
    let pieces = Ink::new(&small, sensitivity).pieces();
    let total: u64 = pieces.iter().map(|p| p.pixels).sum();
    if total == 0 {
        return Vec::new();
    }

    let min_area = page.area() * MIN_PANEL_PERMILLE / 1000;
    // A piece is in the panels when they cover most of it together: a
    // drawing that crosses from one panel into the next is in neither alone.
    let inside = |boxes: &[Rect], p: &Piece| {
        boxes.iter().map(|b| b.overlap(&p.rect)).sum::<u64>() * 2 >= p.rect.area().max(1)
    };
    // The model's frames may overlap a little (an inset panel breaking out of
    // a bigger one); only a frame almost inside another is a duplicate. The
    // pieces' outlines overlap when one panel is drawn into another.
    let same_panel = if frames.is_some() { 8 } else { 3 };
    let found: Vec<Rect> = match frames {
        // The model's frames, measured the same way as the pieces.
        Some(frames) => {
            let sx = small.width as f64 / img.width as f64;
            let sy = small.height as f64 / img.height as f64;
            frames
                .iter()
                .filter_map(|f| {
                    let x0 = ((f.x as f64 * sx).floor() as u32).min(page.w);
                    let y0 = ((f.y as f64 * sy).floor() as u32).min(page.h);
                    let x1 = ((f.right() as f64 * sx).ceil() as u32).min(page.w);
                    let y1 = ((f.bottom() as f64 * sy).ceil() as u32).min(page.h);
                    (x1 > x0 && y1 > y0).then(|| Rect {
                        x: x0,
                        y: y0,
                        w: x1 - x0,
                        h: y1 - y0,
                    })
                })
                .collect()
        }
        None => pieces.iter().map(|p| p.rect).collect(),
    };
    let mut boxes = merge_overlapping(
        found.into_iter().filter(|r| r.area() >= min_area).collect(),
        same_panel,
    );
    if boxes.is_empty() {
        return Vec::new();
    }
    // What is left outside the frames, gathered into blocks: a caption's
    // letters are close together, a page number stands alone and is small.
    let loose: Vec<Rect> = pieces
        .iter()
        .filter(|p| !inside(&boxes, p))
        .map(|p| p.rect)
        .collect();
    let reach = page.w.min(page.h) / 60;
    boxes.extend(
        gather(loose, reach)
            .into_iter()
            .filter(|r| r.area() * 2 >= min_area),
    );
    let boxes = merge_thin(merge_overlapping(boxes, same_panel), page);
    if boxes.len() < 2 {
        return Vec::new();
    }
    // Nearly everything drawn has to be in a panel, or these are not really
    // the page's panels.
    let covered: u64 = pieces
        .iter()
        .filter(|p| inside(&boxes, p))
        .map(|p| p.pixels)
        .sum();
    if covered * 100 < total * MIN_INK_COVERAGE {
        return Vec::new();
    }
    let slack = page.w.min(page.h) / 40;
    let ordered = order(boxes, direction, slack);

    // Back to the page's own pixels, with a little of the page around each.
    let sx = img.width as f64 / small.width as f64;
    let sy = img.height as f64 / small.height as f64;
    let pad = padding as i64;
    ordered
        .into_iter()
        .map(|r| {
            let x0 = ((r.x as f64 * sx).floor() as i64 - pad).max(0) as u32;
            let y0 = ((r.y as f64 * sy).floor() as i64 - pad).max(0) as u32;
            let x1 = ((r.right() as f64 * sx).ceil() as i64 + pad).min(img.width as i64) as u32;
            let y1 = ((r.bottom() as f64 * sy).ceil() as i64 + pad).min(img.height as i64) as u32;
            Rect {
                x: x0,
                y: y0,
                w: (x1 - x0).max(1),
                h: (y1 - y0).max(1),
            }
        })
        .collect()
}

/// Which pixels have something drawn on them.
struct Ink {
    width: u32,
    height: u32,
    marks: Vec<bool>,
}

/// One connected piece of drawing.
struct Piece {
    rect: Rect,
    pixels: u64,
}

impl Ink {
    /// A pixel is drawn on when it differs from the paper. A lone pixel with
    /// no drawn neighbour is scanner dust or JPEG noise and does not count,
    /// so it cannot bridge a gutter.
    fn new(img: &GrayImage, sensitivity: u8) -> Self {
        let (w, h) = (img.width, img.height);
        let bg = border_median(img) as i16;
        let threshold = trim_threshold(sensitivity);
        let raw: Vec<bool> = img
            .pixels
            .iter()
            .map(|&p| (p as i16 - bg).abs() > threshold)
            .collect();
        let at = |x: u32, y: u32| raw[(y * w + x) as usize];
        let mut marks = vec![false; raw.len()];
        for y in 0..h {
            for x in 0..w {
                if !at(x, y) {
                    continue;
                }
                let neighbour = (y.saturating_sub(1)..=(y + 1).min(h - 1)).any(|ny| {
                    (x.saturating_sub(1)..=(x + 1).min(w - 1))
                        .any(|nx| (nx, ny) != (x, y) && at(nx, ny))
                });
                marks[(y * w + x) as usize] = neighbour;
            }
        }
        Self {
            width: w,
            height: h,
            marks,
        }
    }

    /// The connected pieces of drawing (pixels touching at a side or corner).
    fn pieces(&self) -> Vec<Piece> {
        let (w, h) = (self.width, self.height);
        let mut seen = vec![false; self.marks.len()];
        let mut pieces = Vec::new();
        let mut stack = Vec::new();
        for start in 0..self.marks.len() {
            if !self.marks[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
            let mut pixels = 0u64;
            while let Some(i) = stack.pop() {
                let (x, y) = (i as u32 % w, i as u32 / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                pixels += 1;
                for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let j = (ny * w + nx) as usize;
                        if self.marks[j] && !seen[j] {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
            pieces.push(Piece {
                rect: Rect {
                    x: x0,
                    y: y0,
                    w: x1 + 1 - x0,
                    h: y1 + 1 - y0,
                },
                pixels,
            });
        }
        pieces
    }
}

/// Boxes that largely overlap (by `tenths` of the smaller one) are one panel:
/// an inset panel inside a bigger one, or a large drawing that breaks out of
/// its frame. Boxes that only touch at their edges, as neighbours on a crooked
/// scan do, stay apart.
fn merge_overlapping(mut boxes: Vec<Rect>, tenths: u64) -> Vec<Rect> {
    loop {
        let mut merged = None;
        'search: for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let smaller = boxes[i].area().min(boxes[j].area());
                if boxes[i].overlap(&boxes[j]) * 10 >= smaller * tenths {
                    merged = Some((i, j));
                    break 'search;
                }
            }
        }
        let Some((i, j)) = merged else {
            return boxes;
        };
        let other = boxes.remove(j);
        boxes[i] = boxes[i].union(&other);
    }
}

/// Joins rectangles no more than `reach` apart into blocks.
fn gather(mut rects: Vec<Rect>, reach: u32) -> Vec<Rect> {
    let near = |a: &Rect, b: &Rect| {
        let gap_x = b.x.saturating_sub(a.right()).max(a.x.saturating_sub(b.right()));
        let gap_y = b.y.saturating_sub(a.bottom()).max(a.y.saturating_sub(b.bottom()));
        gap_x <= reach && gap_y <= reach
    };
    loop {
        let mut blocks: Vec<Rect> = Vec::with_capacity(rects.len());
        for r in &rects {
            match blocks.iter_mut().find(|b| near(b, r)) {
                Some(b) => *b = b.union(r),
                None => blocks.push(*r),
            }
        }
        if blocks.len() == rects.len() {
            return blocks;
        }
        rects = blocks;
    }
}

/// A caption box (a thin strip above or beside a panel) joins the panel next
/// to it, rather than filling a screen of its own.
fn merge_thin(mut boxes: Vec<Rect>, page: Rect) -> Vec<Rect> {
    let thin =
        |r: &Rect| r.h * 100 < page.h * THIN_PERCENT || r.w * 100 < page.w * THIN_PERCENT;
    let reach = page.w.min(page.h) / 20;
    loop {
        let mut joined = None;
        for (i, t) in boxes.iter().enumerate().filter(|(_, r)| thin(r)) {
            // The nearest box beside it that shares most of its length.
            let best = boxes
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .filter_map(|(j, o)| {
                    let across_x = t.right().min(o.right()).saturating_sub(t.x.max(o.x));
                    let across_y = t.bottom().min(o.bottom()).saturating_sub(t.y.max(o.y));
                    let gap_y = o.y.saturating_sub(t.bottom()).max(t.y.saturating_sub(o.bottom()));
                    let gap_x = o.x.saturating_sub(t.right()).max(t.x.saturating_sub(o.right()));
                    if across_x * 2 >= t.w && gap_y <= reach {
                        Some((gap_y, j))
                    } else if across_y * 2 >= t.h && gap_x <= reach {
                        Some((gap_x, j))
                    } else {
                        None
                    }
                })
                .min();
            if let Some((_, j)) = best {
                joined = Some((i, j));
                break;
            }
        }
        let Some((i, j)) = joined else {
            return boxes;
        };
        let caption = boxes[i];
        boxes[j] = boxes[j].union(&caption);
        boxes.remove(i);
    }
}

/// Reading order: rows from the top, the columns of a row in the reading
/// direction, and each of those again. Boxes that overlap by no more than
/// `slack` pixels, as on a crooked page, still count as side by side.
fn order(mut boxes: Vec<Rect>, direction: ReadDirection, slack: u32) -> Vec<Rect> {
    if boxes.len() < 2 {
        return boxes;
    }
    let rows = groups(&boxes, |r| (r.y, r.bottom()), slack);
    if rows.len() > 1 {
        return rows
            .into_iter()
            .flat_map(|row| order(row, direction, slack))
            .collect();
    }
    let mut columns = groups(&boxes, |r| (r.x, r.right()), slack);
    if columns.len() > 1 {
        if direction == ReadDirection::RightToLeft {
            columns.reverse();
        }
        return columns
            .into_iter()
            .flat_map(|column| order(column, direction, slack))
            .collect();
    }
    // Neither rows nor columns: by the middle of each, top to bottom.
    boxes.sort_by_key(|r| {
        let across = r.x + r.w / 2;
        let across = if direction == ReadDirection::RightToLeft {
            u32::MAX - across
        } else {
            across
        };
        (r.y + r.h / 2, across)
    });
    boxes
}

/// `boxes` in groups whose spans (from `span`) do not overlap by more than
/// `slack`, in order along that axis.
fn groups(boxes: &[Rect], span: impl Fn(&Rect) -> (u32, u32), slack: u32) -> Vec<Vec<Rect>> {
    let mut sorted = boxes.to_vec();
    sorted.sort_by_key(|r| span(r));
    let mut out: Vec<Vec<Rect>> = Vec::new();
    let mut end = 0;
    for r in sorted {
        let (from, to) = span(&r);
        match out.last_mut() {
            Some(group) if from + slack < end => {
                group.push(r);
                end = end.max(to);
            }
            _ => {
                out.push(vec![r]);
                end = to;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page with a black-bordered panel at each rectangle.
    fn page(w: u32, h: u32, panels: &[(u32, u32, u32, u32)]) -> GrayImage {
        let mut img = GrayImage::new(w, h, 255).unwrap();
        for &(x, y, pw, ph) in panels {
            img.fill_rect(x, y, pw, ph, 0);
            // A light inside with some drawing, so the panel is not one blob.
            img.fill_rect(x + 3, y + 3, pw - 6, ph - 6, 235);
            img.fill_rect(x + pw / 3, y + ph / 3, pw / 4, ph / 5, 60);
        }
        img
    }

    fn centres(panels: &[Rect]) -> Vec<(u32, u32)> {
        panels
            .iter()
            .map(|r| (r.x + r.w / 2, r.y + r.h / 2))
            .collect()
    }

    #[test]
    fn a_classic_grid_is_read_right_to_left_then_down() {
        // Two rows of two panels.
        let img = page(
            1000,
            1400,
            &[
                (40, 40, 440, 640),
                (520, 40, 440, 640),
                (40, 720, 440, 640),
                (520, 720, 440, 640),
            ],
        );
        let rtl = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(
            centres(&rtl),
            vec![(740, 360), (260, 360), (740, 1040), (260, 1040)]
        );
        let ltr = find_panels(&img, ReadDirection::LeftToRight, 50, 0);
        assert_eq!(
            centres(&ltr),
            vec![(260, 360), (740, 360), (260, 1040), (740, 1040)]
        );
    }

    #[test]
    fn a_tall_panel_beside_a_stack_is_read_before_it() {
        // Right: one tall panel. Left: two stacked. Right to left, the tall
        // one first, then the stack from the top.
        let img = page(
            1000,
            1400,
            &[
                (520, 40, 440, 1320),
                (40, 40, 440, 640),
                (40, 720, 440, 640),
            ],
        );
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(centres(&p), vec![(740, 700), (260, 360), (260, 1040)]);
    }

    #[test]
    fn panels_cover_their_borders_and_padding_is_added() {
        let img = page(800, 1200, &[(50, 50, 700, 500), (50, 600, 700, 550)]);
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(
            p[0],
            Rect {
                x: 50,
                y: 50,
                w: 700,
                h: 500
            }
        );
        let padded = find_panels(&img, ReadDirection::RightToLeft, 50, 6);
        assert_eq!(
            padded[0],
            Rect {
                x: 44,
                y: 44,
                w: 712,
                h: 512
            }
        );
    }

    #[test]
    fn large_pages_are_measured_at_a_smaller_size_and_mapped_back() {
        let img = page(
            2000,
            3000,
            &[(100, 100, 1800, 1300), (100, 1500, 1800, 1400)],
        );
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(p.len(), 2);
        for (got, want) in p
            .iter()
            .zip([(100, 100, 1800, 1300), (100, 1500, 1800, 1400)])
        {
            assert!(
                got.x.abs_diff(want.0) <= 3 && got.y.abs_diff(want.1) <= 3,
                "{got:?}"
            );
            assert!(
                got.w.abs_diff(want.2) <= 6 && got.h.abs_diff(want.3) <= 6,
                "{got:?}"
            );
        }
    }

    #[test]
    fn a_splash_page_and_a_blank_page_are_not_cut() {
        let splash = page(1000, 1400, &[(40, 40, 920, 1320)]);
        assert!(find_panels(&splash, ReadDirection::RightToLeft, 50, 0).is_empty());
        let blank = GrayImage::new(1000, 1400, 255).unwrap();
        assert!(find_panels(&blank, ReadDirection::RightToLeft, 50, 0).is_empty());
    }

    #[test]
    fn a_bubble_across_a_gutter_keeps_the_two_panels_together() {
        let mut img = page(1000, 1400, &[(40, 40, 920, 640), (40, 720, 920, 640)]);
        // Only the two panels split by the bubble, so nothing is left to cut.
        img.fill_rect(400, 600, 200, 200, 30);
        assert!(find_panels(&img, ReadDirection::RightToLeft, 50, 0).is_empty());
    }

    #[test]
    fn a_page_number_and_dust_do_not_become_panels() {
        let mut img = page(1000, 1400, &[(40, 40, 920, 620), (40, 700, 920, 620)]);
        img.fill_rect(490, 1350, 20, 25, 0); // the page number
        img.set(20, 680, 0); // a lone speck in the gutter
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|r| r.y + r.h <= 1330), "{p:?}");
    }

    #[test]
    fn a_crooked_gutter_with_no_blank_row_still_separates_the_panels() {
        // The gutter between the two panels falls 60 pixels from left to
        // right, so no row of the page is blank all the way across.
        let mut img = GrayImage::new(1000, 1400, 255).unwrap();
        for x in 40..960 {
            let drop = (x - 40) * 60 / 920;
            for y in 40..(640 + drop) {
                let edge = !(43..=956).contains(&x) || y < 43 || y + 3 >= 640 + drop;
                img.set(x, y, if edge { 0 } else { 235 });
            }
            for y in (660 + drop)..1360 {
                let edge = !(43..=956).contains(&x) || y < 663 + drop || y > 1356;
                img.set(x, y, if edge { 0 } else { 235 });
            }
        }
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].y < p[1].y, "top first: {p:?}");
    }

    #[test]
    fn a_caption_strip_joins_the_panel_below_it() {
        let mut img = page(1000, 1400, &[(40, 40, 920, 600), (40, 720, 920, 640)]);
        // A framed caption 30 pixels tall just above the bottom panel.
        img.fill_rect(40, 670, 920, 34, 0);
        img.fill_rect(43, 673, 914, 28, 255);
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[1].y.abs_diff(670) <= 2 && p[1].bottom().abs_diff(1360) <= 2, "{p:?}");
    }

    #[test]
    fn lettering_outside_the_frames_is_kept_as_a_block() {
        let mut img = page(1000, 1400, &[(40, 240, 920, 520), (40, 800, 920, 560)]);
        // Three lines of "letters" above the panels, with no frame.
        for line in 0..3 {
            for letter in 0..40 {
                img.fill_rect(60 + letter * 22, 60 + line * 40, 14, 22, 0);
            }
        }
        let p = find_panels(&img, ReadDirection::RightToLeft, 50, 0);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].y <= 60 && p[0].bottom() >= 160 && p[0].bottom() < 240, "{p:?}");
    }

    #[test]
    fn dark_pages_with_white_panels_are_cut_too() {
        let mut img = GrayImage::new(1000, 1400, 10).unwrap();
        for &(x, y, w, h) in &[(40, 40, 920, 640), (40, 720, 920, 640)] {
            img.fill_rect(x, y, w, h, 240);
            img.fill_rect(x + w / 3, y + h / 3, w / 4, h / 5, 60);
        }
        assert_eq!(
            find_panels(&img, ReadDirection::RightToLeft, 50, 0).len(),
            2
        );
    }
}
