//! Where the lettering is on a strip.
//!
//! A letter is a small blob of ink on a paler ground (or of pale on a dark
//! ground), and lettering is several such blobs side by side on one line,
//! with the same top and bottom. The bubble or box around them is not
//! needed: a bubble that sits on a white background runs into it, but its
//! letters are still letters. A page cut through a line of lettering leaves
//! half a sentence on each page, which is the worst thing a cut can do, so
//! the cutter keeps away from it.

use super::*;

/// The strip is looked at every `STEP`th pixel in both directions.
const STEP: u32 = 2;
/// Ink on a pale ground is at most this bright; pale on a dark ground at least [`PALE`].
const INK: u8 = 110;
const PALE: u8 = 170;
/// A letter is between these heights, in looked-at pixels. Smaller is grain
/// or a dot of screen tone; taller is a stroke of the drawing.
const LETTER_MIN_HEIGHT: u32 = 3;
const LETTER_MAX_HEIGHT: u32 = 36;
/// Letters that touch make one blob, so a blob may be a whole word.
const WORD_MAX_WIDTH: u32 = 120;
/// A letter covers between these shares of its own box: a solid stroke
/// covers nearly all of it, a wisp nearly none.
const MIN_COVER: f32 = 0.15;
const MAX_COVER: f32 = 0.85;
/// Two blobs are on one line when their tops and bottoms are this close ...
const LINE_TOLERANCE: u32 = 2;
/// ... and they are at most this many letter heights apart sideways.
const LINE_REACH: u32 = 10;
/// A letter has at least this many other letters on its line, unless it is a
/// whole word by itself (letters that touch make one blob) ...
const MIN_LINE_MATES: u32 = 2;
/// ... which is at least this many letters wide and, as a word is, part holes.
const LONE_WORD_LETTERS: u32 = 3;
const LONE_WORD_MIN_COVER: f32 = 0.25;
const LONE_WORD_MAX_COVER: f32 = 0.7;
/// A row with at least this many letters across it is a line of lettering.
const LETTERS_PER_LINE: u32 = 3;
/// Lines this close together (looked-at rows) belong to one piece of lettering.
const LINE_GAP: u32 = 20;

/// A run of same-tone pixels on one row of the looked-at strip.
struct Run {
    y: u32,
    x0: u32,
    x1: u32,
    parent: usize,
}

fn root(runs: &mut [Run], mut i: usize) -> usize {
    while runs[i].parent != i {
        let up = runs[runs[i].parent].parent;
        runs[i].parent = up;
        i = up;
    }
    i
}

fn join(runs: &mut [Run], a: usize, b: usize) {
    let (ra, rb) = (root(runs, a), root(runs, b));
    if ra != rb {
        runs[ra.max(rb)].parent = ra.min(rb);
    }
}

#[derive(Clone, Copy)]
struct Blob {
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
    area: u32,
}

impl Blob {
    fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    fn height(&self) -> u32 {
        self.y1 - self.y0
    }

    /// Could be a letter, or a word of letters that touch.
    fn letter_like(&self) -> bool {
        let (w, h) = (self.width(), self.height());
        let cover = self.area as f32 / (w * h) as f32;
        (LETTER_MIN_HEIGHT..=LETTER_MAX_HEIGHT).contains(&h)
            && w <= WORD_MAX_WIDTH
            && (MIN_COVER..=MAX_COVER).contains(&cover)
    }

    /// About how many letters wide the blob is.
    fn letters(&self) -> u32 {
        (self.width() / self.height().max(1)).clamp(1, 12)
    }

    /// A word on its own: wide, and with the holes letters have.
    fn lone_word(&self) -> bool {
        let cover = self.area as f32 / (self.width() * self.height()) as f32;
        self.letters() >= LONE_WORD_LETTERS && (LONE_WORD_MIN_COVER..=LONE_WORD_MAX_COVER).contains(&cover)
    }

    fn on_one_line_with(&self, other: &Blob) -> bool {
        let reach = LINE_REACH * self.height();
        self.y0.abs_diff(other.y0) <= LINE_TOLERANCE
            && self.y1.abs_diff(other.y1) <= LINE_TOLERANCE
            && (other.x0 < self.x1 + reach && self.x0 < other.x1 + reach)
    }
}

