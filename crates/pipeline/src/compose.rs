//! Stage 3: build reader files from the downloaded chapters.
//!
//! Works entirely from the library on disk, so it needs no network and can
//! be repeated with different settings or groupings at will. Each book is
//! assembled in a temporary file rather than in memory, so one file holding
//! a whole series is fine.

use crate::conversion_cache;
use crate::cbz::{CbzBuilder, CbzMeta};
use crate::epub::{self, EpubBuilder, EpubMeta};
use crate::error::{PipelineError, Result};
use crate::export::{output_file_name, sanitize_component, ExportTarget, ExportedFile};
use crate::options::process_options_for;
use crate::preview::decode_page;
use crate::types::{CancelFlag, JobEvent};
use converter::xtc::{Chapter as XtcChapter, Format, Metadata, WriteError, XtcBuilder};
use converter::{GrayImage, ProcessOptions};
use device_profiles::DeviceProfile;
use futures_util::stream::{self, StreamExt};
use library::{DownloadedChapter, DownloadedSeries, LibraryFiles};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc::UnboundedSender, OwnedSemaphorePermit, Semaphore};

/// Pages decoded at the same time within one chapter: one for each core, but
/// not so many that tall pages fill the memory.
fn page_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 12)
}
/// Chapters converted ahead of the one being written.
fn chapter_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |n| n.get() / 4)
        .clamp(2, 6)
}
/// Just under the format's limit of 65,535 pages per file.
const DEFAULT_MAX_PAGES_PER_FILE: usize = 60_000;

/// How chapters are divided into files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    /// Everything in one file (the default).
    Single,
    /// One file per chapter. Can mean a lot of files.
    PerChapter,
    /// One file for every N chapters.
    EveryN(usize),
}

#[derive(Clone, Debug)]
pub struct ExportJob {
    pub series: DownloadedSeries,
    /// Which downloaded chapters to include (any order; the series' own
    /// reading order is used).
    pub chapter_keys: Vec<String>,
    pub device: DeviceProfile,
    /// Language tag stored in the file, e.g. "en".
    pub language: String,
    pub grouping: Grouping,
    pub target: ExportTarget,
    /// Scratch space for books being assembled.
    pub work_dir: PathBuf,
    /// Start a new file rather than exceed this many pages.
    pub max_pages_per_file: usize,
    /// Rebuild chapters even when cached pages exist, for diagnostics and
    /// first-export benchmarks. Newly converted pages still refresh caches.
    pub force_conversion: bool,
    /// Open every file with the series cover (or, without one, its first page).
    pub cover: bool,
    /// Make the files from the pages as they were saved, with no e-ink
    /// processing at all (see [`crate::original`]).
    pub original: Option<crate::original::OriginalExport>,
}

impl ExportJob {
    pub fn new(
        series: DownloadedSeries,
        chapter_keys: Vec<String>,
        device: DeviceProfile,
        target: ExportTarget,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            series,
            chapter_keys,
            device,
            language: "en".into(),
            grouping: Grouping::Single,
            target,
            work_dir,
            max_pages_per_file: DEFAULT_MAX_PAGES_PER_FILE,
            force_conversion: false,
            cover: true,
            original: None,
        }
    }
}

#[derive(Debug)]
pub struct ExportOutcome {
    pub files: Vec<ExportedFile>,
    pub pages: usize,
    pub chapters: usize,
    /// Chapters reused without decoding, rendering or packing their pages.
    pub cached_chapters: usize,
}

/// A book being built: an XTC or XTCH, an EPUB, or a CBZ.
enum Book {
    Xtc(XtcBuilder),
    Epub(Box<EpubBuilder>),
    Cbz(Box<CbzBuilder>),
}

impl Book {
    fn page_count(&self) -> usize {
        match self {
            Book::Xtc(b) => b.page_count(),
            Book::Epub(b) => b.page_count(),
            Book::Cbz(b) => b.page_count(),
        }
    }

    /// Add a page made by this kind of book's own encoder.
    fn add(&mut self, page: &[u8]) -> Result<()> {
        match self {
            Book::Xtc(b) => Ok(b.add_encoded_page(page)?),
            Book::Epub(b) => b.add_page(page),
            Book::Cbz(b) => b.add_page(page),
        }
    }

    /// A chapter that starts at page `start` and runs to the last page so far.
    fn chapter(&mut self, name: &str, start: usize, number: Option<f64>) {
        match self {
            Book::Xtc(b) => {
                let end = b.page_count().saturating_sub(1);
                b.push_chapter(XtcChapter {
                    name: name.to_string(),
                    start_page: start as u16,
                    end_page: end as u16,
                })
            }
            Book::Epub(b) => b.push_chapter(name, start),
            Book::Cbz(b) => b.push_chapter(name, start, number),
        }
    }
}

/// A finished book waiting in the scratch folder to be sent.
pub(crate) struct Volume {
    pub(crate) temp: PathBuf,
    /// Which group of chapters (see [`Grouping`]) it belongs to.
    pub(crate) group: usize,
    pub(crate) chapter_names: Vec<String>,
    pub(crate) first_number: Option<f64>,
    pub(crate) last_number: Option<f64>,
}

/// Deletes the scratch files however the export ends.
#[derive(Default)]
pub(crate) struct Scratch(pub(crate) Vec<PathBuf>);

impl Drop for Scratch {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = fs::remove_file(p);
        }
    }
}

/// The most pages a file may hold: what the job says, and over Wi-Fi also
/// what keeps a file under the size the reader can take in one go.
fn pages_per_file(job: &ExportJob, format: Format) -> usize {
    let mut cap = job.max_pages_per_file;
    // An EPUB is cut into parts a reader can load; a CBZ has no such limit.
    if job.series.settings.epub && !job.series.settings.cbz {
        cap = cap.min(epub::MAX_PAGES);
    }
    if matches!(job.target, ExportTarget::WebDav { .. }) {
        let by_size = if job.series.settings.pictures() {
            let depth = crate::options::output_depth(&job.device, &job.series.settings);
            let page_bytes = if job.series.settings.keeps_color(&job.device) {
                epub::typical_color_page_bytes()
            } else {
                epub::typical_page_bytes(depth)
            };
            (crate::export::WIFI_MAX_FILE_BYTES / page_bytes).max(1) as usize
        } else {
            wifi_page_cap(job.device.width, job.device.height, format)
        };
        cap = cap.min(by_size);
    }
    cap
}

/// How many pages of a `width` x `height` screen fit in one file sent over Wi-Fi.
fn wifi_page_cap(width: u32, height: u32, format: Format) -> usize {
    let pixels = u64::from(width) * u64::from(height);
    let page_bytes = match format {
        Format::Xtc => pixels / 8,
        Format::Xtch => pixels / 4,
    }
    .max(1);
    (crate::export::WIFI_MAX_FILE_BYTES / page_bytes).max(1) as usize
}

/// Split `count` chapters (by position) into groups.
pub(crate) fn group_ranges(count: usize, grouping: Grouping) -> Vec<std::ops::Range<usize>> {
    let size = match grouping {
        Grouping::Single => count.max(1),
        Grouping::PerChapter => 1,
        Grouping::EveryN(n) => n.max(1),
    };
    (0..count)
        .step_by(size)
        .map(|start| start..(start + size).min(count))
        .collect()
}

