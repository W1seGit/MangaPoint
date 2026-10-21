//! The reader's decisions: what a key, a tap or a pinch does, how the zoom
//! changes. A port of the pure part of the desktop app's `logic/reader.rs` (that
//! file also decides things about downloads and cannot be compiled for the web),
//! tested without a browser. The desktop's tests for the same functions are
//! repeated here so the two stay in step. Books are read as one long strip.

use library::ReaderPrefs;
use web_engine::state::Prefs;

/// The zoom limits and step, in percent.
pub const ZOOM_MIN: u16 = 25;
pub const ZOOM_MAX: u16 = 500;
pub const ZOOM_STEP: u16 = 10;

/// The reader's settings once defaults are filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderLook {
    pub zoom: u16,
}

/// What the visitor chose for a book, and the defaults for what they did not.
pub fn resolve_look(prefs: &Prefs) -> ReaderLook {
    ReaderLook { zoom: prefs.zoom.unwrap_or(100).clamp(ZOOM_MIN, ZOOM_MAX) }
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

/// The chapter before or after this one, if there is one.
pub fn chapter_step(total: usize, current: usize, forward: bool) -> Option<usize> {
    if forward {
        (current + 1 < total).then_some(current + 1)
    } else {
        current.checked_sub(1).filter(|_| current < total)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
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

/// What a key does. `key` is the browser's name for it (`ArrowDown`, `" "`,
/// `PageDown`, `[`...). `None` for a key the reader does not use.
pub fn key_action(key: &str, shift: bool, ctrl: bool) -> Option<Action> {
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
        "ArrowDown" => Action::ScrollLines(1),
        "ArrowUp" => Action::ScrollLines(-1),
        "PageDown" => Action::ScrollPages(1),
        "PageUp" => Action::ScrollPages(-1),
        " " | "Spacebar" if shift => Action::ScrollPages(-1),
        " " | "Spacebar" => Action::ScrollPages(1),
        _ => return None,
    })
}

/// The one zoom setting the visitor sees: "Fit page", the page at the size
/// that suits the window (the full width of a phone, a comfortable column on a
/// big screen), or a percentage of that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomChoice {
    Fit,
    Percent(u16),
}

impl ZoomChoice {
    /// What the zoom box shows.
    pub fn label(self) -> String {
        match self {
            ZoomChoice::Fit => "Fit page".to_string(),
            ZoomChoice::Percent(p) => format!("{p}%"),
        }
    }

    /// What was typed in the box: "125", "125%", "fit".
    pub fn parse(text: &str) -> Option<ZoomChoice> {
        let text = text.trim().to_lowercase();
        if text.starts_with("fit") || text.starts_with("page") {
            return Some(ZoomChoice::Fit);
        }
        let number: u32 = text.trim_end_matches('%').trim().parse().ok()?;
        Some(ZoomChoice::Percent(number.clamp(u32::from(ZOOM_MIN), u32::from(ZOOM_MAX)) as u16))
    }
}

/// The choice that describes this zoom.
pub fn zoom_choice(zoom: u16) -> ZoomChoice {
    if zoom == 100 {
        ZoomChoice::Fit
    } else {
        ZoomChoice::Percent(zoom)
    }
}

/// The zoom a choice stands for.
pub fn apply_zoom_choice(choice: ZoomChoice) -> u16 {
    match choice {
        ZoomChoice::Fit => 100,
        ZoomChoice::Percent(p) => p.clamp(ZOOM_MIN, ZOOM_MAX),
    }
}

