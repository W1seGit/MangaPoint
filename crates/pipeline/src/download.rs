//! Stage 1: download chapters into the library.
//!
//! Pages are saved exactly as downloaded (or shrunk, if the user chose to
//! save space) and stay on disk until the user deletes them. An interrupted
//! chapter resumes where it stopped, and a chapter that a source cannot
//! supply falls back to the next source that has it.
//!
//! Several chapters, and several pages of each, download at once, up to the
//! limits the user chose. A source that pushes back makes the whole download
//! ease off by itself (see [`crate::limiter`]). In the fastest mode every
//! source that has a chapter is asked at once and the first to finish wins.

use crate::balance::Balancer;
use crate::error::{PipelineError, Result};
use crate::fetch::{fetch_page_limited, with_retry_cancellable, RetryPolicy};
use crate::limiter::LimiterSet;
use crate::types::{CancelFlag, ChapterRef, ChapterRequest, JobEvent, SkippedChapter, SourceUse};
use catalog::{Catalog, CatalogError};
use converter::decode::{image_dimensions, shrink_page, StorageLimits};
use futures_util::stream::{self, FuturesUnordered, StreamExt};
use library::{chapter_key, DownloadLimits, DownloadedChapter, LibraryFiles, PageQuality};
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

/// What to do to a page before it is stored at each quality; `None` keeps the
/// bytes exactly as downloaded.
fn storage_limits(quality: PageQuality) -> Option<StorageLimits> {
    match quality {
        PageQuality::Ultra => Some(StorageLimits::ULTRA),
        PageQuality::Save => Some(StorageLimits::SAVE),
        PageQuality::Full => None,
    }
}
/// How often a waiting download looks at the cancel button.
const CANCEL_POLL: Duration = Duration::from_millis(60);

#[derive(Debug, Default)]
pub struct DownloadOutcome {
    /// Chapters now completely on disk, in the order requested.
    pub chapters: Vec<DownloadedChapter>,
    /// Chapters no source could supply.
    pub skipped: Vec<SkippedChapter>,
    pub source_use: Vec<SourceUse>,
}

type Events = Option<UnboundedSender<JobEvent>>;

fn send(events: &Events, event: JobEvent) {
    if let Some(tx) = events {
        let _ = tx.send(event);
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> PipelineError + '_ {
    move |e| PipelineError::Io(path.to_path_buf(), e)
}

/// What became of one requested chapter.
struct ChapterResult {
    saved: Option<DownloadedChapter>,
    skipped: Option<SkippedChapter>,
    source_use: Vec<SourceUse>,
}

/// Everything one chapter's download needs besides the chapter itself.
struct Context<'a, C> {
    catalog: &'a C,
    files: &'a LibraryFiles,
    series_id: &'a str,
    quality: PageQuality,
    policy: &'a RetryPolicy,
    limiters: &'a LimiterSet,
    balancer: &'a Balancer,
    /// Ask every source that has a chapter at once and keep the first to finish.
    race: bool,
    /// Page requests allowed at once, as chosen.
    pages_at_once: usize,
    events: &'a Events,
    cancel: &'a CancelFlag,
    /// Set when this attempt lost a race and should stop.
    stop: Option<&'a CancelFlag>,
    /// The furthest any racer for this chapter has got, so progress does not
    /// jump back and forth between racers.
    lead: Option<&'a AtomicUsize>,
}

impl<C> Clone for Context<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<C> Copy for Context<'_, C> {}

impl<C> Context<'_, C> {
    fn stopped(&self) -> bool {
        self.cancel.is_cancelled() || self.stop.is_some_and(CancelFlag::is_cancelled)
    }
}

