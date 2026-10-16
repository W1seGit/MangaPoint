//! The app's decisions that do not depend on the UI, so they can be tested.

use library::{Settings, Verdict};
use pipeline::{ExportTarget, JobEvent};
use search::{MergedChapter, ScoredMember};
use std::collections::HashSet;
use std::path::Path;

/// The best entry from each source, at most `limit` of them. `members` must
/// be ordered best first, as search results are.
///
/// A source can list one series twice (a duplicate page, another edition).
/// Loading and merging both adds nothing, and the screens key their lists on
/// the source, so each source appears once.
pub fn one_per_source(members: &[ScoredMember], limit: usize) -> Vec<ScoredMember> {
    let mut seen = HashSet::new();
    members
        .iter()
        .filter(|m| seen.insert(m.candidate.source_id.clone()))
        .take(limit)
        .cloned()
        .collect()
}

// ---- The chapter table -----------------------------------------------------

/// Which column the chapter table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Number,
    Name,
    Sources,
    Date,
}

/// What clicking a column header does: sorting by a new column uses that
/// column's natural direction (newest and biggest first, names A to Z);
/// clicking the current column flips the direction.
pub fn next_sort(current: (SortKey, bool), clicked: SortKey) -> (SortKey, bool) {
    if current.0 == clicked {
        (clicked, !current.1)
    } else {
        let descending = !matches!(clicked, SortKey::Name);
        (clicked, descending)
    }
}

/// Newest upload time among the sources that have the chapter (0 if unknown).
pub fn latest_upload(chapter: &MergedChapter) -> i64 {
    chapter
        .options
        .iter()
        .map(|o| o.upload_date)
        .max()
        .unwrap_or(0)
}

/// Indices of the chapters to show: those matching every word of `filter`
/// (in the name or the number), ordered by `key`. Chapters with no value for
/// the sort column (no number, no date) always come last.
pub fn visible_chapters(
    merged: &[MergedChapter],
    filter: &str,
    key: SortKey,
    descending: bool,
) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<usize> = (0..merged.len())
        .filter(|&i| {
            let ch = &merged[i];
            let haystack = format!(
                "{} {}",
                ch.name.to_lowercase(),
                ch.number.map(format_number).unwrap_or_default()
            );
            words.iter().all(|w| haystack.contains(w.as_str()))
        })
        .collect();

    // Value to sort on; `None` sorts after everything else.
    let value = |i: usize| -> Option<f64> {
        let ch = &merged[i];
        match key {
            SortKey::Number => ch.number,
            SortKey::Sources => Some(ch.options.len() as f64),
            SortKey::Date => {
                let d = latest_upload(ch);
                (d > 0).then_some(d as f64)
            }
            SortKey::Name => Some(0.0),
        }
    };
    let (mut present, absent): (Vec<usize>, Vec<usize>) =
        shown.drain(..).partition(|&i| value(i).is_some());
    present.sort_by(|&a, &b| {
        let ord = if key == SortKey::Name {
            merged[a]
                .name
                .to_lowercase()
                .cmp(&merged[b].name.to_lowercase())
        } else {
            value(a).unwrap().total_cmp(&value(b).unwrap())
        };
        // Ties keep the chapters' own order (by number, then position).
        let ord = ord.then_with(|| {
            merged[a]
                .number
                .unwrap_or(f64::MAX)
                .total_cmp(&merged[b].number.unwrap_or(f64::MAX))
        });
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });
    present.extend(absent);
    present
}

/// `12`, `12.5`: whole numbers without a decimal point.
pub fn format_number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `2024-05-03` from milliseconds since the Unix epoch; empty if unknown.
/// (UTC; computed directly so no date library is needed.)
pub fn format_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let days = ms.div_euclid(86_400_000);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

// ---- Choosing chapters -----------------------------------------------------

/// Indices of chapters matching text like `1-10, 15, 20-`.
///
/// `numbers` holds each chapter's number (`None` for unnumbered ones, which
/// text can never select). A bare number selects that chapter, `a-b` selects
/// the inclusive range and `a-` selects from `a` to the end.
pub fn select_by_text(text: &str, numbers: &[Option<f64>]) -> Result<Vec<usize>, String> {
    let mut wanted: Vec<(f64, f64)> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let parse = |s: &str| {
            s.trim()
                .parse::<f64>()
                .map_err(|_| format!("'{}' is not a chapter number", s.trim()))
        };
        match part.split_once('-') {
            None => {
                let n = parse(part)?;
                wanted.push((n, n));
            }
            Some((a, "")) => wanted.push((parse(a)?, f64::INFINITY)),
            Some((a, b)) => {
                let (a, b) = (parse(a)?, parse(b)?);
                if a > b {
                    return Err(format!("the range {a}-{b} runs backwards"));
                }
                wanted.push((a, b));
            }
        }
    }
    if wanted.is_empty() {
        return Err("type chapter numbers such as 1-10, 15".into());
    }
    Ok(numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let n = (*n)?;
            wanted
                .iter()
                .any(|&(a, b)| n >= a - 0.0005 && n <= b + 0.0005)
                .then_some(i)
        })
        .collect())
}

// ---- Where files go --------------------------------------------------------

/// Where the user chose to send the result.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetChoice {
    /// The configured output folder (or the app's default one).
    Folder,
    /// The reader's drive (or SD card).
    Usb,
    /// The reader over Wi-Fi.
    WiFi,
}

pub fn export_target(
    choice: &TargetChoice,
    settings: &Settings,
    default_dir: &Path,
) -> Result<ExportTarget, String> {
    match choice {
        TargetChoice::Folder => Ok(ExportTarget::Folder {
            path: settings
                .export
                .output_dir
                .clone()
                .unwrap_or_else(|| default_dir.to_path_buf()),
        }),
        TargetChoice::Usb => {
            let setup = settings.reader_setup();
            match setup.reader_dir {
                Some(dir) => Ok(ExportTarget::Device {
                    mount: dir,
                    folder: setup.folder,
                }),
                None => Err("Choose where the reader is first (under Send to).".into()),
            }
        }
        TargetChoice::WiFi => {
            let setup = settings.reader_setup();
            match setup.wifi_url.as_deref() {
                Some(url) if !url.trim().is_empty() => Ok(ExportTarget::WebDav {
                    base_url: url.trim().to_string(),
                    folder: setup.folder,
                }),
                _ => Err("Add the reader's Wi-Fi address first (under Send to).".into()),
            }
        }
    }
}

