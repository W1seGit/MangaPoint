//! The reader's decisions: where a page turn goes, what a key does, where to
//! start, what to remember. Plain data in, plain data out, so all of it is
//! tested without a window.

use library::{
    DownloadedChapter, DownloadedSeries, PageDirection, PageQuality, ReadFit, ReadMode,
    ReadProgress, ReaderPrefs, SeriesInfo,
};

// ---- What the reader looks like ---------------------------------------------

/// The zoom limits and step, in percent.
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

fn has_genre(info: Option<&SeriesInfo>, words: &[&str]) -> bool {
    info.is_some_and(|i| {
        i.genres.iter().any(|g| {
            let g = g.to_lowercase();
            words.iter().any(|w| g == *w || g.contains(w))
        })
    })
}

/// Which way a series is read when the user has not said: right to left for
/// manga, left to right for everything else.
pub fn default_direction(info: Option<&SeriesInfo>) -> PageDirection {
    if has_genre(info, &["manga"]) && !has_genre(info, &["manhwa", "manhua", "webtoon"]) {
        PageDirection::RightToLeft
    } else {
        PageDirection::LeftToRight
    }
}

/// How a series is read when the user has not said: as a strip for webtoons,
/// manhwa and manhua (named by the sources, or told by very tall pages), one
/// page at a time otherwise. `aspect` is the typical height over width of its
/// pages, if known.
pub fn default_mode(info: Option<&SeriesInfo>, aspect: Option<f32>) -> ReadMode {
    if has_genre(info, &["webtoon", "manhwa", "manhua", "long strip", "long-strip"]) {
        return ReadMode::Scroll;
    }
    if aspect.is_some_and(|a| a >= 2.2) {
        return ReadMode::Scroll;
    }
    ReadMode::Paged
}

/// What the user chose for a series, and the defaults for what they did not.
pub fn resolve_look(prefs: &ReaderPrefs, info: Option<&SeriesInfo>, aspect: Option<f32>) -> ReaderLook {
    let mode = prefs.mode.unwrap_or_else(|| default_mode(info, aspect));
    ReaderLook {
        mode,
        direction: prefs.direction.unwrap_or_else(|| default_direction(info)),
        // A page shown alone is seen whole; a strip fills the width.
        fit: prefs.fit.unwrap_or(match mode {
            ReadMode::Paged => ReadFit::Height,
            ReadMode::Scroll => ReadFit::Width,
        }),
        zoom: prefs.zoom.unwrap_or(100).clamp(ZOOM_MIN, ZOOM_MAX),
    }
}

/// The middle of the pages' height over width, for choosing a mode.
pub fn typical_aspect(sizes: &[(u32, u32)]) -> Option<f32> {
    let mut ratios: Vec<f32> = sizes
        .iter()
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| *h as f32 / *w as f32)
        .collect();
    if ratios.is_empty() {
        return None;
    }
    ratios.sort_by(f32::total_cmp);
    Some(ratios[ratios.len() / 2])
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

/// The one zoom setting the visitor sees: how the page is fitted, or a
/// percentage of that fit. It is stored as the fit plus the zoom factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomChoice {
    Fit,
    Width,
    Original,
    Percent(u16),
}

impl ZoomChoice {
    /// What the zoom box shows.
    pub fn label(self) -> String {
        match self {
            ZoomChoice::Fit => "Fit page".to_string(),
            ZoomChoice::Width => "Fit width".to_string(),
            ZoomChoice::Original => "Original size".to_string(),
            ZoomChoice::Percent(p) => format!("{p}%"),
        }
    }

    /// What was typed in the box: "125", "125%", "fit", "fit width", "original".
    pub fn parse(text: &str) -> Option<ZoomChoice> {
        let text = text.trim().to_lowercase();
        if text.contains("width") {
            return Some(ZoomChoice::Width);
        }
        if text.starts_with("orig") {
            return Some(ZoomChoice::Original);
        }
        if text.starts_with("fit") || text.starts_with("page") || text.starts_with("height") {
            return Some(ZoomChoice::Fit);
        }
        let number: u32 = text.trim_end_matches('%').trim().parse().ok()?;
        Some(ZoomChoice::Percent(number.clamp(u32::from(ZOOM_MIN), u32::from(ZOOM_MAX)) as u16))
    }
}

