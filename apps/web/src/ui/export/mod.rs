//! The Export screen, ported from the desktop app: the same panels (presets, edit, the book,
//! chapters, reader and files), the pages in the middle as they are and as they will be, and the
//! export button. The files are made on this device in a Web Worker (progress bar, Cancel) and
//! downloaded; nothing is sent anywhere.
//!
//! On a phone the panels would not fit side by side, so the pages fill the screen with an
//! Original / Reader switch on top, the panels open as a full-screen sheet from a Settings tab at
//! the bottom, and the Export button sits right above that tab.

mod conversion;
mod panels;
mod preview;

use super::widgets::{EmptyState, Icon};
use crate::app::{Ctx, Page};
use crate::bridge::{self, Event, ExportFile};
use crate::format::plural;
use crate::model::Book;
use converter::xtc::ReadDirection;
use dioxus::prelude::*;
use library::ConversionSettings;
use panels::{BookPanel, ChaptersPanel, EditPanel, ExportFooter, PresetsPanel, RunCard, SendPanel};
use preview::Preview;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use web_engine::devices::{self, DeviceInfo};
use web_engine::options::{default_options_json, ExportOptions, FileType, Grouping};

/// Above this many pages in one file the whole result is a lot to hold in one tab.
pub const BIG_PAGES: usize = 600;
/// Above this many bytes of source pictures, too.
pub const BIG_BYTES: u64 = 300_000_000;
/// How many pages of a chapter the preview shows (it converts them in one go).
pub const PREVIEW_PAGES: usize = 24;
/// The preset entry that stands for "how this book was last exported".
pub const LAST: &str = "last-export";

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Idle,
    Running { fraction: f32, label: String, status: String, elapsed: f64 },
    Done { files: Vec<ExportFile>, pages: usize },
    Failed(String),
}

/// What the middle of the screen shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    Idle,
    Loading,
    Ready {
        /// Blob URLs of the pages as they are (empty text for one that could not be read).
        originals: Vec<String>,
        /// Blob URLs of the pages as they will be. Empty while they are being made.
        converted: Vec<String>,
        /// The pages of the chapter, and how many are shown.
        total: usize,
        shown: usize,
    },
    Failed(String),
}

/// Which of the two pictures a phone shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Original,
    Reader,
}

/// How a book was last exported (kept in the browser, by book).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastExport {
    pub preset: Option<String>,
    pub settings: ConversionSettings,
    /// Unix milliseconds.
    pub at: u64,
    /// The chapters that have been in an export.
    pub done: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportUi {
    pub book: Option<String>,
    pub chosen: BTreeSet<usize>,
    /// The options being edited (device, file type, the settings, grouping).
    pub opts: ExportOptions,
    /// The id of the preset in use ([`LAST`] for how the book was last exported), or empty.
    pub preset: String,
    pub every: usize,
    pub run: Run,
    pub started: bool,
    /// The chapter whose pages are on show.
    pub shown: usize,
    pub look: Look,
    /// Side by side with the originals (wide screens).
    pub compare: bool,
    pub view: View,
    /// The settings sheet is open (phones).
    pub sheet: bool,
    /// Show the controls most people never need.
    pub advanced: bool,
    /// The folded groups of the edit controls that are open.
    pub groups: Vec<&'static str>,
    /// Pages held open for the preview, to let go of.
    pub held: Vec<(String, usize)>,
    /// Counts up with every preview asked for, so an old answer can be told from a new one.
    pub stamp: u64,
    /// What to record as the last export once this one is done.
    pub pending: Option<(String, LastExport)>,
}

impl Default for ExportUi {
    fn default() -> Self {
        Self {
            book: None,
            chosen: BTreeSet::new(),
            opts: ExportOptions::default(),
            preset: "manga".into(),
            every: 5,
            run: Run::Idle,
            started: false,
            shown: 0,
            look: Look::Idle,
            compare: false,
            view: View::Reader,
            sheet: false,
            advanced: false,
            groups: vec!["Layout"],
            held: Vec::new(),
            stamp: 0,
            pending: None,
        }
    }
}

