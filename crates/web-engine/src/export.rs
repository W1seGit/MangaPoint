//! Converting chapters into reader files in memory, with the desktop app's own
//! conversion and writers.
//!
//! An [`Exporter`] is fed one chapter at a time ([`Exporter::add_chapter`]) with
//! the stored bytes of the chapter's pictures, converts it (decode, prepare,
//! plan, render, encode: the very functions the desktop export calls), appends it
//! to the file being built and drops what it no longer needs. [`Exporter::finish`]
//! closes the books; [`Exporter::take_file`] hands each finished file over once.
//!
//! The files are the same bytes the desktop app makes for the same pages and
//! options (`tests/parity.rs` runs both and compares). The only inputs the
//! desktop gets from elsewhere are given in the options: the title, the details
//! of the series, the time to write into the files (`created`) and whether files
//! open with a cover page.
//!
//! Nothing is global: dropping an `Exporter` (or the worker that holds it) is how
//! an export is cancelled.
//!
//! ## Memory
//!
//! WebAssembly has 4 GB at most, and the visitor's phone far less. The exporter
//! refuses, with a sentence that suggests one file per chapter, when
//!
//! * what a chapter holds decoded (about a byte a pixel, three for colour, a
//!   quarter more while it is cropped or scaled) would pass
//!   [`Limits::max_chapter_bytes`], or
//! * the files made so far, plus the chapter just converted, would pass
//!   [`Limits::max_output_bytes`] (an XTC or XTCH book counts twice, because its
//!   pages are copied once more when the file is assembled).
//!
//! Both are about 1.2 GB unless [`Exporter::with_limits`] says otherwise. A chapter
//! that is refused changes nothing: the exporter can go on with smaller ones.

mod cbz;
mod epub;
mod original;
mod pdf;

use self::cbz::{CbzBuilder, CbzMeta};
use self::epub::{EpubBuilder, EpubMeta};
use self::pdf::{PdfBuilder, PdfMeta};
use crate::options::{ExportOptions, FileType};
use converter::xtc::{Chapter as XtcChapter, Format, Metadata, XtcBuilder};
use converter::{Depth, GrayImage, ProcessOptions};
use device_profiles::DeviceProfile;
use library::ConversionSettings;

/// The most pages an XTC or XTCH book can hold.
const XTC_MAX_PAGES: usize = u16::MAX as usize;

/// What an export may hold in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most the files made so far (and the chapter being added) may add up to.
    pub max_output_bytes: u64,
    /// The most a chapter may take decoded.
    pub max_chapter_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_output_bytes: 1_200_000_000, max_chapter_bytes: 1_200_000_000 }
    }
}

/// A finished file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

enum Book {
    Xtc(XtcBuilder),
    Epub(Box<EpubBuilder>),
    Cbz(Box<CbzBuilder>),
    Pdf(Box<PdfBuilder>),
}

impl Book {
    fn page_count(&self) -> usize {
        match self {
            Book::Xtc(b) => b.page_count(),
            Book::Epub(b) => b.page_count(),
            Book::Cbz(b) => b.page_count(),
            Book::Pdf(b) => b.page_count(),
        }
    }

    /// Add a page made by this kind of book's own encoder.
    fn add(&mut self, page: &[u8]) -> Result<(), String> {
        match self {
            Book::Xtc(b) => b.add_encoded_page(page).map_err(|e| e.to_string()),
            Book::Epub(b) => b.add_page(page),
            Book::Cbz(b) => b.add_page(page),
            Book::Pdf(_) => Err("a PDF takes pictures as they are".into()),
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
            Book::Pdf(b) => b.push_chapter(name, start),
        }
    }

    fn finish(self) -> Result<Vec<u8>, String> {
        match self {
            Book::Xtc(b) => b.build().map_err(|e| e.to_string()),
            Book::Epub(b) => (*b).finish(),
            Book::Cbz(b) => (*b).finish(),
            Book::Pdf(b) => (*b).finish(),
        }
    }
}