/// Wait until the user presses cancel.
async fn cancelled(cancel: &CancelFlag) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// Download `requests` into the library under `series_id`.
///
/// Pass only chapters that are not downloaded yet. A chapter is reported
/// through [`JobEvent::ChapterSaved`] the moment it is complete, so the
/// caller can record it even if a later chapter fails or the user cancels.
/// Cancelling returns at once; pages already on disk are kept for next time.
#[allow(clippy::too_many_arguments)]
pub async fn download_chapters<C: Catalog>(
    catalog: &C,
    files: &LibraryFiles,
    series_id: &str,
    requests: &[ChapterRequest],
    quality: PageQuality,
    limits: DownloadLimits,
    policy: &RetryPolicy,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<DownloadOutcome> {
    if requests.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let pages_at_once = limits.pages.max(1);
    // When every source is asked at once, each gets its own allowance;
    // otherwise one allowance is shared by everything.
    // Unless sources are being mixed on purpose, each source works through
    // its own chapters with its own allowance, so a slow source can only slow
    // itself and never hold up a fast one.
    let grouped = !limits.race_sources && !limits.spread_sources;
    let limiters = LimiterSet::new(pages_at_once, grouped || limits.race_sources);
    let balancer = Balancer::new(limits.spread_sources && !limits.race_sources);
    let cx = Context {
        catalog,
        files,
        series_id,
        quality,
        policy,
        limiters: &limiters,
        balancer: &balancer,
        race: limits.race_sources,
        pages_at_once,
        events: &events,
        cancel,
        stop: None,
        lead: None,
    };
    let total = requests.len();
    let cx = &cx;

    let chapters_at_once = limits.chapters.max(1);
    let work = run_chapters(cx, requests, total, chapters_at_once, grouped);
    // Dropping the work on cancel stops every request in flight right away.
    let mut results = tokio::select! {
        done = work => done,
        () = cancelled(cancel) => return Err(PipelineError::Cancelled),
    };
    results.sort_by_key(|(index, _)| *index);

    let mut outcome = DownloadOutcome::default();
    for (_, result) in results {
        let result = result?; // only a cancellation comes back as an error
        outcome.source_use.extend(result.source_use);
        outcome.chapters.extend(result.saved);
        outcome.skipped.extend(result.skipped);
    }
    if outcome.chapters.is_empty() {
        return Err(PipelineError::NothingDownloaded);
    }
    Ok(outcome)
}

/// Work through the chapters. Grouped, every source has its own line of
/// chapters (those it is the first choice for), `chapters_at_once` at a
/// time, and the lines run side by side. Otherwise there is one line.
fn run_chapters<'a, C: Catalog>(
    cx: &'a Context<'a, C>,
    requests: &'a [ChapterRequest],
    total: usize,
    chapters_at_once: usize,
    grouped: bool,
) -> impl Future<Output = Vec<(usize, Result<ChapterResult>)>> + 'a {
    let mut lines: Vec<Vec<(usize, &'a ChapterRequest)>> = Vec::new();
    if grouped {
        let mut sources: Vec<&str> = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            let source = request.options.first().map_or("", |o| o.source_id.as_str());
            match sources.iter().position(|s| *s == source) {
                Some(line) => lines[line].push((index, request)),
                None => {
                    sources.push(source);
                    lines.push(vec![(index, request)]);
                }
            }
        }
    } else {
        lines.push(requests.iter().enumerate().collect());
    }
    let streams = lines
        .into_iter()
        .map(|line| {
            stream::iter(line)
                .map(move |(index, request)| async move {
                    (index, download_request(cx, index, total, request).await)
                })
                .buffer_unordered(chapters_at_once)
                .boxed_local()
        })
        .collect::<Vec<_>>();
    stream::select_all(streams).collect::<Vec<_>>()
}

/// One requested chapter: get it from a source, trying others if it fails.
/// Fails only when the job is cancelled; a chapter nobody can supply is
/// reported as skipped.
async fn download_request<C: Catalog>(
    cx: &Context<'_, C>,
    index: usize,
    total: usize,
    request: &ChapterRequest,
) -> Result<ChapterResult> {
    if cx.stopped() {
        return Err(PipelineError::Cancelled);
    }
    send(
        cx.events,
        JobEvent::ChapterStarted {
            index,
            total,
            name: request.name.clone(),
        },
    );
    // Named after the first (best) source, whichever one ends up supplying it.
    let key = request.key.clone().unwrap_or_else(|| {
        chapter_key(
            request.number,
            request.options.first().map_or(0, |o| o.chapter_id),
        )
    });

    let (saved, source_use, last_reason) = if cx.race && request.options.len() > 1 {
        race_sources(cx, &key, &request.options, index).await?
    } else {
        try_in_turn(cx, &key, &request.options, index).await?
    };

    let result = match saved {
        Some((option, (pages, bytes))) => {
            let chapter = DownloadedChapter {
                key,
                number: request.number,
                name: request.name.clone(),
                source_id: option.source_id.clone(),
                chapter_id: option.chapter_id,
                pages,
                bytes,
                downloaded_at: now_secs(),
            };
            send(
                cx.events,
                JobEvent::ChapterSaved {
                    chapter: chapter.clone(),
                },
            );
            ChapterResult {
                saved: Some(chapter),
                skipped: None,
                source_use,
            }
        }
        None => {
            send(
                cx.events,
                JobEvent::ChapterSkipped {
                    chapter_index: index,
                    name: request.name.clone(),
                    reason: last_reason.clone(),
                },
            );
            ChapterResult {
                saved: None,
                skipped: Some(SkippedChapter {
                    index,
                    name: request.name.clone(),
                    reason: last_reason,
                }),
                source_use,
            }
        }
    };
    send(cx.events, JobEvent::ChapterDone { index });
    Ok(result)
}