/// Options for a reader and a preset, as the desktop's presets make them.
pub fn options_for(device: &str, preset: &str) -> Result<ExportOptions, String> {
    ExportOptions::from_json(&default_options_json(device, preset)?)
}

pub fn device_of(opts: &ExportOptions) -> Option<device_profiles::DeviceProfile> {
    opts.resolve_device().ok()
}

/// Which grouping the form shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    One,
    PerChapter,
    EveryN,
}

pub fn group_of(g: Grouping) -> Group {
    match g {
        Grouping::Single => Group::One,
        Grouping::PerChapter => Group::PerChapter,
        Grouping::EveryN { .. } => Group::EveryN,
    }
}

/// How many pages and bytes (estimated) the chosen chapters hold.
pub fn chosen_size(book: &Book, chosen: &BTreeSet<usize>) -> (usize, u64) {
    let pages: usize = chosen.iter().filter_map(|&i| book.chapters.get(i)).map(|c| c.count).sum();
    let bytes = if book.page_count == 0 {
        0
    } else {
        (book.size as f64 * pages as f64 / book.page_count as f64) as u64
    };
    (pages, bytes)
}

/// Is this much work worth a warning, and does one file per chapter help?
pub fn is_big(pages: usize, bytes: u64) -> bool {
    pages > BIG_PAGES || bytes > BIG_BYTES
}

/// The name a file type goes by in the readers' lists.
pub fn type_name(t: FileType) -> &'static str {
    match t {
        FileType::Xtc => "XTC",
        FileType::Xtch => "XTCH",
        FileType::Epub => "EPUB",
        FileType::Pdf => "PDF",
        FileType::Cbz => "CBZ",
    }
}

pub fn type_of(name: &str) -> FileType {
    match name {
        "XTC" => FileType::Xtc,
        "XTCH" => FileType::Xtch,
        "EPUB" => FileType::Epub,
        "PDF" => FileType::Pdf,
        _ => FileType::Cbz,
    }
}

/// These settings with the file type, and the colour choice, of `like`: a preset or an edit
/// changes how pages look, never what the reader opens.
pub fn keep_output(mut settings: ConversionSettings, like: &ConversionSettings) -> ConversionSettings {
    settings.set_output(like.output());
    settings.black_and_white = like.black_and_white;
    settings
}

/// Is the book in use set up as this preset (or last export) says?
pub fn is_edited(current: &ConversionSettings, preset: &ConversionSettings) -> bool {
    keep_output(preset.clone(), current) != *current
}

/// Open the Export screen for a book.
pub fn export_book(ctx: Ctx, id: &str) {
    let Some(book) = ctx.book(id) else { return };
    {
        let mut export = ctx.export;
        let mut e = export.write();
        if e.run != Run::Idle && !matches!(e.run, Run::Running { .. }) {
            e.run = Run::Idle;
        }
    }
    start_book(ctx, &book, true);
    ctx.go(Page::Export);
}

/// Choose a book without leaving the page or changing the reader.
fn export_book_quiet(ctx: Ctx, id: &str) {
    let Some(book) = ctx.book(id) else { return };
    start_book(ctx, &book, false);
}

fn start_book(ctx: Ctx, book: &Book, fresh: bool) {
    let settings = ctx.settings.peek().clone();
    let device = if settings.device.is_empty() { device_profiles::X4_PRO_ID.to_string() } else { settings.device };
    let wanted = if settings.preset.is_empty() { "manga".to_string() } else { settings.preset };
    let mut export = ctx.export;
    let first = !export.peek().started || fresh || export.peek().opts == ExportOptions::default();
    if first {
        let presets = ctx.presets.peek().clone();
        let id = if presets.find(&wanted).is_some() { wanted } else { "manga".to_string() };
        if let Ok(mut opts) = options_for(&device, "manga") {
            if let Some(entry) = presets.find(&id) {
                opts.settings = keep_output(entry.preset.settings, &opts.settings);
            }
            export.write().opts = opts;
        }
        export.write().preset = id;
    }
    let mut e = export.write();
    e.book = Some(book.id.clone());
    e.chosen = (0..book.chapters.len()).collect();
    e.shown = 0;
    e.look = Look::Idle;
    e.started = true;
}