pub(crate) fn unique_name(kind: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "mangapoint-{}-{}.{kind}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// How much decoded picture the chapters being converted may hold between
/// them, in MiB, whatever the number of chapters converted at once. A page
/// decoded for conversion is one byte a pixel and a whole chapter is held
/// until it is converted, so a few big chapters at once would otherwise reach
/// several gigabytes.
const DECODED_BUDGET_MB: u32 = 1536;

/// How many pixels a chapter of `pixels` pixels holds while it is converted:
/// a colour chapter keeps a luma and two chroma planes, so three times as many.
fn held_pixels(pixels: u64, color: bool) -> u64 {
    if color {
        pixels.saturating_mul(3)
    } else {
        pixels
    }
}

/// A share of the memory the decoded chapters may use. A chapter takes what
/// all its pages need at once before it decodes the first, so two chapters
/// cannot each hold half of what the other is waiting for.
#[derive(Clone)]
struct MemoryBudget {
    pool: Arc<Semaphore>,
    total_mb: u32,
}

impl MemoryBudget {
    fn new(total_mb: u32) -> Self {
        Self {
            pool: Arc::new(Semaphore::new(total_mb as usize)),
            total_mb,
        }
    }

    /// MiB to reserve for pictures that decode to `pixels` pixels in all: one
    /// byte a pixel, a quarter more for the cropped or scaled copy that exists
    /// while the original is let go, and never more than the whole budget, so
    /// that a huge chapter runs alone instead of waiting for ever.
    fn weight(&self, pixels: u64) -> u32 {
        let mb = (pixels.saturating_add(pixels / 4)).div_ceil(1 << 20).max(1);
        mb.min(self.total_mb as u64) as u32
    }

    async fn acquire(&self, pixels: u64) -> OwnedSemaphorePermit {
        self.pool
            .clone()
            .acquire_many_owned(self.weight(pixels))
            .await
            .expect("the decode memory budget remains open")
    }

    #[cfg(test)]
    fn try_acquire(&self, pixels: u64) -> Option<OwnedSemaphorePermit> {
        self.pool.clone().try_acquire_many_owned(self.weight(pixels)).ok()
    }

    #[cfg(test)]
    fn free_mb(&self) -> usize {
        self.pool.available_permits()
    }
}

/// Whether the first chapter of a file leaves room for the cover under the
/// page limit. The cover is a page like the others; a chapter that fills the
/// file by itself keeps the file within its limit and goes without.
fn cover_fits(page_cap: usize, chapter_pages: usize) -> bool {
    chapter_pages < page_cap
}

/// What a task that did not finish says: for a panic, its message, so that a
/// bug is reported as one and not as a page that could not be read.
fn task_failed(
    what: &str,
    error: tokio::task::JoinError,
    otherwise: impl FnOnce(String) -> PipelineError,
) -> PipelineError {
    if error.is_panic() {
        let payload = error.into_panic();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".into());
        // There is no variant of its own for a bug; this is the closest.
        PipelineError::Decode(format!("internal error while {what}: {message}"))
    } else {
        otherwise(error.to_string())
    }
}

fn cancelled(_: converter::plan::Cancelled) -> PipelineError {
    PipelineError::Cancelled
}

/// Bytes the scratch files of an export may take on the system drive: the
/// pages as they are spooled, and for an XTC or XTCH book the finished book
/// written next to them. Counted from the pages saved, one page each, which
/// is the least a chapter can make (a spread or a zoom makes more), so it does
/// not turn away an export that would have fitted.
fn scratch_needed(source_pages: usize, page_bytes: u64, pictures: bool) -> u64 {
    let copies = if pictures { 1 } else { 2 };
    let bytes = source_pages as u64 * page_bytes * copies;
    bytes + bytes / 20
}

/// The room left on the drive that holds `dir`, or `None` for a place that
/// matches no drive.
pub(crate) fn available_space(dir: &Path) -> Option<u64> {
    #[cfg(test)]
    if let Some(free) = SCRATCH_FREE.with(|c| c.get()) {
        return Some(free);
    }
    let dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().components().count())
        .map(|d| d.available_space())
}

pub(crate) fn space_verdict(available: Option<u64>, needed: u64, dir: &Path) -> Result<()> {
    match available {
        Some(available) if available < needed => Err(PipelineError::NoSpace {
            mount: dir.to_path_buf(),
            needed,
            available,
        }),
        _ => Ok(()),
    }
}

/// The size of one converted page of a book, as a guess for the room it needs.
fn page_bytes_estimate(opts: &ProcessOptions, pictures: bool) -> u64 {
    if pictures {
        return if opts.color {
            epub::typical_color_page_bytes()
        } else {
            epub::typical_page_bytes(opts.depth)
        };
    }
    let (w, h) = opts.output_size();
    converter::xtc::PAGE_HEADER_SIZE as u64
        + match opts.depth {
            converter::Depth::One => converter::xtc::xtg_data_size(w, h),
            _ => converter::xtc::xth_data_size(w, h),
        } as u64
}

/// Whether a process with this id is running now.
#[cfg(windows)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    use std::ffi::c_void;
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    // SAFETY: plain Win32 calls with a handle that is closed before returning.
    unsafe {
        let handle = OpenProcess(QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // No such process; any other failure (access denied) means there is one.
            return std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INVALID_PARAMETER);
        }
        let mut code = 0u32;
        let known = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        known == 0 || code == STILL_ACTIVE
    }
}

#[cfg(unix)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const ESRCH: i32 = 3;
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process can be signalled.
    unsafe { kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() != Some(ESRCH) }
}

/// The process id in the name of a scratch file of ours,
/// `mangapoint-<pid>-<n>.spool`, `.book` or `.part`; `None` for any other name.
fn spool_file_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("mangapoint-")?;
    let (stem, extension) = rest.rsplit_once('.')?;
    if !matches!(extension, "spool" | "book" | "part") {
        return None;
    }
    let (pid, n) = stem.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (digits(pid) && digits(n)).then(|| pid.parse().ok())?
}

/// Delete the scratch files an export left in `dir` when the app was closed
/// or killed part-way: files named as this crate names them
/// (`mangapoint-<pid>-<n>.spool`, `.book`, `.part`) whose process is no longer
/// running, or that were last written more than `older_than` ago. Nothing else
/// is touched, and nothing below `dir`. Call it when the app starts, with the
/// folder exports use as their `work_dir`. Returns how many files it removed.
pub fn sweep_spool(dir: &Path, older_than: Duration) -> usize {
    sweep_spool_at(dir, older_than, SystemTime::now(), &process_is_running)
}

fn sweep_spool_at(
    dir: &Path,
    older_than: Duration,
    now: SystemTime,
    running: &dyn Fn(u32) -> bool,
) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        // A real file only: not a folder, not a link to somewhere else.
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Some(pid) = entry.file_name().to_str().and_then(spool_file_pid) else {
            continue;
        };
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= older_than);
        let orphan = pid != std::process::id() && !running(pid);
        if (old || orphan) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Take back the files this export already sent to a folder or a drive. Files
/// on the reader's Wi-Fi share are left: there is no way to take them back.
pub(crate) fn remove_sent(target: &ExportTarget, sent: &[ExportedFile]) {
    if matches!(target, ExportTarget::WebDav { .. }) {
        return;
    }
    for file in sent {
        let _ = fs::remove_file(&file.location);
    }
}