/// Where a chapter stands in the Downloads tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterStatus {
    Saved,
    Downloading,
    Waiting,
    Failed,
}

// ---- Progress --------------------------------------------------------------

/// One chapter being worked on right now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveChapter {
    pub name: String,
    pub pages_done: usize,
    pub pages_total: usize,
}

/// What the progress bar shows. Chapters can download together, so it keeps
/// every chapter in progress, not just one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobProgress {
    pub chapter_total: usize,
    /// Chapters completely dealt with.
    pub finished: usize,
    /// Chapters under way, by their position in the job.
    pub active: std::collections::BTreeMap<usize, ActiveChapter>,
    pub note: Option<String>,
    /// When the first chapter began, and how many pages have arrived since.
    pub started: Option<std::time::Instant>,
    pub pages_seen: usize,
    /// The user pressed cancel; the job is winding down.
    pub stopping: bool,
    /// Set once the finished files are being sent to their target.
    pub writing: Option<WriteProgress>,
}

/// How far the finished files have got to the drive, folder or reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriteProgress {
    /// The file being sent now.
    pub file: String,
    /// Bytes stored so far, of all the files, and the bytes there are in all.
    pub done: u64,
    pub total: u64,
    /// The drive has everything and is being made to keep it.
    pub flushing: bool,
    /// When the first file began.
    began: Option<std::time::Instant>,
    /// Recent (time, bytes stored), for a speed that follows the drive as it
    /// slows down or speeds up instead of averaging the whole copy.
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
}

impl WriteProgress {
    /// How long back the speed looks.
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

    fn record(&mut self, done: u64, total: u64) {
        let now = std::time::Instant::now();
        self.done = done;
        self.total = total;
        self.samples.push_back((now, done));
        while self.samples.len() > 2
            && self
                .samples
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) > Self::WINDOW)
        {
            self.samples.pop_front();
        }
    }

    /// Bytes a second over the whole copy, for the log.
    pub fn average_bytes_per_second(&self) -> Option<f64> {
        let seconds = self.began?.elapsed().as_secs_f64();
        (seconds > 0.0 && self.done > 0).then(|| self.done as f64 / seconds)
    }

    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }

    /// Bytes a second over the last few seconds, once there is enough to say.
    pub fn bytes_per_second(&self) -> Option<f64> {
        let (first_at, first) = *self.samples.front()?;
        let (last_at, last) = *self.samples.back()?;
        let seconds = last_at.duration_since(first_at).as_secs_f64();
        (seconds >= 0.5 && last > first).then(|| (last - first) as f64 / seconds)
    }

    /// Seconds left at the current speed.
    pub fn eta_secs(&self) -> Option<u64> {
        let speed = self.bytes_per_second()?;
        (self.done < self.total).then(|| ((self.total - self.done) as f64 / speed).ceil() as u64)
    }
}

impl JobProgress {
    pub fn apply(&mut self, event: &JobEvent) {
        match event {
            JobEvent::ChapterStarted { index, total, name } => {
                self.chapter_total = *total;
                self.started.get_or_insert_with(std::time::Instant::now);
                self.active.insert(
                    *index,
                    ActiveChapter {
                        name: name.clone(),
                        ..Default::default()
                    },
                );
                self.note = None;
            }
            JobEvent::PageDone {
                chapter_index,
                done,
                total,
            } => {
                self.pages_seen += 1;
                let chapter = self.active.entry(*chapter_index).or_default();
                chapter.pages_done = *done;
                chapter.pages_total = *total;
            }
            JobEvent::UsedFallback { to, .. } => {
                self.note = Some(format!("That source failed, trying {to}."));
            }
            JobEvent::ChapterSkipped { name, .. } => {
                self.note = Some(format!("Skipped {name}: no source could supply it."));
            }
            JobEvent::ChapterSaved { .. } => {}
            JobEvent::ChapterDone { index } => {
                if self.active.remove(index).is_some() {
                    self.finished += 1;
                }
            }
            JobEvent::PagesAtOnce { now, chosen } => {
                self.note = (now < chosen).then(|| {
                    format!(
                        "The source is pushing back, so downloads are slowed to {now} at a time."
                    )
                });
            }
            JobEvent::Flushing => {
                if let Some(writing) = self.writing.as_mut() {
                    writing.flushing = true;
                }
            }
            JobEvent::Writing { file } => {
                self.active.clear();
                self.finished = self.chapter_total;
                self.note = None;
                let writing = self.writing.get_or_insert_with(WriteProgress::default);
                writing.file = file.clone();
                writing.flushing = false;
                writing.began.get_or_insert_with(std::time::Instant::now);
            }
            JobEvent::Sent { done, total } => {
                if let Some(writing) = self.writing.as_mut() {
                    writing.record(*done, *total);
                }
            }
        }
    }

    /// How fast pages are arriving, once there is enough to say.
    pub fn pages_per_second(&self) -> Option<f32> {
        if self.writing.is_some() {
            return None;
        }
        let seconds = self.started?.elapsed().as_secs_f32();
        (seconds >= 4.0 && self.pages_seen >= 6).then(|| self.pages_seen as f32 / seconds)
    }

    /// Seconds left at the pace so far, once there is enough to say.
    pub fn eta_secs(&self) -> Option<u64> {
        if self.writing.is_some() {
            return None;
        }
        let seconds = self.started?.elapsed().as_secs_f32();
        let done = self.fraction();
        (seconds >= 4.0 && (0.03..1.0).contains(&done))
            .then(|| (seconds * (1.0 - done) / done).round() as u64)
    }