/// What a file holds, for naming it.
struct Volume {
    /// Which group of chapters (see [`crate::options::Grouping`]) it belongs to.
    group: usize,
    chapter_names: Vec<String>,
    first_number: Option<f64>,
    last_number: Option<f64>,
}

struct Open {
    book: Book,
    volume: Volume,
    /// Bytes the book holds so far.
    bytes: u64,
}

struct Done {
    volume: Volume,
    bytes: Vec<u8>,
}

/// An export in progress. See the module documentation.
pub struct Exporter {
    options: ExportOptions,
    device: DeviceProfile,
    kind: FileType,
    process: ProcessOptions,
    /// Pages are made as pictures (EPUB or CBZ) rather than packed (XTC, XTCH).
    pictures: bool,
    /// Pages are kept as they are (the phone entry).
    original: bool,
    rtl: bool,
    page_cap: usize,
    limits: Limits,
    open: Option<Open>,
    done: Vec<Done>,
    /// Bytes in `done`.
    held: u64,
    chapters_added: usize,
    pages_made: usize,
    /// The cover page, made from the first page of the first chapter.
    cover: Option<Vec<u8>>,
    cover_tried: bool,
    files: Vec<Option<OutputFile>>,
    closed: bool,
    failed: bool,
}

impl Exporter {
    /// Start an export. `options_json` is described in [`crate::options`]; an
    /// empty text means the defaults.
    pub fn new(options_json: &str) -> Result<Self, String> {
        Self::with_options(ExportOptions::from_json(options_json)?)
    }

    pub fn with_options(options: ExportOptions) -> Result<Self, String> {
        let device = options.resolve_device()?;
        let kind = options.file_type();
        if options.format == Some(FileType::Pdf) && kind != FileType::Pdf {
            return Err("A PDF can only be made with the \"Phone or tablet\" entry.".into());
        }
        let original = device_profiles::is_original(&device.id);
        let settings: &ConversionSettings = &options.settings;
        let pictures = matches!(kind, FileType::Epub | FileType::Cbz);
        let process = process_options_for(&device, settings, kind);
        let mut page_cap = options.max_pages_per_file.max(1);
        // An EPUB is cut into parts a reader can load; a CBZ has no such limit.
        if kind == FileType::Epub {
            page_cap = page_cap.min(epub::MAX_PAGES);
        }
        if matches!(kind, FileType::Xtc | FileType::Xtch) {
            page_cap = page_cap.min(XTC_MAX_PAGES);
        }
        let rtl = settings.direction == converter::xtc::ReadDirection::RightToLeft;
        Ok(Self {
            options,
            device,
            kind,
            process,
            pictures,
            original,
            rtl,
            page_cap,
            limits: Limits::default(),
            open: None,
            done: Vec::new(),
            held: 0,
            chapters_added: 0,
            pages_made: 0,
            cover: None,
            cover_tried: false,
            files: Vec::new(),
            closed: false,
            failed: false,
        })
    }

    /// Use other memory limits (tests, or a worker that knows it has less).
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// The device pages are made for.
    pub fn device(&self) -> &DeviceProfile {
        &self.device
    }

    /// The file type that is made.
    pub fn file_type(&self) -> FileType {
        self.kind
    }

    /// How many chapters have been added.
    pub fn chapter_count(&self) -> usize {
        self.chapters_added
    }

    /// How many pages the added chapters made.
    pub fn page_count(&self) -> usize {
        self.pages_made
    }