pub fn edit(ctx: Ctx, f: impl FnOnce(&mut ExportUi)) {
    let mut export = ctx.export;
    f(&mut export.write());
}

fn remember_choices(ctx: Ctx) {
    let (device, preset) = {
        let e = ctx.export.peek();
        (e.opts.device.clone(), e.preset.clone())
    };
    let mut settings = ctx.settings;
    {
        let mut s = settings.write();
        s.device = device;
        if preset != LAST && !preset.is_empty() {
            s.preset = preset;
        }
    }
    ctx.save_settings();
}

pub fn set_device(ctx: Ctx, id: &str) {
    let Ok(mut opts) = options_for(id, "manga") else { return };
    let old = ctx.export.peek().opts.clone();
    // What carries over from one reader to the next: how pages are made, the grouping, and the
    // file type if the new reader opens it too.
    let mut settings = old.settings.clone();
    let wanted = old.file_type();
    let keeps = devices::devices()
        .iter()
        .find(|d| d.id == id)
        .is_some_and(|d| d.formats.iter().any(|f| type_of(f) == wanted));
    if keeps {
        opts.format = Some(wanted);
    }
    if let Some(out) = opts.format.and_then(FileType::output) {
        settings.set_output(out);
    }
    opts.settings = settings;
    opts.grouping = old.grouping;
    edit(ctx, |e| e.opts = opts);
    remember_choices(ctx);
}

pub fn set_format(ctx: Ctx, format: FileType) {
    edit(ctx, |e| {
        e.opts.format = Some(format);
        if let Some(out) = format.output() {
            e.opts.settings.set_output(out);
        }
    });
}

/// Use a preset: its settings replace the book's (the file type and colour choice stay).
pub fn use_preset(ctx: Ctx, id: &str) {
    let settings = if id == LAST {
        ctx.export.peek().book.as_ref().and_then(|b| ctx.last_exports.peek().get(b).map(|l| l.settings.clone()))
    } else {
        ctx.presets.peek().find(id).map(|e| e.preset.settings)
    };
    let Some(settings) = settings else { return };
    let id = id.to_string();
    edit(ctx, |e| {
        e.opts.settings = keep_output(settings, &e.opts.settings);
        e.preset = id;
    });
    remember_choices(ctx);
}

/// A preset that was deleted: the screen stops following it.
pub fn forget_preset(ctx: Ctx, id: &str) {
    let gone = ctx.export.peek().preset == id;
    if gone {
        edit(ctx, |e| e.preset = String::new());
    }
}

fn start_export(ctx: Ctx) {
    let e = ctx.export.peek().clone();
    let Some(book) = e.book.as_ref().and_then(|id| ctx.book(id)) else { return };
    let chapters: Vec<_> = e
        .chosen
        .iter()
        .filter_map(|&i| book.chapters.get(i))
        .map(|c| json!({ "name": c.name, "number": c.number, "first": c.first, "count": c.count }))
        .collect();
    if chapters.is_empty() {
        ctx.warn("Pick at least one chapter.");
        return;
    }
    let mut opts = e.opts.clone();
    opts.title = book.title.clone();
    opts.created = bridge::now_ms() / 1000;
    if let Grouping::EveryN { .. } = opts.grouping {
        opts.grouping = Grouping::EveryN { n: e.every.max(1) };
    }
    let mut done = ctx.last_exports.peek().get(&book.id).map(|l| l.done.clone()).unwrap_or_default();
    done.extend(e.chosen.iter().filter_map(|&i| book.chapters.get(i)).map(|c| c.name.clone()));
    let last = LastExport {
        preset: Some(e.preset.clone()).filter(|p| !p.is_empty() && p != LAST),
        settings: e.opts.settings.clone(),
        at: bridge::now_ms(),
        done,
    };
    edit(ctx, |e| {
        e.run = Run::Running { fraction: 0.0, label: String::new(), status: "Starting\u{2026}".into(), elapsed: 0.0 };
        e.pending = Some((book.id.clone(), last));
    });
    bridge::send("export_start", json!({ "options": opts, "book": book.id, "chapters": chapters }));
}