    /// Overall progress, 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        // Once the files are being sent, the bar is about that.
        if let Some(fraction) = self.writing.as_ref().and_then(WriteProgress::fraction) {
            return fraction;
        }
        if self.chapter_total == 0 {
            return 0.0;
        }
        let partial: f32 = self
            .active
            .values()
            .filter(|c| c.pages_total > 0)
            .map(|c| c.pages_done as f32 / c.pages_total as f32)
            .sum();
        ((self.finished as f32 + partial) / self.chapter_total as f32).clamp(0.0, 1.0)
    }

    pub fn label(&self) -> String {
        if self.stopping {
            return "Stopping…".into();
        }
        if self.chapter_total == 0 {
            return "Starting…".into();
        }
        match self.active.len() {
            0 if self.finished == 0 => "Starting…".into(),
            0 => format!("{} of {} chapters done", self.finished, self.chapter_total),
            1 => {
                let (index, chapter) = self.active.iter().next().expect("one entry");
                let pages = if chapter.pages_total > 0 {
                    format!(", page {} of {}", chapter.pages_done, chapter.pages_total)
                } else {
                    String::new()
                };
                format!("Chapter {} of {}{pages}", index + 1, self.chapter_total)
            }
            n => format!(
                "{} of {} chapters done, {n} in progress",
                self.finished, self.chapter_total
            ),
        }
    }
}

// ---- Export size -----------------------------------------------------------

/// A rough size for a reader file: each page is a fixed size in the format
/// (about 48 KB for black and white, 96 KB for four grays). Split spreads make
/// it a little bigger.
pub fn estimate_export_bytes(pages: usize, format: converter::xtc::Format) -> u64 {
    let per_page: u64 = match format {
        converter::xtc::Format::Xtc => 48_022,
        converter::xtc::Format::Xtch => 96_022,
    };
    pages as u64 * per_page
}

/// A rough size of a book as `output`: the XTC and XTCH figures are exact for a
/// 480 x 800 screen; a page of an EPUB depends on the drawing, and about 24 KB
/// is what manga comes to.
pub fn estimate_output_bytes(pages: usize, output: library::OutputFormat) -> u64 {
    match output {
        library::OutputFormat::Xtc => estimate_export_bytes(pages, converter::xtc::Format::Xtc),
        library::OutputFormat::Xtch => estimate_export_bytes(pages, converter::xtc::Format::Xtch),
        library::OutputFormat::Epub => pages as u64 * 24_000,
        library::OutputFormat::Cbz => pages as u64 * 23_000,
    }
}

// ---- Wording ---------------------------------------------------------------

pub fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "Working",
        Verdict::NeedsBrowser => "Needs browser support",
        Verdict::NeedsFlareSolverr => "Blocked by Cloudflare",
        Verdict::Failed => "Not working",
    }
}

/// The biggest a saved cover is kept: posters are shown at most 200 px wide, so
/// this is sharp on a high-density screen, fills an e-reader screen when it is
/// the cover page of an export, and still takes only tens of kilobytes.
const COVER_MAX: (u32, u32) = (540, 810);
/// Covers larger than this are shrunk when they are next read.
pub const COVER_KEEP_BYTES: usize = 160 * 1024;

/// A cover as a small JPEG, to keep on disk instead of what the source sent.
/// `None` if the bytes are not an image.
pub fn small_cover(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > COVER_MAX.0 || img.height() > COVER_MAX.1 {
        img.resize(
            COVER_MAX.0,
            COVER_MAX.1,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&img.to_rgb8())
        .ok()?;
    Some(out)
}

/// A length of time in words: "45 s", "2 min 14 s", "1 h 05 min".
pub fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min {:02} s", secs / 60, secs % 60),
        _ => format!("{} h {:02} min", secs / 3600, secs % 3600 / 60),
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} bytes")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// "just now", "5 minutes ago", "3 days ago".
pub fn ago(now_secs: u64, then_secs: u64) -> String {
    let s = now_secs.saturating_sub(then_secs);
    let (n, unit) = match s {
        0..=59 => return "just now".into(),
        60..=3599 => (s / 60, "minute"),
        3600..=86_399 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// Turn what a source says about a series into what the Library keeps.
pub fn series_info_from(
    details: &catalog::MangaDetails,
    source_names: Vec<(String, String)>,
    has_cover: bool,
    now: u64,
) -> library::SeriesInfo {
    let clean = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or("").to_string();
    let mut genres: Vec<String> = Vec::new();
    for g in &details.genre {
        let g = g.trim();
        if !g.is_empty() && !genres.iter().any(|x| x.eq_ignore_ascii_case(g)) {
            genres.push(g.to_string());
        }
    }
    // Descriptions often arrive with Windows line breaks and long runs of blank lines.
    let mut description = clean(&details.description).replace("\r\n", "\n");
    while description.contains("\n\n\n") {
        description = description.replace("\n\n\n", "\n\n");
    }
    library::SeriesInfo {
        description,
        author: clean(&details.author),
        artist: clean(&details.artist),
        status: library::status_label(details.status.as_deref().unwrap_or("")),
        genres,
        source_names,
        has_cover,
        saved_at: now,
    }
}

/// How many chapters make a series slow to load: the sources send the whole
/// list at once, so a long one takes a while.
pub const LONG_SERIES: usize = 500;

/// What to say while the chapters of a long series load, when how many it has
/// is known; nothing for a short series or when the count is not known.
pub fn long_series_note(chapters: Option<usize>) -> Option<String> {
    let n = chapters.filter(|n| *n > LONG_SERIES)?;
    // "about 3,000": rounded, with a comma in the thousands.
    let step = if n >= 1000 { 100 } else { 50 };
    let rounded = (n + step / 2) / step * step;
    let text = if rounded >= 1000 {
        format!("{},{:03}", rounded / 1000, rounded % 1000)
    } else {
        rounded.to_string()
    };
    Some(format!(
        "This series has about {text} chapters, so it can take a while to load them from your sources."
    ))
}

/// "1 chapter", "2 chapters".
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// How search results are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSort {
    /// How well the title matches, then how good the sources are.
    #[default]
    BestMatch,
    MostChapters,
    RecentlyUpdated,
    MostSources,
    TitleAz,
}

impl SearchSort {
    pub const ALL: [SearchSort; 5] = [
        SearchSort::BestMatch,
        SearchSort::MostChapters,
        SearchSort::RecentlyUpdated,
        SearchSort::MostSources,
        SearchSort::TitleAz,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "Best match",
            SearchSort::MostChapters => "Most chapters",
            SearchSort::RecentlyUpdated => "Recently updated",
            SearchSort::MostSources => "Most sources",
            SearchSort::TitleAz => "Title A to Z",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "match",
            SearchSort::MostChapters => "chapters",
            SearchSort::RecentlyUpdated => "recent",
            SearchSort::MostSources => "sources",
            SearchSort::TitleAz => "az",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The order to show results in, as positions in `ranked`. Ties keep the
/// original (best match) order, and results that lack the thing being sorted
/// on come last.
pub fn sorted_order(ranked: &[search::RankedSeries], sort: SearchSort) -> Vec<usize> {
    let chapters = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.chapter_count))
            .max()
    };
    let updated = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.latest_upload_ms))
            .max()
    };
    let mut order: Vec<usize> = (0..ranked.len()).collect();
    match sort {
        SearchSort::BestMatch => {}
        SearchSort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(chapters(&ranked[i]).map(i64::from)))
        }
        SearchSort::RecentlyUpdated => {
            order.sort_by_key(|&i| std::cmp::Reverse(updated(&ranked[i])))
        }
        SearchSort::MostSources => {
            order.sort_by_key(|&i| std::cmp::Reverse(ranked[i].members.len()))
        }
        SearchSort::TitleAz => {
            order.sort_by_key(|&i| ranked[i].title.to_lowercase());
        }
    }
    order
}