    /// Convert one chapter and add it to the file being built. `pages` are the
    /// stored bytes of the chapter's pictures in reading order. Returns how many
    /// pages the chapter made (a double page can make two, a webtoon strip many).
    ///
    /// A chapter that is refused for its size, or one with a page that cannot be
    /// read, leaves the exporter as it was. If writing a file itself fails the
    /// exporter is spoiled and says so on every later call.
    pub fn add_chapter<P: AsRef<[u8]>>(
        &mut self,
        name: &str,
        number: Option<f64>,
        pages: &[P],
    ) -> Result<usize, String> {
        if self.closed {
            return Err("The export is finished; start a new one to convert more.".into());
        }
        if self.failed {
            return Err("The export stopped after an error; start it again.".into());
        }
        if pages.is_empty() {
            return Err(format!("{name} has no pages."));
        }
        let number = number.filter(|n| n.is_finite());
        let group = self.options.grouping.group_of(self.chapters_added);
        let input_bytes: u64 = pages.iter().map(|p| p.as_ref().len() as u64).sum();

        let made = if self.original {
            // The pages go in as they are: the output is about as big as the input.
            self.check_output(input_bytes, name)?;
            None
        } else {
            let color = self.pictures && self.process.color;
            let decoded = decoded_bytes(pages, color);
            if decoded > self.limits.max_chapter_bytes {
                return Err(format!(
                    "{name} is too big to convert in the browser (about {} MB once unpacked; the limit is {} MB). Export one file per chapter, or fewer chapters at a time.",
                    decoded >> 20,
                    self.limits.max_chapter_bytes >> 20
                ));
            }
            let images = convert_chapter(name, pages, &self.process, self.pictures)?;
            let bytes: u64 = images.iter().map(|i| i.len() as u64).sum();
            self.check_output(bytes, name)?;
            Some(images)
        };

        // From here the exporter changes; a failure spoils it.
        let result = self.append(group, name, number, pages, made.as_deref());
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn weight(&self) -> u64 {
        // An XTC book copies its pages once more when it is assembled.
        if matches!(self.kind, FileType::Xtc | FileType::Xtch) {
            2
        } else {
            1
        }
    }

    fn check_output(&self, new_bytes: u64, name: &str) -> Result<(), String> {
        let open = self.open.as_ref().map_or(0, |o| o.bytes);
        let total = (self.held + open + new_bytes).saturating_mul(self.weight());
        if total > self.limits.max_output_bytes {
            return Err(format!(
                "Adding {name} would make the files about {} MB, more than the browser can build at once (the limit is {} MB). Export one file per chapter, or fewer chapters at a time.",
                total >> 20,
                self.limits.max_output_bytes >> 20
            ));
        }
        Ok(())
    }

    fn append<P: AsRef<[u8]>>(
        &mut self,
        group: usize,
        name: &str,
        number: Option<f64>,
        pages: &[P],
        made: Option<&[Vec<u8>]>,
    ) -> Result<usize, String> {
        let made_len = made.map_or(pages.len(), <[Vec<u8>]>::len);
        // A new group starts a new file.
        if self.open.as_ref().is_some_and(|o| o.volume.group != group) {
            self.close_open()?;
        }
        // Start a new file if this chapter would not fit in the open one.
        if !self.original && self.open.as_ref().is_some_and(|o| o.book.page_count() + made_len > self.page_cap) {
            self.close_open()?;
        }
        if self.open.is_none() {
            self.open = Some(Open {
                book: self.new_book()?,
                volume: Volume { group, chapter_names: Vec::new(), first_number: None, last_number: None },
                bytes: 0,
            });
        }
        // The cover is made from the first page of the first chapter.
        if self.options.cover && !self.original && !self.cover_tried {
            self.cover_tried = true;
            self.cover = make_cover(pages[0].as_ref(), &self.process, self.pictures);
        }
        let cover_fits = made_len < self.page_cap;
        let open = self.open.as_mut().ok_or("the book is not open")?;
        if open.book.page_count() == 0 {
            if let Some(cover) = self.cover.as_ref().filter(|_| cover_fits) {
                open.book.add(cover)?;
                open.bytes += cover.len() as u64;
            }
        }
        let start = open.book.page_count();
        match made {
            Some(images) => {
                for image in images {
                    open.book.add(image)?;
                    open.bytes += image.len() as u64;
                }
            }
            None => {
                for (i, page) in pages.iter().enumerate() {
                    let bytes = page.as_ref();
                    let added = match &mut open.book {
                        Book::Cbz(b) => original::add_to_cbz(b, bytes),
                        Book::Pdf(b) => original::add_to_pdf(b, bytes),
                        _ => Err("this kind of file cannot keep pages as they are".into()),
                    };
                    added.map_err(|e| format!("{name}, page {}: {e}", i + 1))?;
                    open.bytes += bytes.len() as u64;
                }
            }
        }
        open.book.chapter(name, start, number);
        open.volume.chapter_names.push(name.to_string());
        if open.volume.first_number.is_none() {
            open.volume.first_number = number;
        }
        if number.is_some() {
            open.volume.last_number = number;
        }
        self.chapters_added += 1;
        self.pages_made += made_len;
        Ok(made_len)
    }

    fn new_book(&self) -> Result<Book, String> {
        let o = &self.options;
        let info = &o.info;
        let title = o.title.clone();
        Ok(match self.kind {
            FileType::Xtc | FileType::Xtch => {
                let format = if self.kind == FileType::Xtc { Format::Xtc } else { Format::Xtch };
                Book::Xtc(
                    XtcBuilder::new(format)
                        .direction(o.settings.direction)
                        .metadata(Metadata {
                            title,
                            language: o.language.clone(),
                            create_time: o.created as u32,
                            cover_page: Some(0),
                            ..Default::default()
                        }),
                )
            }
            FileType::Epub => Book::Epub(Box::new(EpubBuilder::new(EpubMeta {
                title,
                language: o.language.clone(),
                rtl: self.rtl,
                width: self.process.width,
                height: self.process.height,
                created: o.created,
            })?)),
            FileType::Cbz => Book::Cbz(Box::new(CbzBuilder::new(CbzMeta {
                series: title,
                language: o.language.clone(),
                rtl: self.rtl,
                summary: info.description.clone(),
                author: info.author.clone(),
                artist: info.artist.clone(),
                genres: info.genres.clone(),
                status: info.status.clone(),
                sources: info.sources.clone(),
                created: o.created,
            }))),
            FileType::Pdf => Book::Pdf(Box::new(PdfBuilder::new(PdfMeta {
                title,
                author: if info.author.trim().is_empty() { info.artist.clone() } else { info.author.clone() },
                rtl: self.rtl,
                created: o.created,
            })?)),
        })
    }

    fn close_open(&mut self) -> Result<(), String> {
        if let Some(Open { book, volume, .. }) = self.open.take() {
            let bytes = book.finish()?;
            self.held += bytes.len() as u64;
            self.done.push(Done { volume, bytes });
        }
        Ok(())
    }

    /// Close the last file and name all of them. After this,
    /// [`file_count`](Self::file_count) and [`take_file`](Self::take_file) give
    /// the files. Fails if no chapter was added.
    pub fn finish(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if self.failed {
            return Err("The export stopped after an error; start it again.".into());
        }
        if self.chapters_added == 0 {
            return Err("Nothing to export: no chapter was added.".into());
        }
        self.close_open().inspect_err(|_| self.failed = true)?;
        let ext = self.kind.extension();
        let done = std::mem::take(&mut self.done);
        let groups: Vec<usize> = done.iter().map(|d| d.volume.group).collect();
        let mut used: Vec<String> = Vec::new();
        for (i, d) in done.into_iter().enumerate() {
            let in_group: Vec<usize> =
                groups.iter().enumerate().filter(|(_, g)| **g == d.volume.group).map(|(j, _)| j).collect();
            let part = (in_group.len() > 1).then(|| in_group.iter().position(|&j| j == i).unwrap_or(0) + 1);
            let mut name = volume_name(&self.options.title, &d.volume, part, ext);
            // Two files must not take each other's place.
            let mut n = 2;
            while used.contains(&name) {
                name = format!("{} ({n}).{ext}", name.trim_end_matches(&format!(".{ext}")));
                n += 1;
            }
            used.push(name.clone());
            self.files.push(Some(OutputFile { name, bytes: d.bytes }));
        }
        self.held = 0;
        self.closed = true;
        Ok(())
    }

    /// How many files [`finish`](Self::finish) made.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// The name of file `i`, or `None` if it does not exist or was taken.
    pub fn file_name(&self, i: usize) -> Option<&str> {
        self.files.get(i)?.as_ref().map(|f| f.name.as_str())
    }

    /// Take file `i`; its memory is the caller's from then on. Each file can be
    /// taken once.
    pub fn take_file(&mut self, i: usize) -> Result<OutputFile, String> {
        if !self.closed {
            return Err("Finish the export before taking its files.".into());
        }
        self.files
            .get_mut(i)
            .ok_or_else(|| format!("There is no file {i}."))?
            .take()
            .ok_or_else(|| format!("File {i} was already taken."))
    }
}

// ---- Converting --------------------------------------------------------------

/// Everything the converter needs for pages shown on `device`, converted the way
/// `conversion` says: `pipeline::options::process_options_for`, which the
/// parity tests check this against. `kind` is the file type, which decides the
/// depth and the colour.
fn process_options_for(device: &DeviceProfile, conversion: &ConversionSettings, kind: FileType) -> ProcessOptions {
    // Held sideways, the pages are laid out on the sideways screen. Pictures
    // (EPUB, CBZ) stay that shape and the reader rotates them; an XTC or XTCH
    // page is always the reader's own frame, so it is turned into it.
    let sideways = conversion.orientation == library::Orientation::Landscape;
    let pictures = matches!(kind, FileType::Epub | FileType::Cbz);
    ProcessOptions {
        width: if sideways { device.height } else { device.width },
        height: if sideways { device.width } else { device.height },
        layout: conversion.layout,
        crop: conversion.crop,
        margins: conversion.margins,
        trim_sensitivity: conversion.trim_sensitivity,
        trim_padding: conversion.trim_padding,
        spread: conversion.spread,
        direction: conversion.direction,
        contrast: conversion.contrast,
        auto_levels: conversion.auto_levels,
        strip_cut: conversion.strip_cut,
        overlap_percent: conversion.overlap_percent,
        dither: conversion.dither,
        depth: output_depth(device, kind),
        turn: sideways && !pictures,
        zoom: conversion.zoom.clamped(),
        zoom_mode: conversion.zoom_mode,
        guide_arrows: conversion.guide_arrows,
        // The one place colour is decided: only an EPUB or CBZ, only for a
        // colour screen, and only when the visitor did not choose black and white.
        color: pictures && conversion.keeps_color(device),
    }
}

/// How many grays the pages are made with: an XTC or XTCH book is as its format
/// says; an EPUB or CBZ follows the reader's screen.
fn output_depth(device: &DeviceProfile, kind: FileType) -> Depth {
    match kind {
        FileType::Xtc => Depth::One,
        FileType::Xtch => Depth::Two,
        _ if device.gray_levels <= 4 => Depth::Two,
        _ => Depth::Eight,
    }
}

/// What a chapter takes decoded, from the pictures' headers: a byte a pixel
/// (three planes for colour) and a quarter more for the cropped or scaled copy.
fn decoded_bytes<P: AsRef<[u8]>>(pages: &[P], color: bool) -> u64 {
    let pixels: u64 = pages
        .iter()
        .filter_map(|p| converter::decode::image_dimensions(p.as_ref()).ok())
        .map(|(w, h)| u64::from(w) * u64::from(h))
        .fold(0u64, u64::saturating_add);
    let planes = if color { 3 } else { 1 };
    let pixels = pixels.saturating_mul(planes);
    pixels.saturating_add(pixels / 4)
}

/// Decode every page of a chapter, then plan and convert it as a whole (a
/// webtoon's images are joined before they are cut into pages), and pack or
/// encode each page. `pack_chapter` / `pack_chapter_color` of the desktop export.
fn convert_chapter<P: AsRef<[u8]>>(
    name: &str,
    pages: &[P],
    opts: &ProcessOptions,
    pictures: bool,
) -> Result<Vec<Vec<u8>>, String> {
    convert_chapter_planned(name, pages, opts, pictures).map(|(made, _)| made)
}

/// Where a converted page of a preview comes from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PreviewSource {
    /// The picture it was made from (an index into the pages given); none for a strip.
    pub image: Option<usize>,
    /// `whole`, `rotated`, `left`, `right`, `piece` (one screen of a zoomed piece) or `strip`.
    pub part: String,
    /// For a strip: the rows of the joined strip the page holds.
    pub top: u32,
    pub height: u32,
}