type Supplied<'a> = (
    Option<(&'a ChapterRef, (usize, u64))>,
    Vec<SourceUse>,
    String,
);

/// Try the sources one after another, the least busy good one first, until
/// one supplies the chapter.
async fn try_in_turn<'a, C: Catalog>(
    cx: &Context<'_, C>,
    key: &str,
    options: &'a [ChapterRef],
    index: usize,
) -> Result<Supplied<'a>> {
    let mut source_use = Vec::new();
    let mut last_reason = String::from("no source offers this chapter");
    let order = cx.balancer.order(options);
    for (n, option) in order.iter().copied().enumerate() {
        let started = Instant::now();
        cx.balancer.begin(&option.source_id);
        let attempt = download_one(cx, key, option, index).await;
        let pages = attempt.as_ref().map_or(0, |(pages, _)| *pages);
        cx.balancer
            .finish(&option.source_id, pages, started.elapsed().as_secs_f64());
        source_use.push(SourceUse {
            source_id: option.source_id.clone(),
            ok: attempt.is_ok(),
            ms: started.elapsed().as_millis() as u64,
        });
        match attempt {
            Ok(done) => return Ok((Some((option, done)), source_use, last_reason)),
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(e) => {
                last_reason = e.to_string();
                if let Some(next) = order.get(n + 1) {
                    send(
                        cx.events,
                        JobEvent::UsedFallback {
                            chapter_index: index,
                            from: option.source_id.clone(),
                            to: next.source_id.clone(),
                            reason: last_reason.clone(),
                        },
                    );
                }
            }
        }
    }
    Ok((None, source_use, last_reason))
}

/// Ask every source at once. The first to finish wins; the others are
/// stopped and their partial pages thrown away. Each source downloads into
/// its own folder, so they never mix pages.
async fn race_sources<'a, C: Catalog>(
    cx: &Context<'_, C>,
    key: &str,
    options: &'a [ChapterRef],
    index: usize,
) -> Result<Supplied<'a>> {
    let stop = CancelFlag::new();
    let lead = AtomicUsize::new(0);
    let racing = Context {
        stop: Some(&stop),
        lead: Some(&lead),
        ..*cx
    };
    let stage_of = |option: &ChapterRef| format!("{key}-via-{}", option.source_id);

    let mut pending: FuturesUnordered<_> = options
        .iter()
        .map(|option| {
            let stage = stage_of(option);
            async move {
                let started = Instant::now();
                let result = download_one(&racing, &stage, option, index).await;
                (option, result, started.elapsed().as_millis() as u64)
            }
        })
        .collect();

    let mut winner: Option<(&ChapterRef, (usize, u64))> = None;
    let mut source_use = Vec::new();
    let mut last_reason = String::from("no source offers this chapter");
    while let Some((option, result, ms)) = pending.next().await {
        match result {
            Ok(done) if winner.is_none() => {
                stop.cancel();
                source_use.push(SourceUse {
                    source_id: option.source_id.clone(),
                    ok: true,
                    ms,
                });
                winner = Some((option, done));
            }
            // A second source that finished in the same instant, or one that
            // was stopped (by the user or by losing): neither says anything
            // bad about the source.
            Ok(_) | Err(PipelineError::Cancelled) => {}
            Err(e) => {
                last_reason = e.to_string();
                source_use.push(SourceUse {
                    source_id: option.source_id.clone(),
                    ok: false,
                    ms,
                });
            }
        }
    }
    drop(pending);

    let finish = |winner_stage: Option<&str>| {
        for option in options {
            let stage = stage_of(option);
            if winner_stage != Some(stage.as_str()) {
                let _ = cx.files.remove_chapter(cx.series_id, &stage);
            }
        }
    };
    if cx.cancel.is_cancelled() {
        finish(None);
        return Err(PipelineError::Cancelled);
    }
    let Some((option, _)) = winner else {
        finish(None);
        return Ok((None, source_use, last_reason));
    };
    // Move the winner's pages into the chapter's own folder.
    let stage = stage_of(option);
    finish(Some(&stage));
    let (from, to) = (
        cx.files.chapter_dir(cx.series_id, &stage),
        cx.files.chapter_dir(cx.series_id, key),
    );
    cx.files
        .remove_chapter(cx.series_id, key)
        .map_err(io_err(&to))?;
    fs::rename(&from, &to).map_err(io_err(&to))?;
    let pages = cx.files.page_files(cx.series_id, key).len();
    let bytes = cx.files.chapter_bytes(cx.series_id, key);
    Ok((Some((option, (pages, bytes))), source_use, last_reason))
}