// ---- Narrowing the search results -----------------------------------------

/// The kinds of series that can be picked, as sources tag them.
pub const SERIES_TYPES: [&str; 5] = ["Manga", "Manhwa", "Manhua", "Webtoon", "Comic"];

/// Genres offered in the filter.
pub const COMMON_GENRES: [&str; 24] = [
    "Action", "Adventure", "Comedy", "Drama", "Fantasy", "Horror", "Isekai", "Martial Arts",
    "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural",
    "Thriller", "School Life", "Historical", "Mecha", "Seinen", "Shounen", "Shoujo", "Josei",
    "Tragedy",
];

/// Tags that mean a series is for adults.
const EXPLICIT_TAGS: [&str; 7] = [
    "adult", "hentai", "smut", "erotica", "mature", "pornographic", "explicit",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExplicitFilter {
    #[default]
    Show,
    Hide,
    Only,
}

/// What the search results are narrowed to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchFilters {
    pub types: Vec<String>,
    pub explicit: ExplicitFilter,
    pub genres: Vec<String>,
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Types and genres can only be told from the details of each series.
    pub fn needs_tags(&self) -> bool {
        !self.types.is_empty() || !self.genres.is_empty()
    }

    /// What to show on the folded pills: each choice made, or "All types" and
    /// "All genres" while there is none.
    pub fn pills(&self) -> Vec<String> {
        let mut out: Vec<String> = if self.types.is_empty() {
            vec!["All types".into()]
        } else {
            self.types.clone()
        };
        match self.explicit {
            ExplicitFilter::Show => {}
            ExplicitFilter::Hide => out.push("No explicit".into()),
            ExplicitFilter::Only => out.push("Explicit only".into()),
        }
        if self.genres.is_empty() {
            out.push("All genres".into());
        } else {
            out.extend(self.genres.iter().cloned());
        }
        out
    }

    /// Short words for each choice made, for the pills.
    pub fn labels(&self) -> Vec<String> {
        let mut out: Vec<String> = self.types.clone();
        match self.explicit {
            ExplicitFilter::Show => {}
            ExplicitFilter::Hide => out.push("No explicit".into()),
            ExplicitFilter::Only => out.push("Explicit only".into()),
        }
        out.extend(self.genres.iter().cloned());
        out
    }

    /// Does a series pass? `tags` are the genres its source lists (`None`
    /// until they are known); `adult_source` says its source is for adults.
    pub fn allows(&self, tags: Option<&[String]>, adult_source: bool) -> bool {
        let has = |tags: &[String], word: &str| tags.iter().any(|t| t.trim().eq_ignore_ascii_case(word));
        let explicit = adult_source
            || tags.is_some_and(|t| EXPLICIT_TAGS.iter().any(|w| has(t, w)));
        match self.explicit {
            ExplicitFilter::Hide if explicit => return false,
            ExplicitFilter::Only if !explicit => return false,
            _ => {}
        }
        if !self.needs_tags() {
            return true;
        }
        let Some(tags) = tags else {
            return false;
        };
        (self.types.is_empty() || self.types.iter().any(|t| has(tags, t)))
            && self.genres.iter().all(|g| has(tags, g))
    }
}

/// Which series the Library shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryView {
    #[default]
    All,
    /// Series with chapters on this computer.
    Downloaded,
    /// Series that have been exported for a reader.
    Exported,
    /// Series put in the library but not downloaded yet.
    Saved,
}

impl LibraryView {
    pub const ALL: [LibraryView; 4] = [
        LibraryView::All,
        LibraryView::Downloaded,
        LibraryView::Exported,
        LibraryView::Saved,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryView::All => "All",
            LibraryView::Downloaded => "Downloaded",
            LibraryView::Exported => "Exported",
            LibraryView::Saved => "Saved for later",
        }
    }

    pub fn includes(self, series: &library::DownloadedSeries, exported: bool) -> bool {
        match self {
            LibraryView::All => true,
            LibraryView::Downloaded => !series.chapters.is_empty(),
            LibraryView::Exported => exported,
            LibraryView::Saved => series.chapters.is_empty(),
        }
    }
}