/// How much of the chapter is behind the reader, in whole percent: `along` is
/// how far down the strip the reader is, 0 to 1.
pub fn percent_finished(along: f32) -> u32 {
    (along.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// A swipe that starts at the left edge of the screen and goes right leaves
/// the book (the back gesture of phones).
pub fn is_edge_back(start_x: f64, dx: f64, dy: f64) -> bool {
    start_x.is_finite() && start_x <= 28.0 && dx > 70.0 && dx > dy.abs() * 1.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_the_visitors_choice() {
        assert_eq!(resolve_look(&Prefs::default()).zoom, 100);
        let chosen = Prefs { zoom: Some(9999), ..Prefs::default() };
        assert_eq!(resolve_look(&chosen).zoom, ZOOM_MAX);
        let chosen = Prefs { zoom: Some(1), ..Prefs::default() };
        assert_eq!(resolve_look(&chosen).zoom, ZOOM_MIN);
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
    fn chapters_step_one_at_a_time() {
        assert_eq!(chapter_step(3, 0, false), None);
        assert_eq!(chapter_step(3, 1, false), Some(0));
        assert_eq!(chapter_step(3, 2, true), None);
        assert_eq!(chapter_step(3, 7, false), None);
    }

    #[test]
    fn keys() {
        let k = |key: &str, shift, ctrl| key_action(key, shift, ctrl);
        assert_eq!(k("ArrowDown", false, false), Some(Action::ScrollLines(1)));
        assert_eq!(k("ArrowUp", false, false), Some(Action::ScrollLines(-1)));
        assert_eq!(k(" ", false, false), Some(Action::ScrollPages(1)));
        assert_eq!(k(" ", true, false), Some(Action::ScrollPages(-1)));
        assert_eq!(k("PageUp", false, false), Some(Action::ScrollPages(-1)));
        assert_eq!(k("]", false, false), Some(Action::NextChapter));
        assert_eq!(k("[", false, false), Some(Action::PrevChapter));
        assert_eq!(k("Home", false, false), Some(Action::First));
        assert_eq!(k("Escape", false, false), Some(Action::Back));
        assert_eq!(k("+", false, false), Some(Action::ZoomIn));
        assert_eq!(k("=", false, true), Some(Action::ZoomIn));
        assert_eq!(k("0", false, true), Some(Action::ZoomReset));
        assert_eq!(k("c", false, true), None, "Ctrl+C is the browser's");
        assert_eq!(k("ArrowRight", false, false), None);
        assert_eq!(k("q", false, false), None);
    }

    #[test]
    fn one_zoom_setting_covers_fit_and_percent() {
        assert_eq!(zoom_choice(100), ZoomChoice::Fit);
        assert_eq!(zoom_choice(150), ZoomChoice::Percent(150));
        assert_eq!(apply_zoom_choice(ZoomChoice::Fit), 100);
        assert_eq!(apply_zoom_choice(ZoomChoice::Percent(125)), 125);
        assert_eq!(apply_zoom_choice(ZoomChoice::Percent(9000)), ZOOM_MAX);
        assert_eq!(ZoomChoice::Percent(125).label(), "125%");
        assert_eq!(ZoomChoice::Fit.label(), "Fit page");
    }

    #[test]
    fn typed_zooms_are_understood() {
        assert_eq!(ZoomChoice::parse("fit"), Some(ZoomChoice::Fit));
        assert_eq!(ZoomChoice::parse("Fit page"), Some(ZoomChoice::Fit));
        assert_eq!(ZoomChoice::parse(" 150 % "), Some(ZoomChoice::Percent(150)));
        assert_eq!(ZoomChoice::parse("10"), Some(ZoomChoice::Percent(ZOOM_MIN)));
        assert_eq!(ZoomChoice::parse("nonsense"), None);
    }

    #[test]
    fn percent_finished_of_a_chapter() {
        assert_eq!(percent_finished(0.0), 0);
        assert_eq!(percent_finished(0.426), 43);
        assert_eq!(percent_finished(7.0), 100);
    }

    #[test]
    fn the_back_swipe_starts_at_the_left_edge() {
        assert!(is_edge_back(6.0, 120.0, 10.0));
        assert!(!is_edge_back(200.0, 120.0, 10.0));
        assert!(!is_edge_back(6.0, 40.0, 0.0));
        assert!(!is_edge_back(6.0, 100.0, 90.0));
        assert!(!is_edge_back(6.0, -120.0, 0.0));
    }
}