/// The choice that describes this fit and zoom.
pub fn zoom_choice(fit: ReadFit, zoom: u16) -> ZoomChoice {
    if zoom == 100 {
        match fit {
            ReadFit::Height => ZoomChoice::Fit,
            ReadFit::Width => ZoomChoice::Width,
            ReadFit::Original => ZoomChoice::Original,
        }
    } else {
        ZoomChoice::Percent(zoom)
    }
}

/// The fit and zoom a choice stands for. A percentage keeps the current fit.
pub fn apply_zoom_choice(choice: ZoomChoice, current_fit: ReadFit) -> (ReadFit, u16) {
    match choice {
        ZoomChoice::Fit => (ReadFit::Height, 100),
        ZoomChoice::Width => (ReadFit::Width, 100),
        ZoomChoice::Original => (ReadFit::Original, 100),
        ZoomChoice::Percent(p) => (current_fit, p.clamp(ZOOM_MIN, ZOOM_MAX)),
    }
}

/// How much of the chapter is behind the reader, in whole percent. `page` is
/// the zero-based page (`count` or more is the end card); `along` is how far
/// down the strip the reader is, 0 to 1.
pub fn percent_finished(paged: bool, page: usize, count: usize, along: f32) -> u32 {
    if count == 0 {
        return 0;
    }
    if paged {
        ((page.min(count) + 1).min(count) * 100 / count) as u32
    } else {
        (along.clamp(0.0, 1.0) * 100.0).round() as u32
    }
}

/// A swipe that starts at the left edge of the screen and goes right leaves
/// the book (the back gesture of phones).
/// Do the files made of a series for a phone or tablet say it is read right to
/// left? As the user reads it, or by the series' genres if they never said.
pub fn export_rtl(prefs: Option<&ReaderPrefs>, info: Option<&SeriesInfo>) -> bool {
    prefs.and_then(|p| p.direction).unwrap_or_else(|| default_direction(info)) == PageDirection::RightToLeft
}

// ---- Turning pages ----------------------------------------------------------

/// The page after this one, as (chapter, page). `counts` is the number of pages
/// of each chapter. A chapter has one position after its last page: the end,
/// where "Next chapter" is offered (page number = the number of pages). From
/// the end, the next chapter's first page is next, skipping chapters that have
/// no pages. `None` at the end of the last chapter.
pub fn paged_next(counts: &[usize], chapter: usize, page: usize) -> Option<(usize, usize)> {
    let count = *counts.get(chapter)?;
    if page < count {
        return Some((chapter, page + 1));
    }
    (chapter + 1..counts.len())
        .find(|&c| counts[c] > 0)
        .map(|c| (c, 0))
}

/// The page before this one, as (chapter, page). From a chapter's first page it
/// is the last page of the chapter before (skipping chapters with no pages).
/// `None` at the very start.
pub fn paged_prev(counts: &[usize], chapter: usize, page: usize) -> Option<(usize, usize)> {
    let count = *counts.get(chapter)?;
    if page > 0 {
        return Some((chapter, page.min(count).saturating_sub(1)));
    }
    (0..chapter)
        .rev()
        .find(|&c| counts[c] > 0)
        .map(|c| (c, counts[c] - 1))
}

/// A place held to what exists: a chapter that is there, a page up to the end
/// position (see [`paged_next`]).
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

/// Which third of the page was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Middle,
    Right,
}

/// The third of a `width` wide area that `x` is in. Anything outside it counts
/// as the nearest end.
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

/// What the reader can be told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Prev,
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

/// What pressing a third of the page does: the sides turn the page (which way
/// depends on the reading direction), the middle shows or hides the bar.
pub fn zone_action(zone: Zone, direction: PageDirection) -> Action {
    match (zone, direction) {
        (Zone::Middle, _) => Action::ToggleBar,
        (Zone::Left, PageDirection::LeftToRight) | (Zone::Right, PageDirection::RightToLeft) => Action::Prev,
        (Zone::Right, PageDirection::LeftToRight) | (Zone::Left, PageDirection::RightToLeft) => Action::Next,
    }
}

