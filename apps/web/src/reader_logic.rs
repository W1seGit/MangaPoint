//! The reader's decisions: where a page turn goes, what a key, a tap or a swipe
//! does, how the zoom changes. A port of the pure part of the desktop app's
//! `logic/reader.rs` (that file also decides things about downloads and cannot
//! be compiled for the web), tested without a browser. The desktop's tests for
//! the same functions are repeated here so the two stay in step.

use crate::model::Book;
use library::{PageDirection, ReadFit, ReadMode, ReaderPrefs};
use web_engine::state::Prefs;

pub const ZOOM_MIN: u16 = 50;
pub const ZOOM_MAX: u16 = 400;
pub const ZOOM_STEP: u16 = 10;

/// The reader's settings once defaults are filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderLook {
    pub mode: ReadMode,
    pub direction: PageDirection,
    pub fit: ReadFit,
    pub zoom: u16,
}

/// What the visitor chose for a book, and the defaults for what they did not:
/// a strip for very tall pages, one page at a time otherwise; left to right
/// unless the book says it reads right to left.
pub fn resolve_look(prefs: &Prefs, book: &Book) -> ReaderLook {
    let mode = prefs.mode.unwrap_or(match book.aspect {
        Some(a) if a >= 2.2 => ReadMode::Scroll,
        _ => ReadMode::Paged,
    });
    ReaderLook {
        mode,
        direction: prefs.direction.unwrap_or(if book.rtl == Some(true) {
            PageDirection::RightToLeft
        } else {
            PageDirection::LeftToRight
        }),
        fit: prefs.fit.unwrap_or(match mode {
            ReadMode::Paged => ReadFit::Height,
            ReadMode::Scroll => ReadFit::Width,
        }),
        zoom: prefs.zoom.unwrap_or(100).clamp(ZOOM_MIN, ZOOM_MAX),
    }
}

pub fn prefs_of(p: &ReaderPrefs) -> Prefs {
    Prefs::from_reader_prefs(p)
}