/// How the Library orders its series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibrarySort {
    /// The ones touched most recently first.
    #[default]
    Recent,
    Title,
    MostChapters,
    Size,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 4] = [
        LibrarySort::Recent,
        LibrarySort::Title,
        LibrarySort::MostChapters,
        LibrarySort::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::Recent => "Recently added",
            LibrarySort::Title => "Title A to Z",
            LibrarySort::MostChapters => "Most chapters",
            LibrarySort::Size => "Biggest on disk",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LibrarySort::Recent => "recent",
            LibrarySort::Title => "title",
            LibrarySort::MostChapters => "chapters",
            LibrarySort::Size => "size",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The library's series to show, as positions in `series`: those in the view
/// that match the filter text (in the title, the author or a genre), in order.
pub fn library_order(
    series: &[library::DownloadedSeries],
    view: LibraryView,
    sort: LibrarySort,
    filter: &str,
    exported: &std::collections::HashSet<String>,
) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    let matches = |s: &library::DownloadedSeries| {
        needle.is_empty()
            || s.title.to_lowercase().contains(&needle)
            || s.info.as_ref().is_some_and(|i| {
                i.author.to_lowercase().contains(&needle)
                    || i.genres.iter().any(|g| g.to_lowercase().contains(&needle))
            })
    };
    let mut order: Vec<usize> = (0..series.len())
        .filter(|&i| view.includes(&series[i], exported.contains(&series[i].id)) && matches(&series[i]))
        .collect();
    match sort {
        LibrarySort::Recent => {
            order.sort_by_key(|&i| std::cmp::Reverse(series[i].updated_at.max(series[i].added_at)))
        }
        LibrarySort::Title => order.sort_by_key(|&i| series[i].title.to_lowercase()),
        LibrarySort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(series[i].chapters.len()))
        }
        LibrarySort::Size => order.sort_by_key(|&i| std::cmp::Reverse(series[i].bytes())),
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::chapter;
    use search::merge_chapters;

    fn nums(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|&n| Some(n)).collect()
    }

    fn merged_list() -> Vec<MergedChapter> {
        let mut a = chapter(1, 1.0, 1_000_000_000_000);
        a.name = "Chapter 1: Start".into();
        let mut b = chapter(2, 2.0, 1_500_000_000_000);
        b.name = "Chapter 2: Middle".into();
        let mut c = chapter(3, 10.0, 0);
        c.name = "Chapter 10: Finale".into();
        let mut extra = chapter(4, -1.0, 1_700_000_000_000);
        extra.name = "Author's note".into();
        // A second source also has chapter 2, newer.
        let other = vec![chapter(12, 2.0, 1_600_000_000_000)];
        merge_chapters(&[vec![a, b, c, extra], other])
    }

    fn names(merged: &[MergedChapter], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| merged[i].name.clone()).collect()
    }

    #[test]
    fn the_table_sorts_by_each_column_in_both_directions() {
        let m = merged_list();
        let by = |key, desc| names(&m, &visible_chapters(&m, "", key, desc));

        // Number: numbered first; the unnumbered note is last in either direction.
        assert_eq!(
            by(SortKey::Number, false),
            [
                "Chapter 1: Start",
                "Chapter 2: Middle",
                "Chapter 10: Finale",
                "Author's note"
            ]
        );
        assert_eq!(
            by(SortKey::Number, true),
            [
                "Chapter 10: Finale",
                "Chapter 2: Middle",
                "Chapter 1: Start",
                "Author's note"
            ]
        );
        // Name: A to Z, case-insensitive.
        assert_eq!(by(SortKey::Name, false)[0], "Author's note");
        assert_eq!(by(SortKey::Name, true)[0], "Chapter 2: Middle");
        // Sources: chapter 2 has two sources.
        assert_eq!(by(SortKey::Sources, true)[0], "Chapter 2: Middle");
        // Date: newest first; chapter 10 has no date, so it is last either way.
        assert_eq!(by(SortKey::Date, true)[0], "Author's note");
        assert_eq!(
            *by(SortKey::Date, true).last().unwrap(),
            "Chapter 10: Finale"
        );
        assert_eq!(
            *by(SortKey::Date, false).last().unwrap(),
            "Chapter 10: Finale"
        );
    }

    #[test]
    fn the_filter_matches_every_word_in_names_and_numbers() {
        let m = merged_list();
        let hit = |f: &str| names(&m, &visible_chapters(&m, f, SortKey::Number, false));
        assert_eq!(hit("finale"), ["Chapter 10: Finale"]);
        assert_eq!(
            hit("CHAPTER middle"),
            ["Chapter 2: Middle"],
            "all words, any case"
        );
        assert_eq!(hit("10"), ["Chapter 10: Finale"], "by number");
        assert_eq!(hit("note"), ["Author's note"]);
        assert!(hit("nothing like this").is_empty());
        assert_eq!(hit("   ").len(), 4, "a blank filter shows everything");
    }

    #[test]
    fn a_chapter_with_several_sources_shows_its_newest_upload() {
        let m = merged_list();
        let two = m.iter().find(|c| c.name == "Chapter 2: Middle").unwrap();
        assert_eq!(latest_upload(two), 1_600_000_000_000);
        let ten = m.iter().find(|c| c.name == "Chapter 10: Finale").unwrap();
        assert_eq!(latest_upload(ten), 0);
    }

    #[test]
    fn clicking_a_header_picks_a_sensible_direction() {
        // New column: numbers and dates start newest/biggest first, names A to Z.
        assert_eq!(
            next_sort((SortKey::Name, false), SortKey::Number),
            (SortKey::Number, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Date),
            (SortKey::Date, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Name),
            (SortKey::Name, false)
        );
        // Same column: flip.
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Number),
            (SortKey::Number, false)
        );
        assert_eq!(
            next_sort((SortKey::Number, false), SortKey::Number),
            (SortKey::Number, true)
        );
    }

    #[test]
    fn numbers_and_dates_are_shown_plainly() {
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(0.01), "0.01");
        assert_eq!(format_number(3.10), "3.1");
        assert_eq!(format_date(0), "");
        assert_eq!(format_date(-5), "");
        assert_eq!(format_date(1_700_000_000_000), "2023-11-14");
        assert_eq!(format_date(951_782_400_000), "2000-02-29", "leap day");
        assert_eq!(format_date(1_000_000_000_000), "2001-09-09");
    }

    fn member(source: &str, manga_id: i64, score: f32) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: manga_id,
                    title: "Solo Leveling".into(),
                    thumbnail_url: None,
                },
            ),
            score,
            enrichment: None,
        }
    }

    #[test]
    fn a_source_listing_a_series_twice_appears_once_with_its_best_entry() {
        // MangaFire lists the series twice; the better-scored entry comes first.
        let members = vec![
            member("fire", 1, 0.9),
            member("dex", 2, 0.8),
            member("fire", 3, 0.7),
            member("pill", 4, 0.6),
        ];
        let picked = one_per_source(&members, 10);
        let ids: Vec<_> = picked
            .iter()
            .map(|m| (m.candidate.source_id.as_str(), m.candidate.manga_id))
            .collect();
        assert_eq!(ids, [("fire", 1), ("dex", 2), ("pill", 4)]);
        // The limit applies after de-duplication.
        assert_eq!(one_per_source(&members, 2).len(), 2);
        assert!(one_per_source(&[], 4).is_empty());
    }

    #[test]
    fn chapter_text_selects_numbers_ranges_and_open_ends() {
        let n = nums(&[1.0, 2.0, 3.0, 4.0, 5.0, 10.0, 11.0, 12.0]);
        assert_eq!(select_by_text("2", &n).unwrap(), [1]);
        assert_eq!(select_by_text("2-4", &n).unwrap(), [1, 2, 3]);
        assert_eq!(select_by_text("1, 5, 11", &n).unwrap(), [0, 4, 6]);
        assert_eq!(select_by_text("10-", &n).unwrap(), [5, 6, 7]);
        assert_eq!(
            select_by_text("1-2, 2-3", &n).unwrap(),
            [0, 1, 2],
            "overlaps are not doubled"
        );
        assert_eq!(select_by_text("6-9", &n).unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn chapter_text_handles_halves_and_ignores_unnumbered() {
        let n = vec![Some(1.0), Some(1.5), Some(2.0), None];
        assert_eq!(select_by_text("1-2", &n).unwrap(), [0, 1, 2]);
        assert_eq!(select_by_text("1.5", &n).unwrap(), [1]);
        assert_eq!(
            select_by_text("0-", &n).unwrap(),
            [0, 1, 2],
            "unnumbered chapters are never matched"
        );
    }

    #[test]
    fn bad_chapter_text_gets_a_helpful_message() {
        let n = nums(&[1.0]);
        assert!(select_by_text("abc", &n).unwrap_err().contains("'abc'"));
        assert!(select_by_text("5-2", &n).unwrap_err().contains("backwards"));
        assert!(select_by_text("  ,  ", &n).is_err());
        assert!(select_by_text("", &n).is_err());
    }

    #[test]
    fn export_targets_follow_the_choice_and_settings() {
        let mut s = Settings::default();
        let default_dir = Path::new("/exports");
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/exports".into()
            }
        );
        s.export.output_dir = Some("/mine".into());
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/mine".into()
            }
        );
        assert!(export_target(&TargetChoice::Usb, &s, default_dir).is_err());
        s.update_reader_setup(|r| r.reader_dir = Some("E:\\".into()));
        assert_eq!(
            export_target(&TargetChoice::Usb, &s, default_dir).unwrap(),
            ExportTarget::Device {
                mount: "E:\\".into(),
                folder: "Manga".into()
            }
        );
        assert!(export_target(&TargetChoice::WiFi, &s, default_dir).is_err());
        s.update_reader_setup(|r| r.wifi_url = Some(" http://192.168.3.3 ".into()));
        assert_eq!(
            export_target(&TargetChoice::WiFi, &s, default_dir).unwrap(),
            ExportTarget::WebDav {
                base_url: "http://192.168.3.3".into(),
                folder: "Manga".into()
            }
        );
    }

    fn started(index: usize, total: usize) -> JobEvent {
        JobEvent::ChapterStarted {
            index,
            total,
            name: format!("Ch {}", index + 1),
        }
    }

    fn pages(chapter_index: usize, done: usize, total: usize) -> JobEvent {
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        }
    }

    #[test]
    fn progress_combines_chapter_and_page_position() {
        let mut p = JobProgress::default();
        assert_eq!(p.fraction(), 0.0);
        assert_eq!(p.label(), "Starting…");
        p.apply(&started(1, 4));
        assert_eq!(p.fraction(), 0.0);
        p.apply(&pages(1, 5, 10));
        assert!((p.fraction() - 0.125).abs() < 1e-6);
        assert_eq!(p.label(), "Chapter 2 of 4, page 5 of 10");
        p.apply(&JobEvent::UsedFallback {
            chapter_index: 1,
            from: "a".into(),
            to: "b".into(),
            reason: String::new(),
        });
        assert!(p.note.as_deref().unwrap().contains('b'));
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.fraction(), 0.25);
        p.apply(&started(2, 4));
        assert_eq!(p.note, None, "the note clears with the next chapter");
        assert_eq!(p.label(), "Chapter 3 of 4");
    }

    #[test]
    fn chapters_downloading_together_share_the_bar() {
        let mut p = JobProgress::default();
        for i in 0..3 {
            p.apply(&started(i, 6));
        }
        p.apply(&pages(0, 5, 10));
        p.apply(&pages(1, 10, 10));
        p.apply(&pages(2, 0, 10));
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(p.label(), "0 of 6 chapters done, 3 in progress");
        assert!((p.fraction() - 1.5 / 6.0).abs() < 1e-6);

        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 2]);
        assert_eq!(p.label(), "1 of 6 chapters done, 2 in progress");
        assert!(
            (p.fraction() - 1.5 / 6.0).abs() < 1e-6,
            "one chapter's 100% moved from partial to finished"
        );
        // A second "done" for the same chapter is not counted twice.
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.finished, 1);
    }

    #[test]
    fn easing_off_is_explained_and_clears_when_the_source_recovers() {
        let mut p = JobProgress::default();
        p.apply(&started(0, 1));
        p.apply(&JobEvent::PagesAtOnce { now: 3, chosen: 12 });
        assert!(p.note.as_deref().unwrap().contains('3'));
        p.apply(&JobEvent::PagesAtOnce {
            now: 12,
            chosen: 12,
        });
        assert_eq!(p.note, None);
    }

    #[test]
    fn progress_never_leaves_zero_to_one() {
        let p = JobProgress {
            chapter_total: 2,
            finished: 9,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn export_size_estimates_follow_the_format() {
        use converter::xtc::Format;
        assert_eq!(estimate_export_bytes(0, Format::Xtch), 0);
        assert_eq!(estimate_export_bytes(100, Format::Xtch), 9_602_200);
        assert_eq!(estimate_export_bytes(100, Format::Xtc), 4_802_200);
        // Four grays take about twice the room of black and white.
        assert!(
            estimate_export_bytes(10, Format::Xtch)
                > 2 * estimate_export_bytes(10, Format::Xtc) - 1000
        );
    }

    #[test]
    fn wording_helpers() {
        assert_eq!(
            verdict_label(Verdict::NeedsBrowser),
            "Needs browser support"
        );
        assert_eq!(human_bytes(999), "999 bytes");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(268_200_000), "268 MB");
        assert_eq!(human_bytes(1_200_000_000), "1.2 GB");
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1000, 940), "1 minute ago");
        assert_eq!(ago(10_000, 2_800), "2 hours ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400 * 3), "3 days ago");
        assert_eq!(
            ago(5, 50),
            "just now",
            "a clock set back is not a negative time"
        );
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[test]
    fn a_stopping_job_says_so_whatever_it_was_doing() {
        let mut p = JobProgress::default();
        p.apply(&JobEvent::ChapterStarted {
            index: 0,
            total: 3,
            name: "Ch 1".into(),
        });
        assert_eq!(p.label(), "Chapter 1 of 3");
        p.stopping = true;
        assert_eq!(p.label(), "Stopping…");
    }
}