/// Download one source's copy of a chapter into the folder named `dir_key`.
/// Returns the page count and the bytes on disk.
async fn download_one<C: Catalog>(
    cx: &Context<'_, C>,
    dir_key: &str,
    option: &ChapterRef,
    chapter_index: usize,
) -> Result<(usize, u64)> {
    let (files, series_id, key) = (cx.files, cx.series_id, dir_key);
    let limiter = cx.limiters.get(&option.source_id);
    let limiter = &*limiter;
    let breaker = cx.limiters.breaker(&option.source_id);
    let breaker = &*breaker;
    if breaker.is_open() {
        return Err(crate::limiter::Breaker::error().into());
    }
    let dir = files.chapter_dir(series_id, key);
    let marker = dir.join("source.txt");
    let wanted = format!("{}:{}", option.source_id, option.chapter_id);
    if fs::read_to_string(&marker).ok().as_deref() != Some(wanted.as_str()) {
        // Pages from a different copy of the chapter would not line up with
        // this one, so start clean.
        files.remove_chapter(series_id, key).map_err(io_err(&dir))?;
    }
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    fs::write(&marker, &wanted).map_err(io_err(&marker))?;

    let paths = with_retry_cancellable(cx.policy, cx.cancel, || {
        cx.catalog.chapter_pages(option.chapter_id)
    })
    .await
    .inspect_err(|e| {
        if crate::fetch::is_retryable(e) {
            breaker.failed();
        }
    })?;
    if paths.is_empty() {
        return Err(
            CatalogError::BadResponse("the source lists no pages for this chapter".into()).into(),
        );
    }
    let total = paths.len();
    let done = AtomicUsize::new(0);
    let done = &done;
    let on_limit = |now: usize| {
        send(
            cx.events,
            JobEvent::PagesAtOnce {
                now,
                chosen: cx.pages_at_once,
            },
        )
    };
    let on_limit = &on_limit;

    let results: Vec<Result<()>> = stream::iter(paths.iter().enumerate())
        .map(|(index, path)| async move {
            if cx.stopped() {
                return Err(PipelineError::Cancelled);
            }
            let dest = files.page_path(series_id, key, index);
            let have = fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false);
            if !have {
                let bytes = fetch_page_limited(
                    cx.catalog, path, cx.policy, limiter, breaker, cx.cancel, on_limit,
                )
                .await
                .map_err(|e| {
                    if cx.stopped() {
                        PipelineError::Cancelled
                    } else {
                        e.into()
                    }
                })?;
                if cx.stopped() {
                    return Err(PipelineError::Cancelled);
                }
                // An error page or a blocked response is not a manga page.
                image_dimensions(&bytes).map_err(|e| {
                    PipelineError::Decode(format!("page {} is not an image ({e})", index + 1))
                })?;
                let bytes = if let Some(limits) = storage_limits(cx.quality) {
                    tokio::task::spawn_blocking(move || match shrink_page(&bytes, &limits) {
                        Ok(Some(small)) => Ok(small),
                        Ok(None) => Ok(bytes),
                        Err(e) => Err(PipelineError::Decode(e.to_string())),
                    })
                    .await
                    .map_err(|e| PipelineError::Decode(e.to_string()))??
                } else {
                    bytes
                };
                write_atomically(&dest, &bytes)?;
            }
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            // With several sources racing, report only the one in front.
            let report = cx
                .lead
                .is_none_or(|lead| lead.fetch_max(n, Ordering::SeqCst) < n);
            if report {
                send(
                    cx.events,
                    JobEvent::PageDone {
                        chapter_index,
                        done: n,
                        total,
                    },
                );
            }
            Ok(())
        })
        .buffer_unordered(cx.pages_at_once)
        .collect()
        .await;
    for r in results {
        r?;
    }

    let saved = files.page_files(series_id, key).len();
    if saved != total {
        return Err(PipelineError::Decode(format!(
            "only {saved} of {total} pages were saved"
        )));
    }
    Ok((total, files.chapter_bytes(series_id, key)))
}

/// Write to a temporary name, then rename, so a crash never leaves a
/// half-written page that looks complete.
fn write_atomically(dest: &Path, bytes: &[u8]) -> Result<()> {
    let tmp: PathBuf = dest.with_extension("part");
    fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    fs::rename(&tmp, dest).map_err(io_err(dest))?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
