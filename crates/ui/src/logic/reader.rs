//! The reader's decisions that both apps make the same way: the zoom, what a key does, moving
//! between chapters and how far along a chapter is. A book is read as one long strip.

/// The zoom limits and step, in percent.
pub const ZOOM_MIN: u16 = 25;
pub const ZOOM_MAX: u16 = 500;
pub const ZOOM_STEP: u16 = 10;

/// The reader's settings once defaults are filled in: the zoom is a share of the width that suits
/// the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderLook {
    pub zoom: u16,
}

/// The zoom the reader chose for a book or series, or the default when they did not.
pub fn resolve_look(zoom: Option<u16>) -> ReaderLook {
    ReaderLook { zoom: zoom.unwrap_or(100).clamp(ZOOM_MIN, ZOOM_MAX) }
}

/// One zoom step up or down, held to the limits.
pub fn zoom_step(zoom: u16, up: bool) -> u16 {
    let next = if up {
        zoom.saturating_add(ZOOM_STEP)
    } else {
        zoom.saturating_sub(ZOOM_STEP)
    };
    next.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The zoom after a turn of the mouse wheel (up is negative, as the web view
/// reports it).
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

/// The one zoom setting the visitor sees: "Fit page", the page at the size that
/// suits the window (the full width of a phone, a comfortable column on a big
/// screen), or a percentage of that.
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

// ---- Moving about ------------------------------------------------------------

/// The chapter before or after this one, if there is one.
pub fn chapter_step(total: usize, current: usize, forward: bool) -> Option<usize> {
    if forward {
        (current + 1 < total).then_some(current + 1)
    } else {
        current.checked_sub(1).filter(|_| current < total)
    }
}

/// What the reader can be told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NextChapter,
    PrevChapter,
    First,
    Last,
    /// Scroll the strip by this many screens.
    ScrollPages(i32),
    /// Scroll the strip by this many lines.
    ScrollLines(i32),
    ToggleBar,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fullscreen,
    Back,
}

/// What a key does. `key` is the web view's name for it (`ArrowDown`, `" "`,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_user_chose_wins_over_the_defaults() {
        assert_eq!(resolve_look(None).zoom, 100);
        assert_eq!(resolve_look(Some(9000)).zoom, ZOOM_MAX);
        assert_eq!(resolve_look(Some(1)).zoom, ZOOM_MIN);
        assert_eq!(resolve_look(Some(150)).zoom, 150);
    }

    #[test]
    fn zoom_moves_in_steps_and_stops_at_its_limits() {
        assert_eq!(zoom_step(100, true), 110);
        assert_eq!(zoom_step(100, false), 90);
        assert_eq!(zoom_step(ZOOM_MAX, true), ZOOM_MAX);
        assert_eq!(zoom_step(ZOOM_MIN, false), ZOOM_MIN);
        assert_eq!(zoom_wheel(100, -120.0), 110);
        assert_eq!(zoom_wheel(100, 53.0), 90);
        assert_eq!(zoom_wheel(100, 0.0), 100);
        assert_eq!(zoom_wheel(100, f64::NAN), 100);
        assert_eq!(zoom_from_pinch(137.0), 140);
        assert_eq!(zoom_from_pinch(5.0), ZOOM_MIN);
        assert_eq!(zoom_from_pinch(9000.0), ZOOM_MAX);
        assert_eq!(zoom_from_pinch(f64::INFINITY), 100);
    }

    #[test]
    fn chapters_step_forward_and_back_within_the_list() {
        assert_eq!(chapter_step(3, 0, true), Some(1));
        assert_eq!(chapter_step(3, 2, true), None);
        assert_eq!(chapter_step(3, 0, false), None);
        assert_eq!(chapter_step(3, 2, false), Some(1));
        assert_eq!(chapter_step(0, 0, true), None);
        assert_eq!(chapter_step(3, 7, false), None, "a chapter that is not there");
    }

    #[test]
    fn keys_scroll_the_strip() {
        let strip = |k: &str, s: bool| key_action(k, s, false);
        assert_eq!(strip(" ", false), Some(Action::ScrollPages(1)));
        assert_eq!(strip(" ", true), Some(Action::ScrollPages(-1)));
        assert_eq!(strip("PageDown", false), Some(Action::ScrollPages(1)));
        assert_eq!(strip("PageUp", false), Some(Action::ScrollPages(-1)));
        assert_eq!(strip("ArrowDown", false), Some(Action::ScrollLines(1)));
        assert_eq!(strip("ArrowUp", false), Some(Action::ScrollLines(-1)));
        assert_eq!(strip("ArrowLeft", false), None, "sideways does nothing on a strip");
        assert_eq!(strip("Home", false), Some(Action::First));
        assert_eq!(strip("End", false), Some(Action::Last));
        assert_eq!(key_action("[", false, false), Some(Action::PrevChapter));
        assert_eq!(key_action("]", false, false), Some(Action::NextChapter));
        assert_eq!(key_action("Escape", false, false), Some(Action::Back));
        assert_eq!(key_action("F11", false, false), Some(Action::Fullscreen));
        assert_eq!(key_action("+", false, false), Some(Action::ZoomIn));
        assert_eq!(key_action("=", false, true), Some(Action::ZoomIn));
        assert_eq!(key_action("-", false, false), Some(Action::ZoomOut));
        assert_eq!(key_action("0", false, true), Some(Action::ZoomReset));
        assert_eq!(key_action("a", false, false), None);
        assert_eq!(key_action("ArrowDown", false, true), None, "browser shortcuts are left alone");
        assert_eq!(key_action("r", false, true), None);
    }

    #[test]
    fn the_zoom_box_reads_what_was_typed_and_a_choice_sets_the_zoom() {
        assert_eq!(ZoomChoice::parse("125"), Some(ZoomChoice::Percent(125)));
        assert_eq!(ZoomChoice::parse("Fit page"), Some(ZoomChoice::Fit));
        assert_eq!(ZoomChoice::parse("9999"), Some(ZoomChoice::Percent(ZOOM_MAX)));
        assert_eq!(ZoomChoice::parse("nonsense"), None);
        assert_eq!(apply_zoom_choice(ZoomChoice::Fit), 100);
        assert_eq!(apply_zoom_choice(ZoomChoice::Percent(150)), 150);
        assert_eq!(zoom_choice(100), ZoomChoice::Fit);
        assert_eq!(zoom_choice(150), ZoomChoice::Percent(150));
    }

    #[test]
    fn the_percent_finished_follows_the_scroll() {
        assert_eq!(percent_finished(0.0), 0);
        assert_eq!(percent_finished(0.456), 46);
        assert_eq!(percent_finished(7.0), 100);
    }
}