#[cfg(test)]
mod info_tests {
    use super::*;

    fn details() -> catalog::MangaDetails {
        catalog::MangaDetails {
            id: 1,
            title: "Series".into(),
            description: Some("  First.\r\nSecond.\n\n\n\nThird.  ".into()),
            author: Some(" A. Writer ".into()),
            artist: None,
            genre: vec![
                "Action".into(),
                " action ".into(),
                "".into(),
                "Fantasy".into(),
            ],
            status: Some("ONGOING".into()),
            thumbnail_url: Some("/cover".into()),
        }
    }

    #[test]
    fn series_details_are_tidied_for_display() {
        let info = series_info_from(&details(), vec![("1".into(), "Src".into())], true, 99);
        assert_eq!(info.description, "First.\nSecond.\n\nThird.");
        assert_eq!(info.author, "A. Writer");
        assert_eq!(info.artist, "");
        assert_eq!(info.status, "Ongoing");
        assert_eq!(info.genres, ["Action", "Fantasy"], "no blanks or repeats");
        assert!(info.has_cover);
        assert_eq!(info.saved_at, 99);
        assert_eq!(info.source_name("1"), Some("Src"));
    }

    #[test]
    fn missing_details_become_empty_not_errors() {
        let info = series_info_from(&catalog::MangaDetails::default(), Vec::new(), false, 0);
        assert_eq!(info.description, "");
        assert_eq!(info.status, "");
        assert!(info.genres.is_empty());
        assert!(!info.has_cover);
    }