pub fn handle_export_event(ctx: Ctx, ev: Event) {
    let mut recorded = false;
    edit(ctx, |e| match ev {
        Event::ExportStatus { message } => {
            if let Run::Running { status, .. } = &mut e.run {
                *status = message;
            }
        }
        Event::ExportProgress { fraction, label, stage, elapsed, chapter, total } => {
            let status = match stage.as_str() {
                "reading" => format!("Reading {label}"),
                "converting" => format!("Converting {label}"),
                _ => format!("{chapter} of {} done", plural(total, "chapter")),
            };
            e.run = Run::Running { fraction: fraction as f32, label, status, elapsed };
        }
        Event::ExportDone { files, pages } => {
            e.run = Run::Done { files, pages };
            recorded = true;
        }
        Event::ExportError { message } => e.run = Run::Failed(message),
        Event::ExportCancelled => e.run = Run::Idle,
        _ => {}
    });
    if recorded {
        let pending = ctx.export.peek().pending.clone();
        if let Some((book, last)) = pending {
            let mut all = ctx.last_exports;
            all.write().insert(book, last);
            ctx.save_last_exports();
        }
    }
}

pub fn cancel_export(ctx: Ctx) {
    dioxus::core::spawn_forever(async move {
        let _ = bridge::call_ok("export_cancel", json!({})).await;
    });
    edit(ctx, |e| e.run = Run::Idle);
    ctx.notify("Export cancelled. Nothing was made.");
}

#[derive(Deserialize)]
struct Previewed {
    urls: Vec<String>,
}

/// Let go of the pages held open for the preview.
fn release_held(ctx: Ctx) {
    let mut export = ctx.export;
    let held = std::mem::take(&mut export.write().held);
    if held.is_empty() {
        return;
    }
    dioxus::core::spawn_forever(async move {
        for (book, index) in held {
            let _ = bridge::call_ok("original_release", json!({ "book": book, "index": index })).await;
        }
    });
}

/// Show the chapter on screen again, as it is and as it will be.
fn request_preview(ctx: Ctx) {
    let e = ctx.export.peek().clone();
    let Some(book) = e.book.as_ref().and_then(|id| ctx.book(id)) else { return };
    let Some(chapter) = book.chapters.get(e.shown).cloned() else { return };
    let stamp = {
        let mut export = ctx.export;
        let mut w = export.write();
        w.stamp += 1;
        w.stamp
    };
    let shown = chapter.count.min(PREVIEW_PAGES);
    let (first, total) = (chapter.first, chapter.count);
    let opts = e.opts.clone();
    let book_id = book.id.clone();
    let fresh = move |ctx: Ctx| ctx.export.peek().stamp == stamp;
    dioxus::core::spawn_forever(async move {
        bridge::sleep(220).await;
        if !fresh(ctx) {
            return;
        }
        release_held(ctx);
        let mut originals = Vec::with_capacity(shown);
        let mut held = Vec::with_capacity(shown);
        for k in 0..shown {
            match bridge::call::<String>("original_url", json!({ "book": book_id, "index": first + k })).await {
                Ok(url) => {
                    originals.push(url);
                    held.push((book_id.clone(), first + k));
                }
                Err(_) => originals.push(String::new()),
            }
        }
        if !fresh(ctx) {
            edit(ctx, |e| e.held.extend(held));
            release_held(ctx);
            return;
        }
        edit(ctx, |e| {
            e.held = held;
            e.look = Look::Ready { originals: originals.clone(), converted: Vec::new(), total, shown };
        });
        let made = bridge::call::<Previewed>("preview", json!({ "options": opts, "book": book_id, "first": first, "count": shown })).await;
        if !fresh(ctx) {
            return;
        }
        edit(ctx, |e| {
            e.look = match made {
                Ok(p) => Look::Ready { originals, converted: p.urls, total, shown },
                Err(message) => Look::Failed(message),
            }
        });
    });
}