#[cfg(test)]
thread_local! {
    /// Test hook: cancel the export once this many files have been sent.
    static CANCEL_AFTER_SENT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// Test hook: lose the next file once this many have been sent.
    static LOSE_AFTER_SENT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// Test hook: the room the scratch drive appears to have.
    static SCRATCH_FREE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Build the files and send them to the target.
///
/// If a file after the first cannot be sent, or the export is cancelled, the
/// files already sent are named in the error (or, on a cancel, removed from a
/// folder or drive), so no half-finished set is left unannounced.
pub async fn export_series(
    files: &LibraryFiles,
    job: &ExportJob,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<ExportOutcome> {
    if let Some(original) = &job.original {
        return crate::original::export_original(files, job, original, events, cancel).await;
    }
    let chapters: Vec<&DownloadedChapter> = job
        .series
        .chapters
        .iter()
        .filter(|c| job.chapter_keys.contains(&c.key))
        .collect();
    if chapters.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let emit = |e: JobEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(e);
        }
    };
    fs::create_dir_all(&job.work_dir).map_err(|e| PipelineError::Io(job.work_dir.clone(), e))?;

    let settings = &job.series.settings;
    let opts = process_options_for(&job.device, settings);
    let format = settings.format;
    let extension = settings.output().extension();
    let page_cap = pages_per_file(job, format);

    // Fail now, not after an hour of converting, if the scratch drive is too full.
    let source_pages: usize = chapters
        .iter()
        .map(|c| files.page_files(&job.series.id, &c.key).len())
        .sum();
    let needed = scratch_needed(
        source_pages,
        page_bytes_estimate(&opts, settings.pictures()),
        settings.pictures(),
    );
    space_verdict(available_space(&job.work_dir), needed, &job.work_dir)?;

    let mut scratch = Scratch::default();
    let mut volumes: Vec<Volume> = Vec::new();
    let mut total_pages = 0usize;
    let mut cached_chapters = 0usize;
    let total = chapters.len();
    // More chapters can prepare/pack independently, while the total number
    // of simultaneous decodes remains capped at the old two-chapter budget.
    let decode_slots = Semaphore::new(page_concurrency() * 2);
    // And what they hold decoded, in all, is held to a budget.
    let budget = MemoryBudget::new(DECODED_BUDGET_MB);

    // Chapters are converted a little ahead of the one being written, so the
    // cores stay busy while a finished chapter is added to its book. They come
    // out of the stream in order.
    let order: Vec<(usize, usize)> = group_ranges(total, job.grouping)
        .into_iter()
        .enumerate()
        .flat_map(|(group, range)| range.map(move |ci| (group, ci)))
        .collect();
    let mut converted = stream::iter(order)
        .map(|(group, ci)| {
            let chapter = chapters[ci];
            let (series_id, opts, events, settings, decode_slots, budget) = (
                &job.series.id,
                &opts,
                &events,
                settings,
                &decode_slots,
                &budget,
            );
            async move {
                if cancel.is_cancelled() {
                    return (group, ci, Err(PipelineError::Cancelled));
                }
                if let Some(tx) = events {
                    let _ = tx.send(JobEvent::ChapterStarted {
                        index: ci,
                        total,
                        name: chapter.name.clone(),
                    });
                }
                let images = convert_chapter(
                    files,
                    series_id,
                    chapter,
                    opts,
                    settings,
                    &job.series.crops,
                    job.force_conversion,
                    decode_slots,
                    budget,
                    ci,
                    events,
                    cancel,
                )
                .await;
                if images.is_ok() {
                    if let Some(tx) = events {
                        let _ = tx.send(JobEvent::ChapterDone { index: ci });
                    }
                }
                (group, ci, images)
            }
        })
        .buffered(chapter_concurrency());

    // The series' own cover opens every file, so the reader lists it by its cover.
    let cover = if job.cover {
        cover_page(files, &job.series.id, &chapters[0].key, &opts, settings.pictures()).await
    } else {
        None
    };
    let mut open: Option<(Book, Volume)> = None;
    let mut open_group = usize::MAX;
    while let Some((group, ci, images)) = converted.next().await {
        let (images, cached) = images?;
        cached_chapters += usize::from(cached);
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let chapter = chapters[ci];

        // A new group starts a new file.
        if group != open_group {
            if let Some((builder, volume)) = open.take() {
                volumes.push(
                    finish_volume(builder, volume, &job.work_dir, &mut scratch, cancel).await?,
                );
            }
            open_group = group;
        }
        // Start a new file if this chapter would not fit in the open one.
        if let Some((builder, _)) = &open {
            if builder.page_count() + images.len() > page_cap {
                let (builder, volume) = open.take().expect("checked above");
                volumes.push(
                    finish_volume(builder, volume, &job.work_dir, &mut scratch, cancel).await?,
                );
            }
        }
        if open.is_none() {
            let spool = job.work_dir.join(unique_name("spool"));
            scratch.0.push(spool.clone());
            let book = if settings.cbz {
                let info = job.series.info.as_ref();
                Book::Cbz(Box::new(CbzBuilder::create(
                    &spool,
                    CbzMeta {
                        series: job.series.title.clone(),
                        language: job.language.clone(),
                        rtl: settings.direction == converter::xtc::ReadDirection::RightToLeft,
                        summary: info.map(|i| i.description.clone()).unwrap_or_default(),
                        author: info.map(|i| i.author.clone()).unwrap_or_default(),
                        artist: info.map(|i| i.artist.clone()).unwrap_or_default(),
                        genres: info.map(|i| i.genres.clone()).unwrap_or_default(),
                        status: info.map(|i| i.status.clone()).unwrap_or_default(),
                        sources: info
                            .map(|i| i.source_names.iter().map(|(_, n)| n.clone()).collect())
                            .unwrap_or_default(),
                        created: now_secs(),
                    },
                )?))
            } else if settings.epub {
                Book::Epub(Box::new(EpubBuilder::create(
                    &spool,
                    EpubMeta {
                        title: job.series.title.clone(),
                        language: job.language.clone(),
                        rtl: settings.direction == converter::xtc::ReadDirection::RightToLeft,
                        width: opts.width,
                        height: opts.height,
                        created: now_secs(),
                    },
                )?))
            } else {
                Book::Xtc(
                    XtcBuilder::new(format)
                        .direction(settings.direction)
                        .metadata(Metadata {
                            title: job.series.title.clone(),
                            language: job.language.clone(),
                            create_time: now_secs() as u32,
                            cover_page: Some(0),
                            ..Default::default()
                        })
                        .spool_to(&spool)?,
                )
            };
            open = Some((
                book,
                Volume {
                    temp: if settings.pictures() { spool.clone() } else { PathBuf::new() },
                    group,
                    chapter_names: Vec::new(),
                    first_number: None,
                    last_number: None,
                },
            ));
        }
        let (builder, volume) = open.as_mut().expect("opened above");
        if builder.page_count() == 0 {
            if let Some(cover) = cover.as_ref().filter(|_| cover_fits(page_cap, images.len())) {
                builder.add(cover)?;
            }
        }
        let start = builder.page_count();
        for image in &images {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            builder.add(image)?;
        }
        builder.chapter(&chapter.name, start, chapter.number);
        volume.chapter_names.push(chapter.name.clone());
        if volume.first_number.is_none() {
            volume.first_number = chapter.number;
        }
        if chapter.number.is_some() {
            volume.last_number = chapter.number;
        }
        total_pages += images.len();
    }
    if let Some((builder, volume)) = open.take() {
        volumes.push(finish_volume(builder, volume, &job.work_dir, &mut scratch, cancel).await?);
    }

    // Now that every file exists, name them (part numbers need the count) and
    // send them.
    let mut sent: Vec<ExportedFile> = Vec::new();
    let sizes: Vec<u64> = volumes
        .iter()
        .map(|v| fs::metadata(&v.temp).map_or(0, |m| m.len()))
        .collect();
    let all: u64 = sizes.iter().sum();
    let mut before = 0u64;
    for (i, volume) in volumes.iter().enumerate() {
        #[cfg(test)]
        {
            if CANCEL_AFTER_SENT.with(|c| c.get()) == Some(sent.len()) {
                cancel.cancel();
            }
            if LOSE_AFTER_SENT.with(|c| c.get()) == Some(sent.len()) {
                let _ = fs::remove_file(&volume.temp);
            }
        }
        let name = volume_name(job, volume, &volumes, i, extension);
        let result = if cancel.is_cancelled() {
            Err(PipelineError::Cancelled)
        } else {
            emit(JobEvent::Writing { file: name.clone() });
            emit(JobEvent::Sent { done: before, total: all });
            let (tx_progress, tx_flush) = (events.clone(), events.clone());
            let mut last = std::time::Instant::now();
            job.target
                .write_file_with(
                    &name,
                    &volume.temp,
                    move |written| {
                        // Several a second is plenty to draw.
                        let finished = before + written >= all;
                        if let Some(tx) = &tx_progress {
                            if finished || last.elapsed() >= std::time::Duration::from_millis(100) {
                                last = std::time::Instant::now();
                                let _ = tx.send(JobEvent::Sent { done: before + written, total: all });
                            }
                        }
                    },
                    move || {
                        if let Some(tx) = tx_flush {
                            let _ = tx.send(JobEvent::Flushing);
                        }
                    },
                    cancel,
                )
                .await
        };
        match result {
            Ok(file) => sent.push(file),
            Err(PipelineError::Cancelled) => {
                // A half-set of files is of no use: take back what went out.
                remove_sent(&job.target, &sent);
                return Err(PipelineError::Cancelled);
            }
            Err(e) if !sent.is_empty() => {
                let list: Vec<&str> = sent.iter().map(|f| f.location.as_str()).collect();
                return Err(PipelineError::Upload(format!(
                    "{e}. These files were already sent and are still there: {}",
                    list.join(", ")
                )));
            }
            Err(e) => return Err(e),
        }
        before += sizes[i];
    }
    Ok(ExportOutcome {
        files: sent,
        pages: total_pages,
        chapters: total,
        cached_chapters,
    })
}