/// What a key does. `key` is the web view's name for it (`ArrowLeft`, `" "`,
/// `PageDown`, `[`...). `None` for a key the reader does not use.
pub fn key_action(key: &str, shift: bool, ctrl: bool, mode: ReadMode, direction: PageDirection) -> Option<Action> {
    let paged = mode == ReadMode::Paged;
    // The arrow that goes forward follows the direction pages are read in.
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

// ---- Where to start ---------------------------------------------------------

/// The chapters being read: key and number of pages, in reading order.
pub type ChapterList<'a> = [(&'a str, usize)];

/// Where reading starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub chapter: usize,
    pub page: usize,
    pub scroll: f32,
}

/// Where to start reading. With `wanted` (a chapter the user picked) that
/// chapter, resumed where they left it if that was the last one read;
/// otherwise where they left off, moving on to the next chapter when the last
/// one was finished; otherwise the first chapter that has not been read; a
/// series never read starts at its beginning. Progress for chapters that are
/// not there is ignored. `None` only when there are no chapters.
pub fn start_place(chapters: &ChapterList<'_>, progress: Option<&ReadProgress>, wanted: Option<&str>) -> Option<Start> {
    if chapters.is_empty() {
        return None;
    }
    let index_of = |key: &str| chapters.iter().position(|(k, _)| *k == key);
    let begin = |chapter: usize| Start { chapter, page: 0, scroll: 0.0 };
    let resume = |chapter: usize, p: &ReadProgress| Start {
        chapter,
        page: p.page.min(chapters[chapter].1.saturating_sub(1)),
        scroll: if p.scroll.is_finite() { p.scroll.clamp(0.0, 1.0) } else { 0.0 },
    };
    if let Some(key) = wanted {
        let at = index_of(key)?;
        return Some(match progress {
            Some(p) if p.chapter.as_deref() == Some(key) => resume(at, p),
            _ => begin(at),
        });
    }
    let Some(p) = progress else {
        return Some(begin(0));
    };
    if let Some(at) = p.chapter.as_deref().and_then(index_of) {
        let last = chapters[at].1.saturating_sub(1);
        let finished = p.is_read(chapters[at].0) && p.page >= last;
        if !finished {
            return Some(resume(at, p));
        }
        // That chapter is done: carry on with the next, if there is one.
        return Some(match (at + 1 < chapters.len()).then_some(at + 1) {
            Some(next) => begin(next),
            None => resume(at, p),
        });
    }
    let first_unread = chapters.iter().position(|(k, _)| !p.is_read(k)).unwrap_or(0);
    Some(begin(first_unread))
}

/// What a series card in the Library offers.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadButton {
    /// "Read", or "Continue · Ch. 12".
    pub label: String,
    /// How much of the series is read, 0.0 to 1.0, for the thin bar; `None`
    /// when nothing has been read.
    pub fraction: Option<f32>,
    /// The chapter to open.
    pub chapter: String,
}

/// "Ch. 12" for a numbered chapter, its name otherwise.
pub fn chapter_label(chapter: &DownloadedChapter) -> String {
    match chapter.number {
        Some(n) if n.fract().abs() < 0.0005 => format!("Ch. {}", n.round() as i64),
        Some(n) => format!("Ch. {}", format!("{n:.3}").trim_end_matches('0').trim_end_matches('.')),
        None if !chapter.name.trim().is_empty() => chapter.name.trim().to_string(),
        None => chapter.key.clone(),
    }
}