    #[test]
    fn counts_are_worded_in_the_singular_and_plural() {
        assert_eq!(plural(0, "chapter"), "0 chapters");
        assert_eq!(plural(1, "chapter"), "1 chapter");
        assert_eq!(plural(235, "page"), "235 pages");
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use search::{Enrichment, RankedSeries, ScoredMember};

    fn member(source: &str, chapters: Option<u32>, updated: Option<i64>) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: 1,
                    title: "x".into(),
                    thumbnail_url: None,
                },
            ),
            score: 0.5,
            enrichment: Some(Enrichment {
                chapter_count: chapters,
                latest_upload_ms: updated,
            }),
        }
    }

    fn series(title: &str, members: Vec<ScoredMember>) -> RankedSeries {
        RankedSeries {
            title: title.into(),
            members,
            relevance: 1.0,
        }
    }

    fn results() -> Vec<RankedSeries> {
        vec![
            series("Bravo", vec![member("a", Some(10), Some(300))]),
            series(
                "alpha",
                vec![
                    member("a", Some(50), Some(100)),
                    member("b", Some(80), None),
                ],
            ),
            series("Charlie", vec![member("a", None, None)]),
            series("Delta", vec![member("a", Some(50), Some(900))]),
        ]
    }

    #[test]
    fn best_match_keeps_the_order_the_search_gave() {
        assert_eq!(
            sorted_order(&results(), SearchSort::BestMatch),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn most_chapters_uses_the_biggest_count_any_source_has_and_puts_unknown_last() {
        // alpha has 80 (from its second source), Delta 50, Bravo 10, Charlie unknown.
        assert_eq!(
            sorted_order(&results(), SearchSort::MostChapters),
            [1, 3, 0, 2]
        );
    }

    #[test]
    fn recently_updated_puts_the_newest_first_and_unknown_last() {
        assert_eq!(
            sorted_order(&results(), SearchSort::RecentlyUpdated),
            [3, 0, 1, 2]
        );
    }

    #[test]
    fn most_sources_breaks_ties_by_the_original_order() {
        assert_eq!(
            sorted_order(&results(), SearchSort::MostSources),
            [1, 0, 2, 3]
        );
    }

    #[test]
    fn titles_sort_without_regard_to_case() {
        assert_eq!(sorted_order(&results(), SearchSort::TitleAz), [1, 0, 2, 3]);
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in SearchSort::ALL {
            assert_eq!(SearchSort::from_key(sort.key()), sort);
        }
        assert_eq!(SearchSort::from_key("nonsense"), SearchSort::BestMatch);
        assert_eq!(
            sorted_order(&[], SearchSort::MostChapters),
            Vec::<usize>::new()
        );
    }
}

#[cfg(test)]
mod library_view_tests {
    #[test]
    fn a_long_series_gets_a_note_and_a_short_one_does_not() {
        assert_eq!(long_series_note(None), None);
        assert_eq!(long_series_note(Some(500)), None);
        assert!(long_series_note(Some(501)).unwrap().contains("about 500 chapters"));
        assert!(long_series_note(Some(3_187)).unwrap().contains("about 3,200 chapters"));
        assert!(long_series_note(Some(12_000)).unwrap().contains("about 12,000 chapters"));
    }

    use super::*;
    use library::{DownloadedChapter, DownloadedSeries, SeriesInfo};

    fn chapter(bytes: u64) -> DownloadedChapter {
        DownloadedChapter {
            key: "c1".into(),
            number: Some(1.0),
            name: "Chapter 1".into(),
            source_id: "s".into(),
            chapter_id: 1,
            pages: 10,
            bytes,
            downloaded_at: 0,
        }
    }

    fn series(
        title: &str,
        chapters: usize,
        bytes: u64,
        added: u64,
        updated: u64,
    ) -> DownloadedSeries {
        DownloadedSeries {
            id: title.to_lowercase(),
            title: title.into(),
            chapters: (0..chapters).map(|_| chapter(bytes)).collect(),
            added_at: added,
            updated_at: updated,
            ..Default::default()
        }
    }

    fn shelf() -> Vec<DownloadedSeries> {
        let mut a = series("Berserk", 3, 100, 10, 50);
        a.info = Some(SeriesInfo {
            author: "Kentaro Miura".into(),
            genres: vec!["Dark Fantasy".into()],
            ..Default::default()
        });
        vec![
            a,
            series("alpha", 0, 0, 40, 40), // saved, not downloaded
            series("Zeta", 10, 5, 30, 30),
            series("Delta", 0, 0, 5, 5),
        ]
    }

    #[test]
    fn the_search_filters_narrow_by_type_genre_and_explicit() {
        let tags = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let none = SearchFilters::default();
        assert!(none.is_empty() && none.allows(None, false) && none.allows(None, true));

        let manhwa_action = SearchFilters {
            types: vec!["Manhwa".into()],
            genres: vec!["Action".into()],
            ..Default::default()
        };
        assert!(manhwa_action.allows(Some(&tags(&["manhwa", "ACTION", "Fantasy"])), false));
        assert!(!manhwa_action.allows(Some(&tags(&["Manhwa"])), false), "every genre is needed");
        assert!(!manhwa_action.allows(Some(&tags(&["Manga", "Action"])), false), "wrong type");
        assert!(!manhwa_action.allows(None, false), "not known yet");

        let hide = SearchFilters { explicit: ExplicitFilter::Hide, ..Default::default() };
        assert!(hide.allows(None, false));
        assert!(!hide.allows(None, true), "an adult source");
        assert!(!hide.allows(Some(&tags(&["Hentai"])), false), "an adult tag");
        let only = SearchFilters { explicit: ExplicitFilter::Only, ..Default::default() };
        assert!(only.allows(None, true) && !only.allows(Some(&tags(&["Action"])), false));
        assert_eq!(manhwa_action.labels(), ["Manhwa", "Action"]);
        assert_eq!(hide.labels(), ["No explicit"]);
        assert_eq!(none.pills(), ["All types", "All genres"]);
        assert_eq!(hide.pills(), ["All types", "No explicit", "All genres"]);
        assert_eq!(manhwa_action.pills(), ["Manhwa", "Action"]);
    }

    #[test]
    fn the_views_split_downloaded_from_saved() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let order = |v| library_order(&s, v, LibrarySort::Title, "", &none);
        assert_eq!(order(LibraryView::All), [1, 0, 3, 2]);
        assert!(order(LibraryView::Exported).is_empty());
        let berserk = std::collections::HashSet::from(["berserk".to_string()]);
        assert_eq!(
            library_order(&s, LibraryView::Exported, LibrarySort::Title, "", &berserk),
            [0]
        );
        assert_eq!(order(LibraryView::Downloaded), [0, 2]);
        assert_eq!(order(LibraryView::Saved), [1, 3]);
    }

    #[test]
    fn the_sorts_order_as_they_say() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let by = |sort| library_order(&s, LibraryView::All, sort, "", &none);
        assert_eq!(
            by(LibrarySort::Recent),
            [0, 1, 2, 3],
            "latest touch first (50, 40, 30, 5)"
        );
        assert_eq!(
            by(LibrarySort::Title),
            [1, 0, 3, 2],
            "A to Z, ignoring case"
        );
        assert_eq!(by(LibrarySort::MostChapters), [2, 0, 1, 3]);
        assert_eq!(
            by(LibrarySort::Size),
            [0, 2, 1, 3],
            "300 bytes, 50, then the empty ones"
        );
    }

    #[test]
    fn the_filter_looks_at_the_title_author_and_genres() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let find = |text: &str| library_order(&s, LibraryView::All, LibrarySort::Title, text, &none);
        assert_eq!(find("  ZET "), [2]);
        assert_eq!(find("miura"), [0], "author");
        assert_eq!(find("dark fantasy"), [0], "genre");
        assert!(find("nothing like this").is_empty());
        assert_eq!(
            library_order(&s, LibraryView::Saved, LibrarySort::Title, "berserk", &none),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in LibrarySort::ALL {
            assert_eq!(LibrarySort::from_key(sort.key()), sort);
        }
        assert_eq!(LibrarySort::from_key("???"), LibrarySort::Recent);
    }

    #[test]
    fn a_cover_is_kept_small() {
        let mut big = image::RgbImage::new(1200, 1800);
        for (x, y, p) in big.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8]);
        }
        let mut png = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let small = small_cover(&png).expect("an image shrinks");
        let decoded = image::load_from_memory(&small).unwrap();
        assert!(decoded.width() <= 540 && decoded.height() <= 810);
        assert_eq!(decoded.width() * 3, decoded.height() * 2, "shape kept");
        assert!(
            small.len() < png.len() / 5,
            "{} vs {}",
            small.len(),
            png.len()
        );
        assert!(small_cover(b"not an image").is_none());
    }

    #[test]
    fn durations_read_in_words() {
        assert_eq!(human_duration(7), "7 s");
        assert_eq!(human_duration(134), "2 min 14 s");
        assert_eq!(human_duration(3900), "1 h 05 min");
    }

    #[test]
    fn no_time_left_is_guessed_before_there_is_a_pace() {
        let mut p = JobProgress::default();
        assert_eq!(p.eta_secs(), None);
        p.chapter_total = 10;
        p.finished = 5;
        assert_eq!(p.eta_secs(), None, "nothing has started yet");
    }
}

#[cfg(test)]
mod write_progress_tests {
    use super::*;

    #[test]
    fn the_bar_follows_the_bytes_stored_once_files_are_being_sent() {
        let mut p = JobProgress {
            chapter_total: 4,
            finished: 4,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0, "all chapters converted");
        p.apply(&JobEvent::Writing { file: "a.xtc".into() });
        p.apply(&JobEvent::Sent { done: 0, total: 1000 });
        assert_eq!(p.fraction(), 0.0, "nothing written yet");
        p.apply(&JobEvent::Sent { done: 250, total: 1000 });
        assert_eq!(p.fraction(), 0.25);
        assert!(p.pages_per_second().is_none() && p.eta_secs().is_none());
        p.apply(&JobEvent::Flushing);
        assert!(p.writing.as_ref().unwrap().flushing);
    }

    #[test]
    fn speed_needs_a_little_time_and_progress() {
        let mut w = WriteProgress::default();
        assert!(w.bytes_per_second().is_none());
        w.record(0, 100);
        assert!(w.bytes_per_second().is_none(), "one sample says nothing");
        assert_eq!(w.eta_secs(), None);
    }
}