/// Set by a test to make the cover's conversion panic once.
#[cfg(test)]
static PANIC_IN_COVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The saved cover of a series as one page for the device: the whole picture,
/// fitted to the screen, uncropped. A series without a cover uses the first
/// page of its first chapter. `None` if there is neither, or it cannot be
/// read, in which case the file simply starts with the first chapter.
async fn cover_page(
    files: &LibraryFiles,
    series_id: &str,
    first_chapter: &str,
    opts: &ProcessOptions,
    epub: bool,
) -> Option<Vec<u8>> {
    let bytes = match tokio::fs::read(files.cover_path(series_id)).await {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => {
            let first = files.page_files(series_id, first_chapter).into_iter().next()?;
            tokio::fs::read(first).await.ok()?
        }
    };
    let mut opts = opts.clone();
    opts.layout = converter::process::Layout::Manga;
    opts.crop = converter::process::CropMode::Full;
    opts.margins = Default::default();
    opts.spread = converter::process::SpreadMode::Fit;
    let made = tokio::task::spawn_blocking(move || {
        #[cfg(test)]
        if PANIC_IN_COVER.swap(false, Ordering::SeqCst) {
            panic!("injected cover failure");
        }
        if epub && opts.color {
            let (luma, chroma) = converter::decode::decode_to_ycc(&bytes).ok()?;
            let prepared = converter::plan::prepare_owned_color(vec![(luma, chroma)], &opts, &|| false).ok()?;
            let plan = converter::plan::plan(&prepared, &opts);
            let page = converter::plan::render_page_color(&prepared, plan.pages.first()?, &opts, [0.0; 4]);
            return epub::encode_color_page(&page).ok();
        }
        let gray = decode_page(&bytes).ok()?;
        let page = converter::plan::convert_chapter(&[gray], &opts)
            .into_iter()
            .next()?;
        if epub {
            epub::encode_page(&page).ok()
        } else {
            converter::xtc::encode_page(&page).ok()
        }
    })
    .await;
    match made {
        Ok(cover) => cover,
        Err(e) => {
            // A bug, not a cover that cannot be read: the file goes without a
            // cover, and the reason is not lost.
            let error = task_failed("making the cover", e, PipelineError::Decode);
            eprintln!("mangapoint: the cover was left out: {error}");
            None
        }
    }
}

/// Write a finished book out as its file. A book can be gigabytes, so this
/// happens on a thread of its own, not the one the app's window runs on.
async fn finish_volume(
    book: Book,
    volume: Volume,
    work_dir: &Path,
    scratch: &mut Scratch,
    cancel: &CancelFlag,
) -> Result<Volume> {
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let interrupted = |m: String| PipelineError::Upload(format!("writing the book was interrupted: {m}"));
    let flag = cancel.clone();
    match book {
        Book::Xtc(builder) => {
            let temp = work_dir.join(unique_name("book"));
            scratch.0.push(temp.clone());
            tokio::task::spawn_blocking(move || write_volume(builder, volume, temp, &flag))
                .await
                .map_err(|e| task_failed("writing the book", e, interrupted))?
        }
        // An EPUB or a CBZ has been written to its file as it grew; this closes it.
        Book::Epub(builder) => tokio::task::spawn_blocking(move || {
            if flag.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            (*builder).finish()?;
            Ok(volume)
        })
        .await
        .map_err(|e| task_failed("writing the book", e, interrupted))?,
        Book::Cbz(builder) => tokio::task::spawn_blocking(move || {
            if flag.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            (*builder).finish()?;
            Ok(volume)
        })
        .await
        .map_err(|e| task_failed("writing the book", e, interrupted))?,
    }
}

fn write_volume(
    builder: XtcBuilder,
    mut volume: Volume,
    temp: PathBuf,
    cancel: &CancelFlag,
) -> Result<Volume> {
    let file = File::create(&temp).map_err(|e| PipelineError::Io(temp.clone(), e))?;
    let mut writer = BufWriter::new(file);
    builder
        .write_to_cancellable(&mut writer, &|| cancel.is_cancelled())
        .map_err(|e| match e {
            WriteError::Xtc(e) => PipelineError::Xtc(e),
            WriteError::Io(e) => PipelineError::Io(temp.clone(), e),
            WriteError::Cancelled => PipelineError::Cancelled,
        })?;
    writer
        .flush()
        .map_err(|e| PipelineError::Io(temp.clone(), e))?;
    volume.temp = temp;
    Ok(volume)
}

/// `Title - Ch 1-12.xtch`; with several files from one group,
/// `Title - Part 2 - Ch 13-25.xtch`; a lone unnumbered chapter is named
/// after itself.
pub(crate) fn volume_name(
    job: &ExportJob,
    volume: &Volume,
    all: &[Volume],
    index: usize,
    ext: &str,
) -> String {
    let in_group: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, v)| v.group == volume.group)
        .map(|(i, _)| i)
        .collect();
    let part =
        (in_group.len() > 1).then(|| in_group.iter().position(|&i| i == index).unwrap_or(0) + 1);
    if volume.chapter_names.len() == 1 && volume.first_number.is_none() {
        let mut name = format!(
            "{} - {}",
            sanitize_component(&job.series.title),
            sanitize_component(&volume.chapter_names[0])
        );
        if let Some(p) = part {
            name.push_str(&format!(" - Part {p}"));
        }
        return format!("{name}.{ext}");
    }
    output_file_name(
        &job.series.title,
        volume.first_number,
        volume.last_number,
        part,
        ext,
    )
}

/// How many pixels each saved page will be once decoded, read from the start
/// of its file. A page whose size cannot be read counts as nothing: decoding
/// it will say what is wrong with it.
fn decoded_pixels(pages: &[PathBuf]) -> u64 {
    pages
        .iter()
        .map(|path| {
            let dimensions = |bytes: &[u8]| converter::decode::image_dimensions(bytes).ok();
            let mut head = Vec::new();
            let read_head = File::open(path)
                .and_then(|f| std::io::Read::read_to_end(&mut std::io::Read::take(f, 256 * 1024), &mut head));
            // A header further in than that (a JPEG with a big thumbnail) needs the whole file.
            let size = read_head
                .ok()
                .and_then(|_| dimensions(&head))
                .or_else(|| fs::read(path).ok().and_then(|bytes| dimensions(&bytes)));
            size.map_or(0, |(w, h)| u64::from(w) * u64::from(h))
        })
        .sum()
}