/// The reading button of a series' card: `None` for a series with nothing
/// downloaded.
pub fn read_button(series: &DownloadedSeries, progress: Option<&ReadProgress>) -> Option<ReadButton> {
    let first = series.chapters.first()?;
    let list: Vec<(&str, usize)> = series.chapters.iter().map(|c| (c.key.as_str(), c.pages)).collect();
    let started = progress.is_some_and(|p| p.last_read_at > 0 || !p.chapters.is_empty());
    if !started {
        return Some(ReadButton { label: "Read".into(), fraction: None, chapter: first.key.clone() });
    }
    let start = start_place(&list, progress, None)?;
    let chapter = &series.chapters[start.chapter];
    Some(ReadButton {
        label: format!("Continue · {}", chapter_label(chapter)),
        fraction: Some(read_fraction(series, progress)),
        chapter: chapter.key.clone(),
    })
}

/// How much of a series has been read: whole chapters count one each, the
/// chapter in progress by how far into it the reader is.
pub fn read_fraction(series: &DownloadedSeries, progress: Option<&ReadProgress>) -> f32 {
    let Some(p) = progress else { return 0.0 };
    if series.chapters.is_empty() {
        return 0.0;
    }
    let mut done = 0.0f32;
    for c in &series.chapters {
        if p.is_read(&c.key) {
            done += 1.0;
        } else if p.chapter.as_deref() == Some(c.key.as_str()) && c.pages > 0 {
            done += ((p.page + 1) as f32 / c.pages as f32).min(1.0) * 0.99;
        }
    }
    (done / series.chapters.len() as f32).clamp(0.0, 1.0)
}

// ---- Pages saved small --------------------------------------------------------

/// Is a chapter's picture the kind the app's own space saving makes: a JPEG of
/// one channel (gray)? Read from the first bytes of the first page.
pub fn looks_saved_small(first_page_head: &[u8]) -> bool {
    pipeline::pdf::jpeg_info(first_page_head).is_some_and(|i| i.components == 1)
}

/// Were this chapter's pages saved small for e-readers? Known from the quality
/// the download recorded; for older chapters (not recorded) from how the first
/// page looks (`first_page_gray`). Imported files and the sample pages were not
/// downloaded by the app and are never offered a download again.
pub fn saved_small(chapter: &DownloadedChapter, first_page_gray: bool) -> bool {
    if matches!(chapter.source_id.as_str(), "import" | "sample") || chapter.source_id.is_empty() {
        return false;
    }
    match chapter.quality {
        Some(PageQuality::Save) | Some(PageQuality::Ultra) => true,
        Some(PageQuality::Full) => false,
        None => first_page_gray,
    }
}

/// The chapters to offer a better download of, by key. `gray` says, for a
/// chapter whose quality is not recorded, whether its first page looks saved
/// small; it is asked only for those.
pub fn chapters_to_improve(chapters: &[DownloadedChapter], gray: &dyn Fn(&DownloadedChapter) -> bool) -> Vec<String> {
    chapters
        .iter()
        .filter(|c| c.pages > 0)
        .filter(|c| {
            let g = c.quality.is_none() && gray(c);
            saved_small(c, g)
        })
        .map(|c| c.key.clone())
        .collect()
}

/// The words of the banner.
pub const SMALL_PAGES_BANNER: &str = "These pages were saved small for e-readers. Download them again in original quality for the best reading.";

#[cfg(test)]
mod tests {

    #[test]
    fn the_zoom_box_reads_what_was_typed_and_a_choice_sets_the_fit() {
        assert_eq!(ZoomChoice::parse("125"), Some(ZoomChoice::Percent(125)));
        assert_eq!(ZoomChoice::parse("fit width"), Some(ZoomChoice::Width));
        assert_eq!(ZoomChoice::parse("Original size"), Some(ZoomChoice::Original));
        assert_eq!(ZoomChoice::parse("9999"), Some(ZoomChoice::Percent(ZOOM_MAX)));
        assert_eq!(ZoomChoice::parse("nonsense"), None);
        assert_eq!(apply_zoom_choice(ZoomChoice::Fit, ReadFit::Width), (ReadFit::Height, 100));
        assert_eq!(apply_zoom_choice(ZoomChoice::Percent(150), ReadFit::Width), (ReadFit::Width, 150));
        assert_eq!(zoom_choice(ReadFit::Width, 100), ZoomChoice::Width);
        assert_eq!(zoom_choice(ReadFit::Height, 150), ZoomChoice::Percent(150));
    }