/// The connected blobs of pixels for which `tone` holds, as bounding boxes.
fn blobs(strip: &Strip, w: u32, h: u32, tone: impl Fn(u8) -> bool) -> Vec<Blob> {
    let mut runs: Vec<Run> = Vec::new();
    let mut previous: std::ops::Range<usize> = 0..0;
    for y in 0..h {
        let row = strip_row(strip, y * STEP);
        let at = |x: u32| row[(x * STEP) as usize];
        let first = runs.len();
        let mut x = 0;
        while x < w {
            if !tone(at(x)) {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && tone(at(x)) {
                x += 1;
            }
            let i = runs.len();
            runs.push(Run { y, x0, x1: x, parent: i });
            // Runs touching on the row above (corners too) are the same blob.
            for j in previous.clone() {
                if runs[j].x0 <= x && runs[j].x1 >= x0 {
                    join(&mut runs, i, j);
                }
            }
        }
        previous = first..runs.len();
    }
    let mut boxes: Vec<Option<Blob>> = vec![None; runs.len()];
    for i in 0..runs.len() {
        let r = root(&mut runs, i);
        let run = &runs[i];
        let blob = boxes[r].get_or_insert(Blob {
            x0: run.x0,
            x1: run.x1,
            y0: run.y,
            y1: run.y + 1,
            area: 0,
        });
        blob.x0 = blob.x0.min(run.x0);
        blob.x1 = blob.x1.max(run.x1);
        blob.y1 = blob.y1.max(run.y + 1);
        blob.area += run.x1 - run.x0;
    }
    boxes.into_iter().flatten().collect()
}

/// The letter-like blobs that share a line with others: the letters.
fn letters(mut candidates: Vec<Blob>) -> Vec<Blob> {
    candidates.sort_unstable_by_key(|b| b.y0);
    let tops: Vec<u32> = candidates.iter().map(|b| b.y0).collect();
    let mut letters = Vec::new();
    for blob in &candidates {
        let from = tops.partition_point(|&t| t + LINE_TOLERANCE < blob.y0);
        let mates = candidates[from..]
            .iter()
            .take_while(|other| other.y0 <= blob.y0 + LINE_TOLERANCE)
            .filter(|other| !std::ptr::eq(*other, blob) && blob.on_one_line_with(other))
            .map(|other| other.letters())
            .sum::<u32>();
        if mates >= MIN_LINE_MATES || blob.lone_word() {
            letters.push(*blob);
        }
    }
    letters
}

/// The rows of the strip with lettering on them, as `(first row, one past last)`.
pub(super) fn lettered_spans(strip: &Strip, _screen: u32) -> Vec<(u32, u32)> {
    let (w, h) = (strip.width.div_ceil(STEP), strip.total.div_ceil(STEP));
    if w < 4 || h < 4 {
        return Vec::new();
    }
    // How many letters cross each looked-at row.
    let mut count = vec![0u32; h as usize];
    for tone in [&(|p: u8| p <= INK) as &dyn Fn(u8) -> bool, &|p: u8| p >= PALE] {
        let candidates: Vec<Blob> = blobs(strip, w, h, tone).into_iter().filter(Blob::letter_like).collect();
        for blob in letters(candidates) {
            for y in blob.y0..blob.y1 {
                count[y as usize] += blob.letters();
            }
        }
    }
    // Lines of lettering, and lines close together joined into one piece.
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut y = 0;
    while y < h {
        if count[y as usize] < LETTERS_PER_LINE {
            y += 1;
            continue;
        }
        let first = y;
        while y < h && count[y as usize] >= LETTERS_PER_LINE {
            y += 1;
        }
        match spans.last_mut() {
            Some(last) if first - last.1 <= LINE_GAP => last.1 = y,
            _ => spans.push((first, y)),
        }
    }
    spans
        .into_iter()
        .map(|(a, b)| (a * STEP, (b * STEP).min(strip.total)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{CropMode, Layout};

    /// A strip of `height` rows, with `draw` saying which pixels are ink.
    fn strip(height: u32, draw: impl Fn(u32, u32) -> bool) -> Strip {
        let img = GrayImage::from_fn(480, height, |x, y| if draw(x, y) { 0 } else { 255 }).unwrap();
        let mut o = ProcessOptions::manga_default(480, 800, crate::Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        build_strip(&[img], &o)
    }

    /// Letters: 12 by 16 boxes of ink with a hole, `n` of them along row `top`.
    fn line(x: u32, y: u32, top: u32, n: u32) -> bool {
        let (ix, iy) = ((x as i64 - 40) % 20, y as i64 - top as i64);
        (0..n as i64 * 20).contains(&(x as i64 - 40)) && (0..12).contains(&ix) && (0..16).contains(&iy) && !((3..9).contains(&ix) && (3..13).contains(&iy))
    }

    #[test]
    fn a_line_of_letters_is_lettering() {
        let s = strip(2000, |x, y| line(x, y, 1000, 8));
        let spans = lettered_spans(&s, 800);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].0 <= 1000 && spans[0].1 >= 1016, "{spans:?}");
    }

    #[test]
    fn lines_close_together_are_one_piece_of_lettering() {
        let s = strip(2000, |x, y| line(x, y, 1000, 6) || line(x, y, 1024, 6));
        let spans = lettered_spans(&s, 800);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert!(spans[0].0 <= 1000 && spans[0].1 >= 1040, "{spans:?}");
    }

    #[test]
    fn scattered_marks_are_not_lettering() {
        // The same marks, each on its own row: nothing shares a line.
        let s = strip(2000, |x, y| (0..8).any(|i| line(x, y, 1000 + i * 40, 8) && (x - 40) / 20 == i));
        assert!(lettered_spans(&s, 800).is_empty());
    }

    #[test]
    fn a_long_stroke_and_a_solid_bar_are_not_letters() {
        // A thin line the width of the strip, and a solid 200 by 10 bar.
        let s = strip(2000, |x, y| y == 1000 || ((1200..1210).contains(&y) && (100..300).contains(&x)));
        assert!(lettered_spans(&s, 800).is_empty());
    }

    #[test]
    fn pale_letters_on_a_dark_ground_count_too() {
        let img = GrayImage::from_fn(480, 2000, |x, y| if line(x, y, 1000, 8) { 255 } else { 20 }).unwrap();
        let mut o = ProcessOptions::manga_default(480, 800, crate::Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        let s = build_strip(&[img], &o);
        assert_eq!(lettered_spans(&s, 800).len(), 1);
    }
}