pub fn zoom_step(zoom: u16, up: bool) -> u16 {
    let next = if up { zoom.saturating_add(ZOOM_STEP) } else { zoom.saturating_sub(ZOOM_STEP) };
    next.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The zoom after a turn of the wheel with Ctrl held (up is negative).
pub fn zoom_wheel(zoom: u16, delta_y: f64) -> u16 {
    if delta_y == 0.0 || !delta_y.is_finite() {
        zoom
    } else {
        zoom_step(zoom, delta_y < 0.0)
    }
}

/// A pinch ends at this many percent: rounded to the step, held to the limits.
pub fn zoom_from_pinch(pct: f64) -> u16 {
    if !pct.is_finite() {
        return 100;
    }
    let stepped = (pct / f64::from(ZOOM_STEP)).round() * f64::from(ZOOM_STEP);
    (stepped as i64).clamp(i64::from(ZOOM_MIN), i64::from(ZOOM_MAX)) as u16
}

// ---- turning pages ---------------------------------------------------------------

/// The page after this one, as (chapter, page). A chapter has one position
/// after its last page: the end, where "Next chapter" is offered (page =
/// number of pages). From the end, the next chapter's first page is next,
/// skipping empty chapters. `None` at the end of the last chapter.
pub fn paged_next(counts: &[usize], chapter: usize, page: usize) -> Option<(usize, usize)> {
    let count = *counts.get(chapter)?;
    if page < count {
        return Some((chapter, page + 1));
    }
    (chapter + 1..counts.len()).find(|&c| counts[c] > 0).map(|c| (c, 0))
}

/// The page before this one. From a chapter's first page it is the last page of
/// the chapter before (skipping empty ones). `None` at the very start.
pub fn paged_prev(counts: &[usize], chapter: usize, page: usize) -> Option<(usize, usize)> {
    let count = *counts.get(chapter)?;
    if page > 0 {
        return Some((chapter, page.min(count).saturating_sub(1)));
    }
    (0..chapter).rev().find(|&c| counts[c] > 0).map(|c| (c, counts[c] - 1))
}

/// A place held to what exists.
pub fn clamp_place(counts: &[usize], chapter: usize, page: usize) -> Option<(usize, usize)> {
    let chapter = chapter.min(counts.len().checked_sub(1)?);
    Some((chapter, page.min(counts[chapter])))
}

/// The chapter before or after this one, if there is one.
pub fn chapter_step(total: usize, current: usize, forward: bool) -> Option<usize> {
    if forward {
        (current + 1 < total).then_some(current + 1)
    } else {
        current.checked_sub(1).filter(|_| current < total)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Middle,
    Right,
}

/// The third of a `width` wide area that `x` is in.
pub fn zone_at(x: f64, width: f64) -> Zone {
    if !x.is_finite() || !width.is_finite() || width <= 0.0 {
        return Zone::Middle;
    }
    let f = x / width;
    if f < 1.0 / 3.0 {
        Zone::Left
    } else if f > 2.0 / 3.0 {
        Zone::Right
    } else {
        Zone::Middle
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Prev,
    NextChapter,
    PrevChapter,
    First,
    Last,
    ScrollPages(i32),
    ScrollLines(i32),
    ToggleBar,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fullscreen,
    Back,
}

/// What pressing a third of the page does: the sides turn the page (which way
/// depends on the reading direction), the middle shows or hides the bar.
pub fn zone_action(zone: Zone, direction: PageDirection) -> Action {
    match (zone, direction) {
        (Zone::Middle, _) => Action::ToggleBar,
        (Zone::Left, PageDirection::LeftToRight) | (Zone::Right, PageDirection::RightToLeft) => Action::Prev,
        (Zone::Right, PageDirection::LeftToRight) | (Zone::Left, PageDirection::RightToLeft) => Action::Next,
    }
}

/// A tap in the strip: the sides scroll a screen, the middle toggles the bar.
pub fn strip_tap_action(zone: Zone) -> Action {
    match zone {
        Zone::Middle => Action::ToggleBar,
        Zone::Left => Action::ScrollPages(-1),
        Zone::Right => Action::ScrollPages(1),
    }
}

/// What a swipe along the page does: dragging the page away turns to the next
/// one; with right-to-left reading the next page is on the left, so it is the
/// other way round.
pub fn swipe_action(dx: f64, direction: PageDirection) -> Option<Action> {
    if !dx.is_finite() || dx == 0.0 {
        return None;
    }
    let forward = match direction {
        PageDirection::LeftToRight => dx < 0.0,
        PageDirection::RightToLeft => dx > 0.0,
    };
    Some(if forward { Action::Next } else { Action::Prev })
}

/// What a key does. `key` is the browser's name for it (`ArrowLeft`, `" "`,
/// `PageDown`, `[`...). `None` for a key the reader does not use.
pub fn key_action(key: &str, shift: bool, ctrl: bool, mode: ReadMode, direction: PageDirection) -> Option<Action> {
    let paged = mode == ReadMode::Paged;
    let (forward_arrow, back_arrow) = match direction {
        PageDirection::LeftToRight => ("ArrowRight", "ArrowLeft"),
        PageDirection::RightToLeft => ("ArrowLeft", "ArrowRight"),
    };
    let forward = |paged_action: Action, strip: Action| if paged { paged_action } else { strip };
    Some(match key {
        k if ctrl && matches!(k, "+" | "=") => Action::ZoomIn,
        k if ctrl && k == "-" => Action::ZoomOut,
        k if ctrl && k == "0" => Action::ZoomReset,
        _ if ctrl => return None,
        "+" | "=" => Action::ZoomIn,
        "-" | "_" => Action::ZoomOut,
        "0" => Action::ZoomReset,
        "[" => Action::PrevChapter,
        "]" => Action::NextChapter,
        "Escape" => Action::Back,
        "F11" => Action::Fullscreen,
        "Home" => Action::First,
        "End" => Action::Last,
        k if paged && k == forward_arrow => Action::Next,
        k if paged && k == back_arrow => Action::Prev,
        "ArrowDown" => forward(Action::Next, Action::ScrollLines(1)),
        "ArrowUp" => forward(Action::Prev, Action::ScrollLines(-1)),
        "PageDown" => forward(Action::Next, Action::ScrollPages(1)),
        "PageUp" => forward(Action::Prev, Action::ScrollPages(-1)),
        " " | "Spacebar" if shift => forward(Action::Prev, Action::ScrollPages(-1)),
        " " | "Spacebar" => forward(Action::Next, Action::ScrollPages(1)),
        _ => return None,
    })
}

/// Which pages to keep ready beside the one being read in paged mode: the next
/// two in reading order (across chapters), as pages of the whole book.
pub fn preload_pages(book: &Book, chapter: usize, page: usize) -> Vec<usize> {
    let counts = book.counts();
    let mut out = Vec::new();
    let mut at = (chapter, page);
    // The page itself, then two ahead, then one behind.
    if let Some(c) = book.chapters.get(at.0).filter(|c| at.1 < c.count) {
        out.push(c.first + at.1);
    }
    // Two real pages ahead; the end card between chapters has no picture.
    let mut ahead = 0;
    for _ in 0..4 {
        match paged_next(&counts, at.0, at.1) {
            Some((c, p)) => {
                at = (c, p);
                if p < counts[c] {
                    out.push(book.chapters[c].first + p);
                    ahead += 1;
                    if ahead == 2 {
                        break;
                    }
                }
            }
            None => break,
        }
    }
    if let Some((c, p)) = paged_prev(&counts, chapter, page) {
        if p < counts[c] {
            out.push(book.chapters[c].first + p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Chapter;
    use PageDirection::{LeftToRight as Ltr, RightToLeft as Rtl};

    fn book(counts: &[usize]) -> Book {
        let mut first = 0;
        Book {
            chapters: counts
                .iter()
                .enumerate()
                .map(|(i, &count)| {
                    let c = Chapter { name: format!("C{i}"), number: None, first, count };
                    first += count;
                    c
                })
                .collect(),
            page_count: first,
            ..Book::default()
        }
    }

    #[test]
    fn defaults_follow_the_book_and_the_visitors_choices_win() {
        let plain = book(&[3]);
        let look = resolve_look(&Prefs::default(), &plain);
        assert_eq!((look.mode, look.direction, look.fit, look.zoom), (ReadMode::Paged, Ltr, ReadFit::Height, 100));
        let strip = Book { aspect: Some(3.0), ..plain.clone() };
        let look = resolve_look(&Prefs::default(), &strip);
        assert_eq!((look.mode, look.fit), (ReadMode::Scroll, ReadFit::Width));
        let manga = Book { rtl: Some(true), ..plain.clone() };
        assert_eq!(resolve_look(&Prefs::default(), &manga).direction, Rtl);
        let chosen = Prefs { mode: Some(ReadMode::Paged), direction: Some(Ltr), fit: Some(ReadFit::Original), zoom: Some(999) };
        let look = resolve_look(&chosen, &strip);
        assert_eq!((look.mode, look.direction, look.fit, look.zoom), (ReadMode::Paged, Ltr, ReadFit::Original, ZOOM_MAX));
    }

    #[test]
    fn zoom_steps_are_held_to_the_limits() {
        assert_eq!(zoom_step(100, true), 110);
        assert_eq!(zoom_step(ZOOM_MAX, true), ZOOM_MAX);
        assert_eq!(zoom_step(ZOOM_MIN, false), ZOOM_MIN);
        assert_eq!(zoom_wheel(100, -3.0), 110);
        assert_eq!(zoom_wheel(100, 3.0), 90);
        assert_eq!(zoom_wheel(100, 0.0), 100);
        assert_eq!(zoom_wheel(100, f64::NAN), 100);
        assert_eq!(zoom_from_pinch(137.0), 140);
        assert_eq!(zoom_from_pinch(5.0), ZOOM_MIN);
        assert_eq!(zoom_from_pinch(9000.0), ZOOM_MAX);
        assert_eq!(zoom_from_pinch(f64::INFINITY), 100);
    }

    #[test]
    fn turning_pages_crosses_chapters_through_the_end_card() {
        let counts = [2, 0, 3];
        assert_eq!(paged_next(&counts, 0, 0), Some((0, 1)));
        assert_eq!(paged_next(&counts, 0, 1), Some((0, 2)), "the end position");
        assert_eq!(paged_next(&counts, 0, 2), Some((2, 0)), "skips the empty chapter");
        assert_eq!(paged_next(&counts, 2, 3), None);
        assert_eq!(paged_next(&counts, 9, 0), None);
        assert_eq!(paged_prev(&counts, 2, 0), Some((0, 1)));
        assert_eq!(paged_prev(&counts, 0, 0), None);
        assert_eq!(paged_prev(&counts, 0, 2), Some((0, 1)));
        assert_eq!(clamp_place(&counts, 9, 9), Some((2, 3)));
        assert_eq!(clamp_place(&[], 0, 0), None);
        assert_eq!(chapter_step(3, 0, false), None);
        assert_eq!(chapter_step(3, 1, false), Some(0));
        assert_eq!(chapter_step(3, 2, true), None);
        assert_eq!(chapter_step(3, 7, false), None);
    }

    #[test]
    fn thirds_and_what_they_do() {
        assert_eq!(zone_at(10.0, 300.0), Zone::Left);
        assert_eq!(zone_at(150.0, 300.0), Zone::Middle);
        assert_eq!(zone_at(290.0, 300.0), Zone::Right);
        assert_eq!(zone_at(f64::NAN, 300.0), Zone::Middle);
        assert_eq!(zone_at(10.0, 0.0), Zone::Middle);
        assert_eq!(zone_action(Zone::Right, Ltr), Action::Next);
        assert_eq!(zone_action(Zone::Right, Rtl), Action::Prev);
        assert_eq!(zone_action(Zone::Left, Rtl), Action::Next);
        assert_eq!(zone_action(Zone::Middle, Rtl), Action::ToggleBar);
        assert_eq!(strip_tap_action(Zone::Right), Action::ScrollPages(1));
        assert_eq!(strip_tap_action(Zone::Left), Action::ScrollPages(-1));
    }

    #[test]
    fn swipes_follow_the_reading_direction() {
        assert_eq!(swipe_action(-120.0, Ltr), Some(Action::Next));
        assert_eq!(swipe_action(120.0, Ltr), Some(Action::Prev));
        assert_eq!(swipe_action(-120.0, Rtl), Some(Action::Prev));
        assert_eq!(swipe_action(120.0, Rtl), Some(Action::Next));
        assert_eq!(swipe_action(0.0, Ltr), None);
        assert_eq!(swipe_action(f64::NAN, Ltr), None);
    }

    #[test]
    fn keys() {
        use ReadMode::{Paged, Scroll};
        let k = |key: &str, shift, ctrl, mode, dir| key_action(key, shift, ctrl, mode, dir);
        assert_eq!(k("ArrowRight", false, false, Paged, Ltr), Some(Action::Next));
        assert_eq!(k("ArrowRight", false, false, Paged, Rtl), Some(Action::Prev));
        assert_eq!(k("ArrowLeft", false, false, Paged, Rtl), Some(Action::Next));
        assert_eq!(k("ArrowDown", false, false, Paged, Ltr), Some(Action::Next));
        assert_eq!(k("ArrowDown", false, false, Scroll, Ltr), Some(Action::ScrollLines(1)));
        assert_eq!(k(" ", false, false, Scroll, Ltr), Some(Action::ScrollPages(1)));
        assert_eq!(k(" ", true, false, Scroll, Ltr), Some(Action::ScrollPages(-1)));
        assert_eq!(k(" ", false, false, Paged, Ltr), Some(Action::Next));
        assert_eq!(k("PageUp", false, false, Paged, Ltr), Some(Action::Prev));
        assert_eq!(k("]", false, false, Paged, Ltr), Some(Action::NextChapter));
        assert_eq!(k("[", false, false, Scroll, Ltr), Some(Action::PrevChapter));
        assert_eq!(k("Home", false, false, Scroll, Ltr), Some(Action::First));
        assert_eq!(k("Escape", false, false, Scroll, Ltr), Some(Action::Back));
        assert_eq!(k("+", false, false, Paged, Ltr), Some(Action::ZoomIn));
        assert_eq!(k("=", false, true, Paged, Ltr), Some(Action::ZoomIn));
        assert_eq!(k("0", false, true, Paged, Ltr), Some(Action::ZoomReset));
        assert_eq!(k("c", false, true, Paged, Ltr), None, "Ctrl+C is the browser's");
        assert_eq!(k("q", false, false, Paged, Ltr), None);
    }

    #[test]
    fn the_next_pages_are_kept_ready_across_a_chapter_end() {
        let b = book(&[3, 2]);
        // On the last page of chapter 0: itself, the end card has no picture, then chapter 1.
        assert_eq!(preload_pages(&b, 0, 2), vec![2, 3, 4, 1]);
        assert_eq!(preload_pages(&b, 0, 0), vec![0, 1, 2]);
        assert_eq!(preload_pages(&b, 1, 0), vec![3, 4, 2]);
        assert_eq!(preload_pages(&b, 1, 2), vec![4], "on the end card only the page behind");
    }
}