fn source_of(page: &converter::plan::PlannedPage) -> PreviewSource {
    use converter::plan::{Part, Source};
    match page.source {
        Source::Image { index, part } => PreviewSource {
            image: Some(index),
            part: match part {
                Part::Whole => "whole",
                Part::Rotated => "rotated",
                Part::Half(0) => "left",
                Part::Half(_) => "right",
                Part::Tile { .. } => "piece",
            }
            .into(),
            top: 0,
            height: 0,
        },
        Source::Strip { top, height } => PreviewSource { image: None, part: "strip".into(), top, height },
    }
}

/// [`convert_chapter`], with where each page came from.
fn convert_chapter_planned<P: AsRef<[u8]>>(
    name: &str,
    pages: &[P],
    opts: &ProcessOptions,
    pictures: bool,
) -> Result<(Vec<Vec<u8>>, Vec<PreviewSource>), String> {
    let unreadable = |index: usize, e: String| {
        format!("page {} of {name} could not be read ({e}); check that the file is a picture", index + 1)
    };
    let never = || false;
    let cuts: Vec<[f32; 4]> = Vec::new();
    if pictures && opts.color {
        let mut planes = Vec::with_capacity(pages.len());
        for (index, page) in pages.iter().enumerate() {
            let (luma, chroma) = converter::decode::decode_to_ycc(page.as_ref())
                .map_err(|e| unreadable(index, e.to_string()))?;
            planes.push((luma, chroma));
        }
        let prepared = converter::plan::prepare_owned_color(planes, opts, &never).map_err(|e| e.to_string())?;
        let plan = converter::plan::plan(&prepared, opts);
        let packed = converter::plan::render_all_color_with(&prepared, &plan.pages, opts, &cuts, &never, |_, image| {
            epub::encode_color_page(&image)
        })
        .map_err(|e| e.to_string())?;
        let made: Vec<Vec<u8>> = packed.into_iter().collect::<Result<_, _>>()?;
        Ok((made, plan.pages.iter().map(source_of).collect()))
    } else {
        let mut grays: Vec<GrayImage> = Vec::with_capacity(pages.len());
        for (index, page) in pages.iter().enumerate() {
            grays.push(
                converter::decode::decode_to_gray(page.as_ref()).map_err(|e| unreadable(index, e.to_string()))?,
            );
        }
        let prepared = converter::plan::prepare_owned(grays, opts, &never).map_err(|e| e.to_string())?;
        let plan = converter::plan::plan(&prepared, opts);
        let packed =
            converter::plan::render_all_cropped_with(&prepared, &plan.pages, opts, &cuts, &never, |_, image| {
                if pictures {
                    epub::encode_page(&image)
                } else {
                    converter::xtc::encode_page(&image).map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?;
        let made: Vec<Vec<u8>> = packed.into_iter().collect::<Result<_, _>>()?;
        Ok((made, plan.pages.iter().map(source_of).collect()))
    }
}

/// Convert some pages only to look at them: what a chapter's pages would be in a file for
/// this reader, as pictures (PNG or JPEG) whatever the file type. A phone or tablet keeps its pages
/// as they are, so they come back unchanged.
pub fn preview_pages<P: AsRef<[u8]>>(options: &ExportOptions, pages: &[P]) -> Result<Vec<Vec<u8>>, String> {
    let device = options.resolve_device()?;
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    if device_profiles::is_original(&device.id) {
        return Ok(pages.iter().map(|p| p.as_ref().to_vec()).collect());
    }
    let process = process_options_for(&device, &options.settings, options.file_type());
    convert_chapter("the preview", pages, &process, true)
}

/// [`preview_pages`], and where each page of the result comes from (which picture, and which
/// part of it), so the screen can set the originals beside them.
pub fn preview_pages_with_sources<P: AsRef<[u8]>>(
    options: &ExportOptions,
    pages: &[P],
) -> Result<(Vec<Vec<u8>>, Vec<PreviewSource>), String> {
    let device = options.resolve_device()?;
    if pages.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if device_profiles::is_original(&device.id) {
        let same = pages.iter().map(|p| p.as_ref().to_vec()).collect();
        let sources = (0..pages.len())
            .map(|i| PreviewSource { image: Some(i), part: "whole".into(), top: 0, height: 0 })
            .collect();
        return Ok((same, sources));
    }
    let process = process_options_for(&device, &options.settings, options.file_type());
    convert_chapter_planned("the preview", pages, &process, true)
}

/// The cover as one page for the device: the whole picture, fitted to the
/// screen, uncropped. `None` if it cannot be made, in which case the file simply
/// starts with the first chapter.
fn make_cover(first_page: &[u8], opts: &ProcessOptions, pictures: bool) -> Option<Vec<u8>> {
    let mut opts = opts.clone();
    opts.layout = converter::process::Layout::Manga;
    opts.crop = converter::process::CropMode::Full;
    opts.margins = Default::default();
    opts.spread = converter::process::SpreadMode::Fit;
    if pictures && opts.color {
        let (luma, chroma) = converter::decode::decode_to_ycc(first_page).ok()?;
        let prepared = converter::plan::prepare_owned_color(vec![(luma, chroma)], &opts, &|| false).ok()?;
        let plan = converter::plan::plan(&prepared, &opts);
        let page = converter::plan::render_page_color(&prepared, plan.pages.first()?, &opts, [0.0; 4]);
        return epub::encode_color_page(&page).ok();
    }
    let gray = converter::decode::decode_to_gray(first_page).ok()?;
    let page = converter::plan::convert_chapter(&[gray], &opts).into_iter().next()?;
    if pictures {
        epub::encode_page(&page).ok()
    } else {
        converter::xtc::encode_page(&page).ok()
    }
}

// ---- Names -------------------------------------------------------------------

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make `name` safe as one path component on Windows, macOS and Linux: no
/// separators or reserved characters, no trailing dots or spaces, not a reserved
/// device name, at most 100 characters (`sanitize_component` of the desktop export).
pub fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(100).collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        return "manga".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

fn number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `Series - Ch 1-12.xtch`, `Series - Ch 5.xtch`, or with a part number
/// `Series - Part 2 - Ch 13-25.xtch`.
fn output_file_name(series: &str, first: Option<f64>, last: Option<f64>, part: Option<usize>, ext: &str) -> String {
    let mut name = sanitize_component(series);
    if let Some(p) = part {
        name.push_str(&format!(" - Part {p}"));
    }
    match (first, last) {
        (Some(a), Some(b)) if (a - b).abs() < 0.0005 => name.push_str(&format!(" - Ch {}", number(a))),
        (Some(a), Some(b)) => name.push_str(&format!(" - Ch {}-{}", number(a), number(b))),
        _ => {}
    }
    format!("{name}.{ext}")
}

/// A lone unnumbered chapter is named after itself.
fn volume_name(series: &str, volume: &Volume, part: Option<usize>, ext: &str) -> String {
    if volume.chapter_names.len() == 1 && volume.first_number.is_none() {
        let mut name = format!("{} - {}", sanitize_component(series), sanitize_component(&volume.chapter_names[0]));
        if let Some(p) = part {
            name.push_str(&format!(" - Part {p}"));
        }
        return format!("{name}.{ext}");
    }
    output_file_name(series, volume.first_number, volume.last_number, part, ext)
}

// ---- Test helpers shared with the integration tests --------------------------

/// Names and bytes of the entries of a zip, in file order. For tests and tools
/// that compare two archives whose container bytes may differ.
#[doc(hidden)]
pub fn zip_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(|e| e.to_string())?;
        out.push((f.name().map_err(|e| e.to_string())?.to_string(), data));
    }
    Ok(out)
}