/// Convert decoded pages into the packed pages of a book (or the pictures of
/// an EPUB/CBZ). Pages are drawn on every core and each is encoded on the core
/// that drew it, then let go; the packed pages come back in page order.
/// `cuts_of` gives the manual crop of each planned page (none: no cuts).
/// The decoded images are consumed, so a chapter never exists twice.
fn pack_chapter(
    grays: Vec<GrayImage>,
    opts: &ProcessOptions,
    cuts_of: &dyn Fn(&converter::ChapterPlan) -> Vec<[f32; 4]>,
    pictures: bool,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Vec<u8>>> {
    let prepared = converter::plan::prepare_owned(grays, opts, cancel).map_err(cancelled)?;
    let plan = converter::plan::plan(&prepared, opts);
    let cuts = cuts_of(&plan);
    let packed = converter::plan::render_all_cropped_with(
        &prepared,
        &plan.pages,
        opts,
        &cuts,
        cancel,
        |_, image| -> Result<Vec<u8>> {
            Ok(if pictures {
                epub::encode_page(&image)?
            } else {
                converter::xtc::encode_page(&image)?
            })
        },
    )
    .map_err(cancelled)?;
    packed.into_iter().collect()
}

/// [`pack_chapter`] for pages kept in colour: each page is drawn by
/// `render_page_color` (no tone work, no dithering) and saved as a colour JPEG
/// on the core that drew it. The decoded planes are consumed.
fn pack_chapter_color(
    pages: Vec<(GrayImage, [GrayImage; 2])>,
    opts: &ProcessOptions,
    cuts_of: &dyn Fn(&converter::ChapterPlan) -> Vec<[f32; 4]>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Vec<u8>>> {
    let prepared = converter::plan::prepare_owned_color(pages, opts, cancel).map_err(cancelled)?;
    let plan = converter::plan::plan(&prepared, opts);
    let cuts = cuts_of(&plan);
    let packed = converter::plan::render_all_color_with(
        &prepared,
        &plan.pages,
        opts,
        &cuts,
        cancel,
        |_, image| -> Result<Vec<u8>> { epub::encode_color_page(&image) },
    )
    .map_err(cancelled)?;
    packed.into_iter().collect()
}

/// A saved page decoded: its place in the chapter, its luma, its chroma planes
/// when the chapter is kept in colour, and the digest that stands for its bytes.
type DecodedPage = (usize, GrayImage, Option<[GrayImage; 2]>, [u8; 32]);

/// Decode every saved page of a chapter, then plan and convert it as a whole:
/// a webtoon's images are joined before they are cut into pages, so the pages
/// match what the preview showed and the user's corrections apply.
#[allow(clippy::too_many_arguments)]
async fn convert_chapter(
    files: &LibraryFiles,
    series_id: &str,
    chapter: &DownloadedChapter,
    opts: &ProcessOptions,
    settings: &library::ConversionSettings,
    crops: &library::CropOverrides,
    force_conversion: bool,
    decode_slots: &Semaphore,
    budget: &MemoryBudget,
    chapter_index: usize,
    events: &Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<(Vec<Vec<u8>>, bool)> {
    let pages = files.page_files(series_id, &chapter.key);
    if pages.is_empty() {
        return Err(PipelineError::Decode(format!(
            "{} has no saved pages; download it again",
            chapter.name
        )));
    }
    let total = pages.len();
    let epub = settings.pictures();
    let cache_path = files.chapter_dir(series_id, &chapter.key).join(if epub {
        conversion_cache::EPUB_FILE_NAME
    } else {
        conversion_cache::FILE_NAME
    });
    let key = conversion_cache::cache_key(opts, epub);
    let (source_pages, cache, flag, lookup_key) =
        (pages.clone(), cache_path.clone(), cancel.clone(), key.clone());
    let ((width, height), format) = (opts.output_size(), settings.format);
    let crop_configuration = crops.chapter_configuration(&chapter.key);
    let lookup_crops = crop_configuration.clone();
    // A first export has no cache to look up. Hash its decoded snapshot
    // below instead of reading every source twice just to establish a miss.
    let cached = if !force_conversion && cache_path.is_file() {
        tokio::task::spawn_blocking(move || {
            let fingerprint = conversion_cache::fingerprint(&source_pages, &lookup_key, &flag)?;
            let fingerprint = conversion_cache::with_crops(fingerprint, &lookup_crops);
            Ok::<_, PipelineError>(if epub {
                conversion_cache::load_pictures(&cache, &fingerprint)
            } else {
                conversion_cache::load(&cache, &fingerprint, width, height, format)
            })
        })
        .await
        .map_err(|e| task_failed("reading the converted chapter", e, PipelineError::Decode))??
    } else {
        None
    };
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    if let Some(cached) = cached {
        return Ok((cached, true));
    }

    // Wait for room for the whole chapter, decoded, before decoding any of it.
    let sizes = {
        let pages = pages.clone();
        tokio::task::spawn_blocking(move || decoded_pixels(&pages))
            .await
            .map_err(|e| task_failed("measuring the chapter", e, PipelineError::Decode))?
    };
    let _memory = budget.acquire(held_pixels(sizes, opts.color && epub)).await;
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }

    let done = std::sync::atomic::AtomicUsize::new(0);
    let done = &done;
    let name = chapter.name.as_str();

    // Pages kept in colour are decoded to a luma and two chroma planes.
    let color = epub && opts.color;
    let decoded: Vec<Result<DecodedPage>> = stream::iter(pages.into_iter().enumerate())
        .map(|(index, path)| async move {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let permit = decode_slots.acquire().await.expect("the export decode budget remains open");
            if cancel.is_cancelled() { return Err(PipelineError::Cancelled); }
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| PipelineError::Io(path.clone(), e))?;
            // The file's bytes are let go as soon as they are decoded; only
            // the short digest that stands for them in the cache key stays.
            let (gray, chroma, digest) = tokio::task::spawn_blocking(move || {
                let digest = conversion_cache::page_digest(&bytes);
                if color {
                    converter::decode::decode_to_ycc(&bytes)
                        .map(|(luma, chroma)| (luma, Some(chroma), digest))
                        .map_err(|e| PipelineError::Decode(e.to_string()))
                } else {
                    decode_page(&bytes).map(|gray| (gray, None, digest))
                }
            })
            .await
            .map_err(|e| task_failed("reading a page", e, PipelineError::Decode))?
            .map_err(|e| {
                PipelineError::Decode(format!(
                    "page {} of {name} could not be read ({e}); delete the chapter and download it again",
                    index + 1
                ))
            })?;
            drop(permit);
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(tx) = events {
                let _ = tx.send(JobEvent::PageDone {
                    chapter_index,
                    done: n,
                    total,
                });
            }
            Ok((index, gray, chroma, digest))
        })
        // A tall early image must not prevent later completed reads from
        // releasing their slots and starting more work. Restore order below.
        .buffer_unordered(page_concurrency())
        .collect()
        .await;

    let mut decoded = decoded.into_iter().collect::<Result<Vec<_>>>()?;
    decoded.sort_unstable_by_key(|(index, _, _, _)| *index);
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let opts = opts.clone();
    let cancel = cancel.clone();
    let crops = crops.clone();
    let chapter_key = chapter.key.clone();
    let packed = tokio::task::spawn_blocking(move || {
        let digests: Vec<[u8; 32]> = decoded.iter().map(|(_, _, _, digest)| *digest).collect();
        let fingerprint = conversion_cache::fingerprint_digests(&digests, &key);
        let fingerprint = conversion_cache::with_crops(fingerprint, &crop_configuration);
        let stop = || cancel.is_cancelled();
        let cuts_of = |plan: &converter::ChapterPlan| -> Vec<[f32; 4]> {
            if crop_configuration.is_empty() {
                return Vec::new();
            }
            plan.pages
                .iter()
                .map(|page| {
                    crops
                        .effective(&chapter_key, &crate::preview::crop_page_key(page))
                        .unwrap_or_default()
                        .as_array()
                })
                .collect()
        };
        let packed = if color {
            let pages = decoded
                .into_iter()
                .map(|(_, luma, chroma, _)| {
                    let chroma = chroma.expect("a colour page was decoded with its chroma");
                    (luma, chroma)
                })
                .collect();
            pack_chapter_color(pages, &opts, &cuts_of, &stop)?
        } else {
            let grays = decoded.into_iter().map(|(_, gray, _, _)| gray).collect();
            pack_chapter(grays, &opts, &cuts_of, epub, &stop)?
        };
        if epub {
            conversion_cache::save_pictures(&cache_path, &fingerprint, &packed);
        } else {
            conversion_cache::save(&cache_path, &fingerprint, &packed);
        }
        Ok::<_, PipelineError>(packed)
    })
    .await
    .map_err(|e| task_failed("converting the chapter", e, PipelineError::Decode))??;
    Ok((packed, false))
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_sent_over_wifi_is_cut_into_files_the_reader_can_take() {
        use converter::xtc::Format;
        // 480x800 pages are 96,000 bytes in XTCH and 48,000 in XTC.
        let xtch = wifi_page_cap(480, 800, Format::Xtch);
        let xtc = wifi_page_cap(480, 800, Format::Xtc);
        assert_eq!(xtc, xtch * 2);
        assert!((xtch as u64) * 96_000 <= crate::export::WIFI_MAX_FILE_BYTES);
        assert!(xtch > 5_000 && xtch < 6_000);
        // The 235 chapters that took an X4 Pro down (about 2.8 GB) come out as several parts.
        assert!(2_790_000_000u64 / (xtch as u64 * 96_000) >= 5);
    }

    #[test]
    fn groups_split_chapters_the_way_each_option_says() {
        assert_eq!(group_ranges(5, Grouping::Single), vec![0..5]);
        assert_eq!(
            group_ranges(3, Grouping::PerChapter),
            vec![0..1, 1..2, 2..3]
        );
        assert_eq!(group_ranges(5, Grouping::EveryN(2)), vec![0..2, 2..4, 4..5]);
        assert_eq!(
            group_ranges(5, Grouping::EveryN(0)),
            vec![0..1, 1..2, 2..3, 3..4, 4..5],
            "0 counts as 1"
        );
        assert_eq!(group_ranges(2, Grouping::EveryN(10)), vec![0..2]);
        assert!(group_ranges(0, Grouping::Single).is_empty());
    }

    #[test]
    fn scratch_files_are_removed_when_dropped() {
        let dir = std::env::temp_dir().join(format!("mangapoint-scratch-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.tmp");
        fs::write(&file, b"x").unwrap();
        drop(Scratch(vec![file.clone(), dir.join("never-existed")]));
        assert!(!file.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn temporary_names_do_not_repeat() {
        assert_ne!(unique_name("book"), unique_name("book"));
    }

    // ---- Audit fixes ---------------------------------------------------------

    use image::ImageEncoder;
    use library::{ConversionSettings, LibraryDb};

    fn test_png(w: u32, h: u32, seed: u8) -> Vec<u8> {
        let pixels: Vec<u8> = (0..w * h)
            .map(|i| ((i % w) * 3 + (i / w) * 2) as u8 ^ seed)
            .collect();
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&pixels, w, h, image::ExtendedColorType::L8)
            .unwrap();
        out
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-compose-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A library on disk with `chapters` chapters of `pages` pages each.
    fn fixture(tag: &str, chapters: usize, pages: usize) -> (PathBuf, LibraryFiles, DownloadedSeries) {
        let root = scratch_dir(tag);
        let files = LibraryFiles::new(root.join("library"));
        let title = "Test Series";
        let id = library::series_id(title);
        let mut db = LibraryDb::default();
        for c in 1..=chapters {
            let key = format!("c{c}");
            fs::create_dir_all(files.chapter_dir(&id, &key)).unwrap();
            for p in 0..pages {
                fs::write(files.page_path(&id, &key, p), test_png(300, 450, (c * 10 + p) as u8)).unwrap();
            }
            db.record_chapter(
                title,
                &ConversionSettings::default(),
                library::DownloadedChapter {
                    key,
                    number: Some(c as f64),
                    name: format!("Chapter {c}"),
                    source_id: "s".into(),
                    chapter_id: c as i64,
                    pages,
                    bytes: 0,
                    downloaded_at: 1,
                    quality: None,
                },
                1,
            );
        }
        (root.clone(), files, db.series.remove(0))
    }

    fn folder_job(root: &Path, series: &DownloadedSeries) -> ExportJob {
        let mut job = ExportJob::new(
            series.clone(),
            series.chapters.iter().map(|c| c.key.clone()).collect(),
            device_profiles::x4_pro(),
            ExportTarget::Folder { path: root.join("out") },
            root.join("work"),
        );
        job.cover = false;
        job
    }

    fn folder_files(root: &Path) -> Vec<String> {
        fs::read_dir(root.join("out"))
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    // The memory budget

    #[test]
    fn a_colour_chapter_weighs_three_planes_and_a_colour_page_is_guessed_bigger() {
        assert_eq!(held_pixels(1_000, false), 1_000);
        assert_eq!(held_pixels(1_000, true), 3_000);
        assert_eq!(held_pixels(u64::MAX, true), u64::MAX, "no overflow");
        let b = MemoryBudget::new(1000);
        let pixels = 100 << 20;
        assert_eq!(b.weight(held_pixels(pixels, true)), 3 * b.weight(pixels), "three planes reserve three times");
        // A colour chapter that would not leave room for another runs alone, not for ever.
        assert_eq!(b.weight(held_pixels(2000 << 20, true)), 1000);
        let mut gray = ProcessOptions::manga_default(1264, 1680, converter::Depth::Eight);
        let gray_bytes = page_bytes_estimate(&gray, true);
        gray.color = true;
        assert!(page_bytes_estimate(&gray, true) > 3 * gray_bytes);
        let mut packed = gray.clone();
        packed.color = false;
        assert_eq!(page_bytes_estimate(&gray, false), page_bytes_estimate(&packed, false), "packed pages ignore colour");
    }

    #[test]
    fn a_chapters_share_of_the_budget_follows_its_pixels_and_never_exceeds_it() {
        let b = MemoryBudget::new(100);
        assert_eq!(b.weight(0), 1, "even an empty chapter takes a unit");
        assert_eq!(b.weight(40 << 20), 50, "a quarter more than the decoded pictures");
        assert_eq!(b.weight(10_000_000_000), 100, "a huge chapter takes the lot and runs alone");
        assert_eq!(b.weight(u64::MAX), 100);
    }

    #[test]
    fn chapters_that_do_not_fit_together_wait_for_each_other() {
        let b = MemoryBudget::new(100);
        let first = b.try_acquire(60 << 20).expect("75 of 100");
        assert_eq!(b.free_mb(), 25);
        assert!(b.try_acquire(60 << 20).is_none(), "another 75 does not fit");
        let small = b.try_acquire(10 << 20).expect("a small one still does");
        drop(first);
        assert!(b.try_acquire(60 << 20).is_some(), "room again once the first is converted");
        drop(small);
        // A chapter bigger than everything takes the whole budget, alone.
        let all = b.try_acquire(1 << 40).expect("the pool is free");
        assert_eq!(b.free_mb(), 0);
        assert!(b.try_acquire(1 << 20).is_none());
        drop(all);
        assert_eq!(b.free_mb(), 100);
    }

    #[tokio::test]
    async fn a_waiting_chapter_goes_on_as_soon_as_room_is_freed() {
        let b = MemoryBudget::new(100);
        let first = b.acquire(60 << 20).await;
        let waiting = tokio::time::timeout(Duration::from_millis(50), b.acquire(60 << 20)).await;
        assert!(waiting.is_err(), "it must wait while the first holds the memory");
        let b2 = b.clone();
        let second = tokio::spawn(async move { b2.acquire(60 << 20).await });
        tokio::task::yield_now().await;
        drop(first);
        let permit = tokio::time::timeout(Duration::from_secs(5), second).await.unwrap().unwrap();
        drop(permit);
        assert_eq!(b.free_mb(), 100);
    }

    #[test]
    fn the_pixels_of_a_chapter_are_read_from_the_heads_of_its_files() {
        let dir = scratch_dir("pixels");
        let (a, b, junk) = (dir.join("1.img"), dir.join("2.img"), dir.join("3.img"));
        fs::write(&a, test_png(30, 40, 0)).unwrap();
        fs::write(&b, test_png(100, 20, 0)).unwrap();
        fs::write(&junk, b"not a picture").unwrap();
        assert_eq!(decoded_pixels(&[a, b, junk, dir.join("missing.img")]), 30 * 40 + 100 * 20);
        let _ = fs::remove_dir_all(dir);
    }

    // Scratch files and space

    #[test]
    fn only_our_own_scratch_files_of_a_dead_or_old_process_are_swept() {
        let dir = scratch_dir("sweep");
        let touch = |name: &str| fs::write(dir.join(name), b"x").unwrap();
        for name in [
            "mangapoint-111-0.spool",  // a process that is gone
            "mangapoint-111-1.book",
            "mangapoint-111-2.part",
            "mangapoint-222-0.spool",  // a process that is running
            "mangapoint-library.json", // not our pattern
            "mangapoint-333-0.txt",
            "mangapoint-333-x.spool",
            "mangapoint--0.spool",
            "notes.spool",
            "Series - Ch 1.xtc",
        ] {
            touch(name);
        }
        fs::create_dir(dir.join("mangapoint-111-9.spool")).unwrap(); // a folder, not a file
        fs::write(dir.join("mangapoint-111-9.spool").join("mangapoint-111-0.spool"), b"x").unwrap();
        let running = |pid: u32| pid == 222;

        let removed = sweep_spool_at(&dir, Duration::from_secs(3600), SystemTime::now(), &running);
        assert_eq!(removed, 3);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "Series - Ch 1.xtc",
                "mangapoint--0.spool",
                "mangapoint-111-9.spool",
                "mangapoint-222-0.spool",
                "mangapoint-333-0.txt",
                "mangapoint-333-x.spool",
                "mangapoint-library.json",
                "notes.spool",
            ]
        );
        assert!(
            dir.join("mangapoint-111-9.spool").join("mangapoint-111-0.spool").exists(),
            "nothing below the folder is touched"
        );

        // A running process's file goes only once it is old.
        let later = SystemTime::now() + Duration::from_secs(7200);
        assert_eq!(sweep_spool_at(&dir, Duration::from_secs(3600), later, &running), 1);
        assert!(!dir.join("mangapoint-222-0.spool").exists());
        assert!(dir.join("notes.spool").exists());
        // Our own live export's files are kept while they are young.
        let mine = format!("mangapoint-{}-77.spool", std::process::id());
        fs::write(dir.join(&mine), b"x").unwrap();
        assert_eq!(sweep_spool(&dir, Duration::from_secs(3600)), 0);
        assert!(dir.join(&mine).exists());
        assert_eq!(sweep_spool(&dir.join("nowhere"), Duration::ZERO), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn this_process_is_running_and_an_impossible_one_is_not() {
        assert!(process_is_running(std::process::id()));
        assert!(!process_is_running(u32::MAX - 1));
    }

    #[test]
    fn the_room_a_book_needs_is_counted_from_its_pages_and_checked_before_converting() {
        let mut opts = ProcessOptions::manga_default(480, 800, converter::Depth::One);
        assert_eq!(page_bytes_estimate(&opts, false), 22 + 48_000);
        opts.depth = converter::Depth::Two;
        assert_eq!(page_bytes_estimate(&opts, false), 22 + 96_000);
        assert_eq!(page_bytes_estimate(&opts, true), 22_000);
        // The spooled pages and the finished book; an EPUB is only its spool. Plus 5%.
        assert_eq!(scratch_needed(1000, 48_022, false), 2 * 48_022_000 + 2 * 48_022_000 / 20);
        assert_eq!(scratch_needed(1000, 22_000, true), 22_000_000 + 22_000_000 / 20);
        assert_eq!(scratch_needed(0, 48_022, false), 0);
        let dir = Path::new("scratch");
        assert!(matches!(
            space_verdict(Some(10_000), 20_000, dir),
            Err(PipelineError::NoSpace { needed: 20_000, available: 10_000, .. })
        ));
        assert!(space_verdict(Some(20_000), 20_000, dir).is_ok());
        assert!(space_verdict(None, u64::MAX, dir).is_ok(), "a place we cannot match is let through");
    }

    #[tokio::test]
    async fn an_export_that_cannot_fit_on_the_scratch_drive_stops_before_converting() {
        let (root, files, series) = fixture("nospace", 2, 3);
        let job = folder_job(&root, &series);
        SCRATCH_FREE.with(|c| c.set(Some(100_000)));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = export_series(&files, &job, Some(tx), &CancelFlag::new()).await;
        SCRATCH_FREE.with(|c| c.set(None));
        assert!(
            matches!(result, Err(PipelineError::NoSpace { available: 100_000, .. })),
            "{result:?}"
        );
        assert!(rx.try_recv().is_err(), "nothing was started");
        assert!(folder_files(&root).is_empty());
        // And with room, the same export goes through.
        assert!(export_series(&files, &job, None, &CancelFlag::new()).await.is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn an_original_quality_export_checks_the_room_for_its_scratch_file_first() {
        let (root, files, series) = fixture("nospace-original", 2, 3);
        let mut job = folder_job(&root, &series);
        job.original = Some(crate::original::OriginalExport {
            format: crate::original::OriginalFormat::Cbz,
            rtl: false,
        });
        SCRATCH_FREE.with(|c| c.set(Some(1_000)));
        let result = export_series(&files, &job, None, &CancelFlag::new()).await;
        SCRATCH_FREE.with(|c| c.set(None));
        assert!(matches!(result, Err(PipelineError::NoSpace { available: 1_000, .. })), "{result:?}");
        assert!(folder_files(&root).is_empty());
        assert!(export_series(&files, &job, None, &CancelFlag::new()).await.is_ok());
        let _ = fs::remove_dir_all(root);
    }

    // Panics

    #[tokio::test]
    async fn a_panic_in_a_task_is_reported_as_a_bug_with_its_message() {
        let failed = tokio::task::spawn_blocking(|| {
            let n = 7;
            if n > 0 {
                panic!("boom {n}");
            }
        })
        .await
        .unwrap_err();
        let error = task_failed("converting the chapter", failed, PipelineError::Decode);
        let text = error.to_string();
        assert!(text.contains("internal error while converting the chapter"), "{text}");
        assert!(text.contains("boom 7"), "{text}");
        assert!(!text.contains("panicked"), "{text}");
        let literal = tokio::task::spawn_blocking(|| -> () { panic!("plain text") }).await.unwrap_err();
        assert!(task_failed("x", literal, PipelineError::Decode).to_string().contains("plain text"));
    }

    #[tokio::test]
    async fn a_cover_that_panics_is_left_out_and_the_next_one_is_fine() {
        let (root, files, series) = fixture("cover-panic", 1, 2);
        let opts = process_options_for(&device_profiles::x4_pro(), &series.settings);
        PANIC_IN_COVER.store(true, Ordering::SeqCst);
        let first = cover_page(&files, &series.id, "c1", &opts, false).await;
        assert!(first.is_none(), "the export goes on without a cover");
        assert!(cover_page(&files, &series.id, "c1", &opts, false).await.is_some());
        let _ = fs::remove_dir_all(root);
    }

    // The cover counts against the page limit

    #[test]
    fn the_cover_is_a_page_of_the_limit() {
        assert!(cover_fits(4, 3));
        assert!(!cover_fits(3, 3), "a chapter that fills the file leaves no room");
        assert!(!cover_fits(3, 5));
        assert!(cover_fits(60_000, 100));
    }

    #[tokio::test]
    async fn a_file_with_a_cover_stays_within_the_page_limit() {
        let (root, files, series) = fixture("cover-cap", 2, 3);
        let cover = test_png(400, 600, 9);
        let cover_path = files.cover_path(&series.id);
        fs::create_dir_all(cover_path.parent().unwrap()).unwrap();
        fs::write(&cover_path, cover).unwrap();
        let counts = |out: &ExportOutcome| -> Vec<usize> {
            out.files
                .iter()
                .map(|f| converter::xtc::XtcFile::parse(&fs::read(&f.location).unwrap()).unwrap().page_count())
                .collect()
        };
        let mut job = folder_job(&root, &series);
        job.cover = true;
        // Room for a chapter and its cover.
        job.max_pages_per_file = 4;
        let roomy = export_series(&files, &job, None, &CancelFlag::new()).await.unwrap();
        assert_eq!(counts(&roomy), [4, 4], "cover and three pages each");
        // A chapter that fills a file alone: the cover would be the fourth page of three.
        job.max_pages_per_file = 3;
        let tight = export_series(&files, &job, None, &CancelFlag::new()).await.unwrap();
        assert_eq!(counts(&tight), [3, 3], "no file goes over the limit");
        let _ = fs::remove_dir_all(root);
    }

    // Cancelling and failing part-way through a set of files

    #[tokio::test]
    async fn cancelling_after_the_first_of_two_files_leaves_the_folder_empty() {
        let (root, files, series) = fixture("cancel-set", 2, 3);
        let mut job = folder_job(&root, &series);
        job.max_pages_per_file = 3; // one file for each chapter
        CANCEL_AFTER_SENT.with(|c| c.set(Some(1)));
        let result = export_series(&files, &job, None, &CancelFlag::new()).await;
        CANCEL_AFTER_SENT.with(|c| c.set(None));
        assert!(matches!(result, Err(PipelineError::Cancelled)), "{result:?}");
        assert_eq!(folder_files(&root), Vec::<String>::new(), "no half-set is left");
        assert_eq!(
            fs::read_dir(root.join("work")).map(|d| d.count()).unwrap_or(0),
            0,
            "and no scratch files"
        );
        // Without the cancel both files arrive, so the empty folder was the cleanup.
        let whole = export_series(&files, &job, None, &CancelFlag::new()).await.unwrap();
        assert_eq!(whole.files.len(), 2);
        assert_eq!(folder_files(&root).len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_failure_after_the_first_file_names_the_files_already_sent() {
        let (root, files, series) = fixture("fail-set", 2, 3);
        let mut job = folder_job(&root, &series);
        job.max_pages_per_file = 3;
        LOSE_AFTER_SENT.with(|c| c.set(Some(1)));
        let result = export_series(&files, &job, None, &CancelFlag::new()).await;
        LOSE_AFTER_SENT.with(|c| c.set(None));
        let Err(PipelineError::Upload(message)) = result else {
            panic!("expected an upload error, got {result:?}");
        };
        let sent = folder_files(&root);
        assert_eq!(sent.len(), 1, "the first file stays: {sent:?}");
        assert!(message.contains(&sent[0]), "{message}");
        assert!(message.contains("already sent"), "{message}");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn an_already_cancelled_export_stops_before_any_book_is_written() {
        let (root, files, series) = fixture("cancelled-early", 2, 3);
        let job = folder_job(&root, &series);
        let cancel = CancelFlag::new();
        cancel.cancel();
        assert!(matches!(
            export_series(&files, &job, None, &cancel).await,
            Err(PipelineError::Cancelled)
        ));
        assert!(folder_files(&root).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    // Speed: pages encoded on several cores come out as they did one by one

    fn chapter_grays() -> Vec<GrayImage> {
        let mut seed = 3u32;
        (0..9)
            .map(|i| {
                GrayImage::from_fn(300 + i * 40, 450 + i * 90, |x, y| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let noise = (seed >> 24) as u8 / 8;
                    if (x / 20 + y / 30) % 3 == 0 { 30 + noise } else { 230 - noise }
                })
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn pages_packed_on_every_core_are_the_pages_packed_one_by_one() {
        use converter::{Depth, Layout};
        let grays = chapter_grays();
        for (depth, pictures, layout) in [
            (Depth::One, false, Layout::Manga),
            (Depth::Two, false, Layout::Manga),
            (Depth::Two, true, Layout::Manga),
            (Depth::Eight, true, Layout::Manga),
            (Depth::One, false, Layout::Webtoon),
            (Depth::Eight, true, Layout::Webtoon),
        ] {
            let mut opts = ProcessOptions::manga_default(480, 800, depth);
            opts.layout = layout;
            let serial: Vec<Vec<u8>> = converter::plan::convert_chapter(&grays, &opts)
                .iter()
                .map(|image| {
                    if pictures {
                        epub::encode_page(image).unwrap()
                    } else {
                        converter::xtc::encode_page(image).unwrap()
                    }
                })
                .collect();
            let parallel =
                pack_chapter(grays.clone(), &opts, &|_| Vec::new(), pictures, &|| false).unwrap();
            assert!(serial.len() >= grays.len().min(3));
            assert_eq!(parallel, serial, "{depth:?} pictures {pictures} {layout:?}");
        }
    }

    #[test]
    fn packing_stops_soon_after_a_cancel() {
        use std::sync::atomic::AtomicUsize;
        let grays: Vec<GrayImage> = (0..120)
            .map(|i| GrayImage::from_fn(300, 450, |x, y| ((x + y + i) % 251) as u8).unwrap())
            .collect();
        let opts = ProcessOptions::manga_default(480, 800, converter::Depth::Two);
        let asked = AtomicUsize::new(0);
        let stop = || asked.fetch_add(1, Ordering::SeqCst) >= 10;
        let result = pack_chapter(grays, &opts, &|_| Vec::new(), true, &stop);
        assert!(matches!(result, Err(PipelineError::Cancelled)));
        assert!(asked.load(Ordering::SeqCst) < 120, "stopped before the 120 pages");
    }

    /// `cargo test -p pipeline --release --features catalog/fake,converter/decode --lib -- --ignored --nocapture timing`
    #[test]
    #[ignore]
    fn timing_of_packing_a_chapter_against_rendering_first_and_encoding_after() {
        let mut seed = 5u32;
        let grays: Vec<GrayImage> = (0..80)
            .map(|_| {
                GrayImage::from_fn(900, 1400, |x, y| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let noise = (seed >> 24) as u8 / 4;
                    if (x / 40 + y / 60) % 2 == 0 { 20 + noise } else { 235 - noise }
                })
                .unwrap()
            })
            .collect();
        for (depth, pictures, name) in [
            (converter::Depth::Two, true, "EPUB/CBZ PNG"),
            (converter::Depth::Two, false, "XTCH"),
        ] {
            let opts = ProcessOptions::manga_default(480, 800, depth);
            let started = std::time::Instant::now();
            let images = converter::plan::convert_chapter(&grays, &opts);
            let old: Vec<Vec<u8>> = images
                .iter()
                .map(|i| if pictures { epub::encode_page(i).unwrap() } else { converter::xtc::encode_page(i).unwrap() })
                .collect();
            let before = started.elapsed();
            let started = std::time::Instant::now();
            let new = pack_chapter(grays.clone(), &opts, &|_| Vec::new(), pictures, &|| false).unwrap();
            let after = started.elapsed();
            assert_eq!(old, new);
            eprintln!("{name}, 80 pages: render then encode one by one {before:?}; render+encode on every core {after:?}");
        }
    }
}
