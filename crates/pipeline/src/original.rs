//! Exporting the pages as they were saved, for a phone or a tablet.
//!
//! No e-ink processing at all: no gray, no dithering, no splitting and no zoom
//! tiles. Each page of the book is the saved page itself, in color. The one
//! change: the pages of a long strip (a webtoon) are made one width, so a
//! reader shows them as one column (see `formats::original::strip_width`). A CBZ keeps every JPEG, PNG and WebP byte for byte; a PDF embeds a
//! JPEG as it is and writes other pictures without loss. The books are built the
//! same way as the others (see [`crate::compose`]): in a scratch file as the
//! pages arrive, grouped as asked, then sent to a folder, a drive or the
//! reader's Wi-Fi share, with the free-space check, the cancel flag and the
//! temporary-name-then-rename write that every export has.

use crate::cbz::{CbzBuilder, CbzMeta};
use crate::compose::{
    available_space, group_ranges, now_secs, remove_sent, space_verdict, unique_name, volume_name,
    ExportJob, ExportOutcome, Scratch, Volume,
};
use crate::error::{PipelineError, Result};
use crate::export::{ExportedFile, WIFI_MAX_FILE_BYTES};
use crate::pdf::{PdfBuilder, PdfMeta};
use formats::original::{add_to_cbz, add_to_pdf, fit_width, strip_width};
use crate::types::{CancelFlag, JobEvent};
use library::{DownloadedChapter, LibraryFiles};
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

/// What the pages are put into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OriginalFormat {
    Cbz,
    Pdf,
}

impl OriginalFormat {
    pub const ALL: [OriginalFormat; 2] = [OriginalFormat::Cbz, OriginalFormat::Pdf];

    pub fn extension(self) -> &'static str {
        match self {
            OriginalFormat::Cbz => "cbz",
            OriginalFormat::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OriginalFormat::Cbz => "CBZ",
            OriginalFormat::Pdf => "PDF",
        }
    }
}

/// An export of the saved pages as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalExport {
    pub format: OriginalFormat,
    /// Read right to left (manga): written into the file for readers that care.
    pub rtl: bool,
}

/// A book being built.
enum Book {
    Cbz(Box<CbzBuilder>),
    Pdf(Box<PdfBuilder>),
}

impl Book {
    fn page_count(&self) -> usize {
        match self {
            Book::Cbz(b) => b.page_count(),
            Book::Pdf(b) => b.page_count(),
        }
    }

    fn chapter(&mut self, name: &str, start: usize, number: Option<f64>) {
        match self {
            Book::Cbz(b) => b.push_chapter(name, start, number),
            Book::Pdf(b) => b.push_chapter(name, start),
        }
    }

    fn finish(self) -> Result<()> {
        match self {
            Book::Cbz(b) => (*b).finish(),
            Book::Pdf(b) => (*b).finish(),
        }
    }
}

fn bad_page(what: impl std::fmt::Display) -> PipelineError {
    PipelineError::Decode(what.to_string())
}

/// Put one saved page into the book, scaled to `width` first when the chapter is a long strip.
fn add_page(book: &mut Book, bytes: &[u8], width: Option<u32>) -> Result<()> {
    let fitted = match width {
        Some(target) => fit_width(bytes, target)?,
        None => None,
    };
    let bytes = fitted.as_deref().unwrap_or(bytes);
    match book {
        Book::Cbz(b) => {
            let (book, path) = b.parts();
            add_to_cbz(book, bytes).map_err(|e| PipelineError::from_format(path, e))
        }
        Book::Pdf(b) => {
            let (book, path) = b.parts();
            add_to_pdf(book, bytes).map_err(|e| PipelineError::from_format(path, e))
        }
    }
}

fn open_book(job: &ExportJob, original: &OriginalExport, spool: &std::path::Path) -> Result<Book> {
    let info = job.series.info.as_ref();
    Ok(match original.format {
        OriginalFormat::Cbz => Book::Cbz(Box::new(CbzBuilder::create(
            spool,
            CbzMeta {
                series: job.series.title.clone(),
                language: job.language.clone(),
                rtl: original.rtl,
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
        )?)),
        OriginalFormat::Pdf => Book::Pdf(Box::new(PdfBuilder::create(
            spool,
            PdfMeta {
                title: job.series.title.clone(),
                author: info
                    .map(|i| if i.author.trim().is_empty() { i.artist.clone() } else { i.author.clone() })
                    .unwrap_or_default(),
                rtl: original.rtl,
                created: now_secs(),
            },
        )?)),
    })
}

/// Run slow file work on a thread of its own.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| bad_page(format!("the export was interrupted: {e}")))?
}

/// The pages of each chosen chapter, and their sizes on disk, in reading order.
struct Source {
    chapter: DownloadedChapter,
    pages: Vec<PathBuf>,
    bytes: u64,
}