    #[test]
    fn the_percent_finished_follows_the_page_or_the_scroll() {
        assert_eq!(percent_finished(true, 0, 10, 0.0), 10);
        assert_eq!(percent_finished(true, 9, 10, 0.0), 100);
        assert_eq!(percent_finished(false, 0, 10, 0.456), 46);
        assert_eq!(percent_finished(true, 0, 0, 0.0), 0);
    }
    use super::*;
    use PageDirection::{LeftToRight as Ltr, RightToLeft as Rtl};

    fn info(genres: &[&str]) -> SeriesInfo {
        SeriesInfo { genres: genres.iter().map(|g| g.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn manga_reads_right_to_left_and_everything_else_left_to_right() {
        assert_eq!(default_direction(Some(&info(&["Action", "Manga"]))), Rtl);
        assert_eq!(default_direction(Some(&info(&["manga"]))), Rtl);
        assert_eq!(default_direction(Some(&info(&["Action"]))), Ltr);
        assert_eq!(default_direction(Some(&info(&["Manhwa", "Manga"]))), Ltr, "a manhwa is read left to right");
        assert_eq!(default_direction(None), Ltr);
    }

    #[test]
    fn webtoons_and_very_tall_pages_default_to_the_strip() {
        assert_eq!(default_mode(Some(&info(&["Webtoon"])), None), ReadMode::Scroll);
        assert_eq!(default_mode(Some(&info(&["Action", "Manhwa"])), Some(1.4)), ReadMode::Scroll);
        assert_eq!(default_mode(Some(&info(&["Manga"])), Some(1.45)), ReadMode::Paged);
        assert_eq!(default_mode(None, Some(3.0)), ReadMode::Scroll);
        assert_eq!(default_mode(None, None), ReadMode::Paged);
        assert_eq!(typical_aspect(&[(100, 150), (100, 300), (100, 160), (0, 5)]), Some(1.6));
        assert_eq!(typical_aspect(&[]), None);
    }

    #[test]
    fn what_the_user_chose_wins_over_the_defaults() {
        let prefs = ReaderPrefs { mode: Some(ReadMode::Paged), direction: Some(Ltr), fit: Some(ReadFit::Height), zoom: Some(9000), ..Default::default() };
        let look = resolve_look(&prefs, Some(&info(&["Webtoon", "Manga"])), Some(3.0));
        assert_eq!(look, ReaderLook { mode: ReadMode::Paged, direction: Ltr, fit: ReadFit::Height, zoom: ZOOM_MAX });
        let look = resolve_look(&ReaderPrefs::default(), Some(&info(&["Manga"])), None);
        assert_eq!((look.mode, look.direction, look.fit, look.zoom), (ReadMode::Paged, Rtl, ReadFit::Height, 100));
        let strip = resolve_look(&ReaderPrefs::default(), Some(&info(&["Webtoon"])), None);
        assert_eq!((strip.mode, strip.fit), (ReadMode::Scroll, ReadFit::Width), "a strip keeps the width");
        let chosen = ReaderPrefs { fit: Some(ReadFit::Width), ..Default::default() };
        assert_eq!(resolve_look(&chosen, None, None).fit, ReadFit::Width, "what the user chose wins");
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
    }

    #[test]
    fn pages_turn_through_a_chapter_to_its_end_and_on_into_the_next() {
        let counts = [2, 3];
        assert_eq!(paged_next(&counts, 0, 0), Some((0, 1)));
        assert_eq!(paged_next(&counts, 0, 1), Some((0, 2)), "the end of the chapter is a place of its own");
        assert_eq!(paged_next(&counts, 0, 2), Some((1, 0)));
        assert_eq!(paged_next(&counts, 1, 2), Some((1, 3)));
        assert_eq!(paged_next(&counts, 1, 3), None, "nothing after the last chapter");
        assert_eq!(paged_prev(&counts, 1, 0), Some((0, 1)), "back to the last page of the chapter before");
        assert_eq!(paged_prev(&counts, 1, 3), Some((1, 2)), "from the end to the last page");
        assert_eq!(paged_prev(&counts, 0, 1), Some((0, 0)));
        assert_eq!(paged_prev(&counts, 0, 0), None);
        assert_eq!(paged_next(&counts, 9, 0), None, "a chapter that is not there");
        assert_eq!(paged_prev(&counts, 9, 0), None);
    }

    #[test]
    fn chapters_without_pages_are_stepped_over() {
        let counts = [2, 0, 0, 1];
        assert_eq!(paged_next(&counts, 0, 2), Some((3, 0)));
        assert_eq!(paged_prev(&counts, 3, 0), Some((0, 1)));
        // Standing in an empty chapter, on its message.
        assert_eq!(paged_next(&counts, 1, 0), Some((3, 0)));
        assert_eq!(paged_prev(&counts, 2, 0), Some((0, 1)));
        let all_empty = [0, 0];
        assert_eq!(paged_next(&all_empty, 0, 0), None);
        assert_eq!(paged_prev(&all_empty, 1, 0), None);
        assert_eq!(paged_next(&[], 0, 0), None);
    }

    #[test]
    fn a_place_is_held_to_what_exists() {
        assert_eq!(clamp_place(&[2, 3], 5, 99), Some((1, 3)));
        assert_eq!(clamp_place(&[2, 3], 0, 99), Some((0, 2)));
        assert_eq!(clamp_place(&[2, 3], 1, 1), Some((1, 1)));
        assert_eq!(clamp_place(&[], 0, 0), None);
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
    fn the_sides_of_the_page_turn_it_the_way_the_direction_reads() {
        assert_eq!(zone_action(Zone::Right, Ltr), Action::Next);
        assert_eq!(zone_action(Zone::Left, Ltr), Action::Prev);
        assert_eq!(zone_action(Zone::Right, Rtl), Action::Prev);
        assert_eq!(zone_action(Zone::Left, Rtl), Action::Next);
        assert_eq!(zone_action(Zone::Middle, Ltr), Action::ToggleBar);
        assert_eq!(zone_action(Zone::Middle, Rtl), Action::ToggleBar);
        assert_eq!(zone_at(10.0, 300.0), Zone::Left);
        assert_eq!(zone_at(150.0, 300.0), Zone::Middle);
        assert_eq!(zone_at(299.0, 300.0), Zone::Right);
        assert_eq!(zone_at(-5.0, 300.0), Zone::Left);
        assert_eq!(zone_at(900.0, 300.0), Zone::Right);
        assert_eq!(zone_at(5.0, 0.0), Zone::Middle);
        assert_eq!(zone_at(f64::NAN, 100.0), Zone::Middle);
    }

    #[test]
    fn keys_follow_the_mode_and_the_direction() {
        let paged = |k: &str, s: bool, d| key_action(k, s, false, ReadMode::Paged, d);
        let strip = |k: &str, s: bool| key_action(k, s, false, ReadMode::Scroll, Ltr);
        assert_eq!(paged("ArrowRight", false, Ltr), Some(Action::Next));
        assert_eq!(paged("ArrowLeft", false, Ltr), Some(Action::Prev));
        assert_eq!(paged("ArrowRight", false, Rtl), Some(Action::Prev));
        assert_eq!(paged("ArrowLeft", false, Rtl), Some(Action::Next));
        assert_eq!(paged("ArrowDown", false, Rtl), Some(Action::Next));
        assert_eq!(paged("PageDown", false, Ltr), Some(Action::Next));
        assert_eq!(paged("PageUp", false, Ltr), Some(Action::Prev));
        assert_eq!(paged(" ", false, Ltr), Some(Action::Next));
        assert_eq!(paged(" ", true, Ltr), Some(Action::Prev));
        assert_eq!(strip(" ", false), Some(Action::ScrollPages(1)));
        assert_eq!(strip(" ", true), Some(Action::ScrollPages(-1)));
        assert_eq!(strip("PageDown", false), Some(Action::ScrollPages(1)));
        assert_eq!(strip("PageUp", false), Some(Action::ScrollPages(-1)));
        assert_eq!(strip("ArrowDown", false), Some(Action::ScrollLines(1)));
        assert_eq!(strip("ArrowUp", false), Some(Action::ScrollLines(-1)));
        assert_eq!(strip("ArrowLeft", false), None, "sideways does nothing on a strip");
        assert_eq!(strip("Home", false), Some(Action::First));
        assert_eq!(strip("End", false), Some(Action::Last));
        for mode in [ReadMode::Paged, ReadMode::Scroll] {
            assert_eq!(key_action("[", false, false, mode, Ltr), Some(Action::PrevChapter));
            assert_eq!(key_action("]", false, false, mode, Rtl), Some(Action::NextChapter));
            assert_eq!(key_action("Escape", false, false, mode, Ltr), Some(Action::Back));
            assert_eq!(key_action("F11", false, false, mode, Ltr), Some(Action::Fullscreen));
            assert_eq!(key_action("+", false, false, mode, Ltr), Some(Action::ZoomIn));
            assert_eq!(key_action("=", false, true, mode, Ltr), Some(Action::ZoomIn));
            assert_eq!(key_action("-", false, false, mode, Ltr), Some(Action::ZoomOut));
            assert_eq!(key_action("0", false, true, mode, Ltr), Some(Action::ZoomReset));
            assert_eq!(key_action("a", false, false, mode, Ltr), None);
            assert_eq!(key_action("ArrowRight", false, true, mode, Ltr), None, "browser shortcuts are left alone");
            assert_eq!(key_action("r", false, true, mode, Ltr), None);
        }
    }

    fn chapter(key: &str, number: f64, pages: usize) -> DownloadedChapter {
        DownloadedChapter {
            key: key.into(),
            number: Some(number),
            name: format!("Chapter {number}"),
            source_id: "s".into(),
            pages,
            ..Default::default()
        }
    }

    fn series(chapters: Vec<DownloadedChapter>) -> DownloadedSeries {
        DownloadedSeries { id: "a-1".into(), title: "A".into(), chapters, ..Default::default() }
    }

    #[test]
    fn reading_starts_where_the_user_left_off() {
        let list = [("c1", 10), ("c2", 20), ("c3", 5)];
        // Never read: the beginning.
        assert_eq!(start_place(&list, None, None), Some(Start { chapter: 0, page: 0, scroll: 0.0 }));
        let mut p = ReadProgress::default();
        p.record("c2", 7, 0.35, 20, 100);
        assert_eq!(start_place(&list, Some(&p), None), Some(Start { chapter: 1, page: 7, scroll: 0.35 }));
        // A chapter picked by hand starts at its beginning, unless it is the one left.
        assert_eq!(start_place(&list, Some(&p), Some("c3")), Some(Start { chapter: 2, page: 0, scroll: 0.0 }));
        assert_eq!(start_place(&list, Some(&p), Some("c2")), Some(Start { chapter: 1, page: 7, scroll: 0.35 }));
        assert_eq!(start_place(&list, Some(&p), Some("c9")), None, "a chapter that is not there");
        // A chapter finished moves on to the next.
        p.record("c2", 19, 1.0, 20, 101);
        assert_eq!(start_place(&list, Some(&p), None), Some(Start { chapter: 2, page: 0, scroll: 0.0 }));
        // The last chapter finished stays where it is.
        p.record("c3", 4, 1.0, 5, 102);
        assert_eq!(start_place(&list, Some(&p), None), Some(Start { chapter: 2, page: 4, scroll: 1.0 }));
        assert_eq!(start_place(&[], Some(&p), None), None);
    }

    #[test]
    fn progress_for_chapters_that_are_gone_is_ignored() {
        let list = [("c1", 10), ("c2", 20)];
        let mut p = ReadProgress::default();
        p.record("c9", 4, 0.5, 30, 100);
        p.mark_read("c1", 10, 90);
        // The chapter it points at is gone: the first one not read.
        assert_eq!(start_place(&list, Some(&p), None), Some(Start { chapter: 1, page: 0, scroll: 0.0 }));
        // A page past the end of a chapter that got shorter is held to its last page.
        let mut q = ReadProgress::default();
        q.record("c1", 50, 0.2, 100, 1);
        assert_eq!(start_place(&list, Some(&q), None).unwrap().page, 9);
        // A scroll position that is not a number is the start.
        q.scroll = f32::NAN;
        assert_eq!(start_place(&list, Some(&q), None).unwrap().scroll, 0.0);
    }

    #[test]
    fn the_library_button_reads_or_continues_and_shows_how_far() {
        let s = series(vec![chapter("c1", 1.0, 10), chapter("c2", 2.0, 10), chapter("c3", 2.5, 10), chapter("c4", 4.0, 10)]);
        assert_eq!(read_button(&series(vec![]), None), None);
        let button = read_button(&s, None).unwrap();
        assert_eq!((button.label.as_str(), button.fraction, button.chapter.as_str()), ("Read", None, "c1"));
        let mut p = ReadProgress::default();
        p.mark_read("c1", 10, 5);
        p.record("c2", 4, 0.0, 10, 6);
        let button = read_button(&s, Some(&p)).unwrap();
        assert_eq!(button.label, "Continue · Ch. 2");
        assert_eq!(button.chapter, "c2");
        let f = button.fraction.unwrap();
        assert!(f > 0.25 && f < 0.5, "{f}");
        // Finishing a chapter points at the next, even a half-numbered one.
        p.record("c2", 9, 1.0, 10, 7);
        assert_eq!(read_button(&s, Some(&p)).unwrap().label, "Continue · Ch. 2.5");
        for c in ["c3", "c4"] {
            p.mark_read(c, 10, 8);
        }
        let all = read_button(&s, Some(&p)).unwrap();
        assert_eq!(all.fraction, Some(1.0));
        assert_eq!(read_fraction(&s, None), 0.0);
        assert_eq!(chapter_label(&DownloadedChapter { name: "Epilogue".into(), ..Default::default() }), "Epilogue");
    }

    #[test]
    fn the_banner_is_for_pages_known_or_seen_to_be_saved_small() {
        let mut c = chapter("c1", 1.0, 10);
        assert!(!saved_small(&c, false), "unknown and not gray: nothing is claimed");
        assert!(saved_small(&c, true), "unknown but gray");
        c.quality = Some(PageQuality::Save);
        assert!(saved_small(&c, false));
        c.quality = Some(PageQuality::Ultra);
        assert!(saved_small(&c, false));
        c.quality = Some(PageQuality::Full);
        assert!(!saved_small(&c, true), "recorded as original: a gray picture is just a gray picture");
        // Files the app did not download are never offered a download again.
        c.quality = Some(PageQuality::Save);
        c.source_id = "import".into();
        assert!(!saved_small(&c, true));
        c.source_id = String::new();
        assert!(!saved_small(&c, true));
    }

    #[test]
    fn only_the_chapters_that_would_gain_are_offered() {
        let mut saved = chapter("c1", 1.0, 5);
        saved.quality = Some(PageQuality::Save);
        let mut full = chapter("c2", 2.0, 5);
        full.quality = Some(PageQuality::Full);
        let unknown_gray = chapter("c3", 3.0, 5);
        let unknown_color = chapter("c4", 4.0, 5);
        let empty = chapter("c5", 5.0, 0);
        let all = vec![saved, full, unknown_gray, unknown_color, empty];
        let asked = std::cell::RefCell::new(Vec::new());
        let found = chapters_to_improve(&all, &|c| {
            asked.borrow_mut().push(c.key.clone());
            c.key == "c3"
        });
        assert_eq!(found, ["c1", "c3"]);
        assert_eq!(*asked.borrow(), ["c3", "c4"], "the first page is looked at only when the quality is not recorded");
    }

    #[test]
    fn a_gray_jpeg_is_told_from_a_color_one() {
        let jpeg = |channels: u8| {
            let mut v = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 8, 0, 5, 0, 7, channels];
            v.extend_from_slice(&[0; 20]);
            v
        };
        assert!(looks_saved_small(&jpeg(1)));
        assert!(!looks_saved_small(&jpeg(3)));
        assert!(!looks_saved_small(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_saved_small(&[]));
    }
}
