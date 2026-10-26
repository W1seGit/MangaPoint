//! The reader's decisions: where a page turn goes, what a key does, where to
//! start, what to remember. Plain data in, plain data out, so all of it is
//! tested without a window. The parts the website shares (the zoom, the keys, moving between
//! chapters) are in `mangapoint_ui::logic::reader`.

use library::{
    DownloadedChapter, DownloadedSeries, PageDirection, PageQuality, ReadProgress, ReaderPrefs, SeriesInfo,
};
pub use mangapoint_ui::logic::reader::*;

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

/// Do the files made of a series for a phone or tablet say it is read right to
/// left? As the user reads it, or by the series' genres if they never said.
pub fn export_rtl(prefs: Option<&ReaderPrefs>, info: Option<&SeriesInfo>) -> bool {
    prefs.and_then(|p| p.direction).unwrap_or_else(|| default_direction(info)) == PageDirection::RightToLeft
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