/// Build the books from the saved pages and send them. See the module docs.
pub async fn export_original(
    files: &LibraryFiles,
    job: &ExportJob,
    original: &OriginalExport,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<ExportOutcome> {
    let chosen: Vec<DownloadedChapter> = job
        .series
        .chapters
        .iter()
        .filter(|c| job.chapter_keys.contains(&c.key))
        .cloned()
        .collect();
    if chosen.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let emit = |e: JobEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(e);
        }
    };
    fs::create_dir_all(&job.work_dir).map_err(|e| PipelineError::Io(job.work_dir.clone(), e))?;

    let (list_files, series_id) = (files.clone(), job.series.id.clone());
    let sources: Vec<Source> = blocking(move || {
        Ok(chosen
            .into_iter()
            .map(|chapter| {
                let pages = list_files.page_files(&series_id, &chapter.key);
                let bytes = pages
                    .iter()
                    .filter_map(|p| fs::metadata(p).ok())
                    .map(|m| m.len())
                    .sum();
                Source { chapter, pages, bytes }
            })
            .collect())
    })
    .await?;

    // Fail now, not after an hour of copying, if the scratch drive is too full:
    // the books are as big as the pages (a PDF of PNG pages a little bigger).
    let source_bytes: u64 = sources.iter().map(|s| s.bytes).sum();
    let needed = source_bytes + source_bytes / 10;
    space_verdict(available_space(&job.work_dir), needed, &job.work_dir)?;

    // A book sent over Wi-Fi is one request, so it is kept under what the reader takes.
    let size_cap = if matches!(job.target, crate::export::ExportTarget::WebDav { .. }) {
        WIFI_MAX_FILE_BYTES
    } else {
        u64::MAX
    };
    let extension = original.format.extension();
    let total = sources.len();
    let mut scratch = Scratch::default();
    let mut volumes: Vec<Volume> = Vec::new();
    let mut total_pages = 0usize;

    for (group, range) in group_ranges(total, job.grouping).into_iter().enumerate() {
        let mut open: Option<(Book, Volume, u64)> = None;
        for ci in range {
            cancel.wait_while_paused().await;
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let source = &sources[ci];
            if source.pages.is_empty() {
                return Err(bad_page(format!("{} has no saved pages", source.chapter.name)));
            }
            // Start a new file if this chapter would not fit in the open one.
            if open.as_ref().is_some_and(|(_, _, bytes)| *bytes > 0 && bytes + source.bytes > size_cap) {
                let (book, volume, _) = open.take().expect("checked above");
                volumes.push(finish_volume(book, volume, cancel).await?);
            }
            if open.is_none() {
                let spool = job.work_dir.join(unique_name("spool"));
                scratch.0.push(spool.clone());
                let book = open_book(job, original, &spool)?;
                open = Some((
                    book,
                    Volume {
                        temp: spool,
                        group,
                        chapter_names: Vec::new(),
                        first_number: None,
                        last_number: None,
                    },
                    0,
                ));
            }
            emit(JobEvent::ChapterStarted {
                index: ci,
                total,
                name: source.chapter.name.clone(),
            });
            let (mut book, mut volume, mut bytes) = open.take().expect("opened above");
            let start = book.page_count();
            let (pages, name, number) = (source.pages.clone(), source.chapter.name.clone(), source.chapter.number);
            let (flag, tx) = (cancel.clone(), events.clone());
            let page_total = pages.len();
            // A long strip is made one width (see `strip_width`).
            let sizes: Vec<(u32, u32)> = pages
                .iter()
                .filter_map(|p| image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.into_dimensions().ok())
                .collect();
            let width = strip_width(&sizes);
            // The pages go into the book on a thread of their own, one at a time,
            // looking at the cancel flag before each.
            let (book, added) = tokio::task::spawn_blocking(move || {
                let mut added = 0usize;
                for (i, path) in pages.iter().enumerate() {
                    if flag.is_cancelled() {
                        return (book, Err(PipelineError::Cancelled));
                    }
                    let one = fs::read(path)
                        .map_err(|e| PipelineError::Io(path.clone(), e))
                        .and_then(|data| {
                            add_page(&mut book, &data, width)
                                .map_err(|e| match e {
                                    PipelineError::Decode(m) => bad_page(format!("{name}, page {}: {m}", i + 1)),
                                    other => other,
                                })
                                .map(|()| data.len() as u64)
                        });
                    match one {
                        Ok(len) => added += len as usize,
                        Err(e) => return (book, Err(e)),
                    }
                    if let Some(tx) = &tx {
                        let _ = tx.send(JobEvent::PageDone {
                            chapter_index: ci,
                            done: i + 1,
                            total: page_total,
                        });
                    }
                }
                book.chapter(&name, start, number);
                (book, Ok(added as u64))
            })
            .await
            .map_err(|e| bad_page(format!("the export was interrupted: {e}")))?;
            bytes += added?;
            volume.chapter_names.push(source.chapter.name.clone());
            if volume.first_number.is_none() {
                volume.first_number = source.chapter.number;
            }
            if source.chapter.number.is_some() {
                volume.last_number = source.chapter.number;
            }
            total_pages += source.pages.len();
            open = Some((book, volume, bytes));
            emit(JobEvent::ChapterDone { index: ci });
        }
        if let Some((book, volume, _)) = open.take() {
            volumes.push(finish_volume(book, volume, cancel).await?);
        }
    }

    // Now that every file exists, name them (part numbers need the count) and send them.
    let mut sent: Vec<ExportedFile> = Vec::new();
    let sizes: Vec<u64> = volumes
        .iter()
        .map(|v| fs::metadata(&v.temp).map_or(0, |m| m.len()))
        .collect();
    let all: u64 = sizes.iter().sum();
    let mut before = 0u64;
    for (i, volume) in volumes.iter().enumerate() {
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
        cached_chapters: 0,
    })
}

/// Close a book. It can be gigabytes, so this is done on a thread of its own.
async fn finish_volume(book: Book, volume: Volume, cancel: &CancelFlag) -> Result<Volume> {
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let flag = cancel.clone();
    blocking(move || {
        if flag.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        book.finish()?;
        Ok(volume)
    })
    .await
}