#[component]
pub fn Panel(
    title: String,
    icon: &'static str,
    /// What the tests call this panel.
    id: &'static str,
    /// Is it open to begin with?
    open: Option<bool>,
    /// A starting height in pixels. Without one the panel takes what is left.
    height: Option<u32>,
    /// Buttons for the title bar, which do not fold the panel.
    actions: Option<Element>,
    children: Element,
) -> Element {
    let mut open = use_signal(move || open.unwrap_or(true));
    let class = match (open(), height.is_some()) {
        (true, true) => "px-panel open fixed",
        (true, false) => "px-panel open",
        _ => "px-panel collapsed",
    };
    let style = height.map(|h| format!("--h: {h}px;")).unwrap_or_default();
    let chevron = if open() { "chevron-down" } else { "chevron-right" };
    rsx! {
        section { class: "{class}", style: "{style}", "data-panel": "{id}", id: "panel-{id}",
            header { class: "px-panel-head", onclick: move |_| open.set(!open()),
                Icon { name: chevron, size: 15 }
                Icon { name: icon, size: 15 }
                span { class: "grow", "{title}" }
                if let Some(actions) = actions {
                    div { class: "px-panel-actions", onclick: move |e| e.stop_propagation(), {actions} }
                }
            }
            div { class: "px-panel-body", {children} }
        }
    }
}

#[component]
pub fn ExportPage() -> Element {
    let ctx = use_context::<Ctx>();
    // Open on the first book when none was chosen (done after drawing, never during it).
    use_effect(move || {
        let first = ctx.library.read().books.iter().find(|b| !b.needs_file).map(|b| b.id.clone());
        let none_yet = {
            let e = ctx.export.peek();
            e.book.is_none() && !e.started
        };
        if let (true, Some(id)) = (none_yet, first) {
            export_book_quiet(ctx, &id);
        }
    });
    // The pages on show change with the book, the chapter and every setting that changes a page.
    let look_key = use_memo(move || {
        let e = ctx.export.read();
        format!("{:?}|{}|{}", e.book, e.shown, serde_json::to_string(&e.opts).unwrap_or_default())
    });
    use_effect(move || {
        let _ = look_key();
        request_preview(ctx);
    });
    use_drop(move || {
        release_held(ctx);
        dioxus::core::spawn_forever(async move {
            let _ = bridge::call_ok("preview_free", json!({})).await;
        });
        let mut export = ctx.export;
        export.write().sheet = false;
    });

    let lib = ctx.library.read();
    let e = ctx.export.read().clone();
    let narrow = *ctx.narrow.read();
    let usable: Vec<Book> = lib.books.iter().filter(|b| !b.needs_file).cloned().collect();
    if usable.is_empty() {
        return rsx! {
            div { class: "q-head", div { class: "grow", h1 { "Export" } p { class: "muted", "Make files for your e-reader, phone or tablet from the books in your library." } } }
            EmptyState {
                title: "Nothing to export yet",
                message: "Import a book first. The files are made here, on this device.",
                shelf: true,
                button { class: "btn primary", onclick: move |_| ctx.go(Page::Library), Icon { name: "book-open", size: 16 } "Go to the library" }
            }
        };
    }
    let book = e.book.as_ref().and_then(|id| lib.find(id)).cloned();
    let Some(book) = book else {
        return rsx! { EmptyState { title: "Choose a book", message: "Pick the book to export.", compact: true } };
    };
    let device = devices::devices().into_iter().find(|d| d.id == e.opts.device);
    let original = device.as_ref().is_some_and(|d| d.original);
    drop(lib);

    let panels_left = rsx! {
        if !original {
            PresetsPanel { book: book.clone(), open: true }
            EditPanel { open: !narrow }
        } else {
            div { class: "banner", id: "original-note",
                Icon { name: "info", size: 16 }
                div { class: "grow",
                    strong { "Original quality" }
                    p { class: "muted small", style: "margin: 4px 0 0",
                        "Nothing is resized, grayed, dithered or split. Each page goes into the file exactly as it was, in the original quality, so it looks its best on a phone or tablet."
                    }
                }
            }
        }
        BookPanel { books: usable.clone(), book: book.clone(), open: !narrow }
    };
    let panels_right = rsx! {
        ChaptersPanel { book: book.clone(), open: !narrow }
        SendPanel { device: device.clone(), book: book.clone(), open: !narrow }
    };

    if narrow {
        return rsx! {
            div { class: "export-screen narrow", id: "export-screen",
                div { class: "xm-tabs", role: "tablist",
                    button { id: "export-tab-original", role: "tab", class: if e.view == View::Original { "on" } else { "" },
                        onclick: move |_| edit(ctx, |e| e.view = View::Original), "Original" }
                    button { id: "export-tab-reader", role: "tab", class: if e.view == View::Reader { "on" } else { "" },
                        onclick: move |_| edit(ctx, |e| e.view = View::Reader), "Reader" }
                }
                main { class: "export-center", Preview { book: book.clone(), narrow: true } }
                if !e.sheet {
                    BottomBar { sheet: false }
                }
                if e.sheet {
                    div { class: "xm-sheet", id: "export-sheet", role: "dialog", "aria-label": "Export settings",
                        div { class: "xm-sheet-body",
                            {panels_left}
                            {panels_right}
                        }
                        BottomBar { sheet: true }
                    }
                }
            }
        };
    }

    rsx! {
        div { class: "export-screen", id: "export-screen",
            div { class: "export-body",
                aside { class: "export-side left", {panels_left} }
                main { class: "export-center", Preview { book: book.clone(), narrow: false } }
                aside { class: "export-side right",
                    {panels_right}
                    RunCard { book: book.clone() }
                    ExportFooter { book: book.clone(), compact: false }
                }
            }
        }
    }
}

/// The bottom of a phone's Export screen: what the export is doing, the Export button across the
/// whole width, and the Settings tab under it (a panel header that opens or closes the sheet).
#[component]
fn BottomBar(sheet: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let book = e.book.as_ref().and_then(|id| ctx.book(id));
    let Some(book) = book else { return rsx! {} };
    let chevron = if sheet { "chevron-down" } else { "chevron-up" };
    rsx! {
        div { class: "xm-bottom",
            RunCard { book: book.clone() }
            ExportFooter { book, compact: true }
            button { class: "xm-settings-tab", id: "export-settings-tab", "aria-expanded": "{sheet}",
                onclick: move |_| edit(ctx, |e| e.sheet = !e.sheet),
                Icon { name: chevron, size: 16 }
                Icon { name: "sliders-horizontal", size: 16 }
                span { "Settings" }
            }
        }
    }
}

pub fn on(yes: bool) -> &'static str {
    if yes { "on" } else { "" }
}

#[component]
pub fn DeviceNote(d: DeviceInfo) -> Element {
    rsx! {
        div { class: "muted small device-note", id: "export-device-note",
            "{d.width} \u{d7} {d.height} \u{b7} {d.ppi} ppi \u{b7} "
            if d.color { "colour" } else { "{d.grays} grays" }
            if !d.note.is_empty() { div { "{d.note}" } }
        }
    }
}

/// The reading direction the settings carry, for the Original tab's note.
pub fn direction_of(opts: &ExportOptions) -> ReadDirection {
    opts.settings.direction
}
