//! The Export screen: pick a book and chapters, a reader, a file type and a
//! preset, and the website makes the files on this device (in a Web Worker,
//! with a progress bar and a Cancel). Nothing is sent anywhere.

use super::widgets::{Chip, EmptyState, Help, Icon, ProgressBar};
use crate::app::{Ctx, Page};
use crate::bridge::{self, Event, ExportFile};
use crate::format::{human_bytes, human_duration, plural};
use crate::model::Book;
use dioxus::prelude::*;
use converter::xtc::ReadDirection;
use serde_json::json;
use std::collections::BTreeSet;
use web_engine::devices::{self, DeviceInfo};
use web_engine::options::{default_options_json, ExportOptions, FileType, Grouping};

/// Above this many pages in one file the whole result is a lot to hold in one tab.
pub const BIG_PAGES: usize = 600;
/// Above this many bytes of source pictures, too.
pub const BIG_BYTES: u64 = 300_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Idle,
    Running { fraction: f32, label: String, status: String, elapsed: f64 },
    Done { files: Vec<ExportFile>, pages: usize },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportUi {
    pub book: Option<String>,
    pub chosen: BTreeSet<usize>,
    /// The options being edited (device, file type, preset's settings, grouping).
    pub opts: ExportOptions,
    pub preset: String,
    pub every: usize,
    pub run: Run,
    pub started: bool,
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
enum Group {
    One,
    PerChapter,
    EveryN,
}

fn group_of(g: Grouping) -> Group {
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

/// Open the Export screen for a book.
pub fn export_book(ctx: Ctx, id: &str) {
    let Some(book) = ctx.book(id) else { return };
    let mut export = ctx.export;
    {
        let settings = ctx.settings.peek().clone();
        let device = if settings.device.is_empty() { device_profiles::X4_PRO_ID.to_string() } else { settings.device };
        let preset = if settings.preset.is_empty() { "manga".to_string() } else { settings.preset };
        let mut e = export.write();
        if e.run != Run::Idle && !matches!(e.run, Run::Running { .. }) {
            e.run = Run::Idle;
        }
        if let Ok(opts) = options_for(&device, &preset) {
            e.opts = opts;
        }
        e.preset = preset;
        e.book = Some(book.id.clone());
        e.chosen = (0..book.chapters.len()).collect();
        e.started = true;
    }
    ctx.go(Page::Export);
}

fn edit(ctx: Ctx, f: impl FnOnce(&mut ExportUi)) {
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
        s.preset = preset;
    }
    ctx.save_settings();
}

fn set_device(ctx: Ctx, id: &str) {
    let preset = ctx.export.peek().preset.clone();
    let Ok(mut opts) = options_for(id, &preset) else { return };
    // The choices that carry over from one reader to the next.
    let old = ctx.export.peek().opts.clone();
    opts.grouping = old.grouping;
    opts.settings.direction = old.settings.direction;
    opts.settings.black_and_white = old.settings.black_and_white;
    edit(ctx, |e| e.opts = opts);
    remember_choices(ctx);
}

fn set_format(ctx: Ctx, format: FileType) {
    edit(ctx, |e| {
        e.opts.format = Some(format);
        if let Some(out) = format.output() {
            e.opts.settings.set_output(out);
        }
    });
}

fn set_preset(ctx: Ctx, preset: &str) {
    let device = ctx.export.peek().opts.device.clone();
    let Ok(mut opts) = options_for(&device, preset) else { return };
    let old = ctx.export.peek().opts.clone();
    opts.grouping = old.grouping;
    opts.settings.black_and_white = old.settings.black_and_white;
    if let Some(f) = old.format {
        // Keep the file type if the reader still opens it.
        opts.format = Some(f);
        if let Some(out) = f.output() {
            opts.settings.set_output(out);
        }
    }
    let preset = preset.to_string();
    edit(ctx, |e| {
        e.opts = opts;
        e.preset = preset;
    });
    remember_choices(ctx);
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
    edit(ctx, |e| {
        e.run = Run::Running { fraction: 0.0, label: String::new(), status: "Starting\u{2026}".into(), elapsed: 0.0 };
    });
    bridge::send("export_start", json!({ "options": opts, "book": book.id, "chapters": chapters }));
}

pub fn handle_export_event(ctx: Ctx, ev: Event) {
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
        Event::ExportDone { files, pages } => e.run = Run::Done { files, pages },
        Event::ExportError { message } => e.run = Run::Failed(message),
        Event::ExportCancelled => e.run = Run::Idle,
        _ => {}
    });
}

fn cancel_export(ctx: Ctx) {
    dioxus::core::spawn_forever(async move {
        let _ = bridge::call_ok("export_cancel", json!({})).await;
    });
    edit(ctx, |e| e.run = Run::Idle);
    ctx.notify("Export cancelled. Nothing was made.");
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
    let lib = ctx.library.read();
    let e = ctx.export.read().clone();
    let usable: Vec<&Book> = lib.books.iter().filter(|b| !b.needs_file).collect();
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
    let all_devices = devices::devices();
    let device = all_devices.iter().find(|d| d.id == e.opts.device).cloned();
    let profile = device_of(&e.opts);
    let ftype = e.opts.file_type();
    let offers_color = profile.as_ref().is_some_and(|p| e.opts.settings.offers_color(p));
    let keeps_color = profile.as_ref().is_some_and(|p| e.opts.settings.keeps_color(p));
    let rtl = e.opts.settings.direction == ReadDirection::RightToLeft;
    let (pages, bytes) = chosen_size(&book, &e.chosen);
    let big = is_big(pages, bytes);
    let grouping = group_of(e.opts.grouping);
    let running = matches!(e.run, Run::Running { .. });
    let presets = library::builtin_presets();
    let original = device.as_ref().is_some_and(|d| d.original);
    let chosen_n = e.chosen.len();
    let all_chosen = chosen_n == book.chapters.len();
    let ids: Vec<String> = usable.iter().map(|b| b.id.clone()).collect();
    let titles: Vec<String> = usable.iter().map(|b| b.title.clone()).collect();

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Export" }
                p { class: "muted", "Make files for your reader from a book in your library. Everything happens on this device." }
            }
        }
        div { class: "export-grid",
            div { class: "card export-main",
                div { class: "panel-section",
                    span { class: "panel-label", "Book" }
                    select { id: "export-book", "aria-label": "Book", disabled: running,
                        value: "{book.id}",
                        onchange: move |ev| export_book_quiet(ctx, &ev.value()),
                        for (id, title) in ids.iter().zip(titles.iter()) {
                            option { key: "{id}", value: "{id}", selected: *id == book.id, "{title}" }
                        }
                    }
                }
                div { class: "panel-section", style: "margin-top: 14px",
                    div { class: "row",
                        span { class: "panel-label grow", "Chapters" }
                        button { class: "btn ghost small", id: "export-all", disabled: running,
                            onclick: { let n = book.chapters.len(); move |_| edit(ctx, |e| e.chosen = if all_chosen { BTreeSet::new() } else { (0..n).collect() }) },
                            if all_chosen { "Select none" } else { "Select all" }
                        }
                    }
                    div { class: "export-chapters", id: "export-chapters",
                        for (i, c) in book.chapters.iter().enumerate() {
                            label { class: "export-chapter", key: "{i}",
                                input {
                                    r#type: "checkbox", checked: e.chosen.contains(&i), disabled: running,
                                    onchange: move |ev| edit(ctx, |e| { if ev.checked() { e.chosen.insert(i); } else { e.chosen.remove(&i); } }),
                                }
                                span { class: "grow export-chapter-name", title: "{c.name}", "{c.label(i)}" }
                                span { class: "muted small", "{plural(c.count, \"page\")}" }
                            }
                        }
                    }
                }
            }
            div { class: "card export-side",
                div { class: "panel-section",
                    span { class: "panel-label", "Your reader" }
                    select { id: "export-device", "aria-label": "Reader", disabled: running,
                        value: "{e.opts.device}",
                        onchange: move |ev| set_device(ctx, &ev.value()),
                        for d in all_devices.iter() {
                            option { key: "{d.id}", value: "{d.id}", selected: d.id == e.opts.device, "{d.name}" }
                        }
                    }
                    if let Some(d) = &device {
                        DeviceNote { d: d.clone() }
                    }
                }
                div { class: "panel-section", style: "margin-top: 14px",
                    span { class: "panel-label", "File type" }
                    div { class: "pills", id: "export-format",
                        for f in device.as_ref().map(|d| d.formats.clone()).unwrap_or_default() {
                            {
                                let ft = match f.as_str() { "XTC" => FileType::Xtc, "XTCH" => FileType::Xtch, "EPUB" => FileType::Epub, "PDF" => FileType::Pdf, _ => FileType::Cbz };
                                rsx! {
                                    button { key: "{f}", id: "export-format-{f}", disabled: running,
                                        class: if ftype == ft { "pill on" } else { "pill" },
                                        onclick: move |_| set_format(ctx, ft), "{f}" }
                                }
                            }
                        }
                    }
                }
                if original {
                    p { class: "muted small panel-note", style: "margin-top: 10px",
                        "The pages are kept exactly as they are, in the original quality, so they look their best on a phone or tablet."
                    }
                } else {
                    div { class: "panel-section", style: "margin-top: 14px",
                        span { class: "panel-label", "Preset" }
                        div { class: "pills", id: "export-preset",
                            for p in presets.iter() {
                                {
                                    let pid = p.id.clone();
                                    rsx! {
                                        button { key: "{p.id}", id: "export-preset-{p.id}", disabled: running, title: "{p.description}",
                                            class: if e.preset == p.id { "pill on" } else { "pill" },
                                            onclick: move |_| set_preset(ctx, &pid), "{p.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "panel-section", style: "margin-top: 14px",
                    span { class: "panel-label", "Reading direction" }
                    div { class: "seg", role: "group", "aria-label": "Reading direction",
                        button { id: "export-dir-rtl", disabled: running, class: on(rtl), onclick: move |_| edit(ctx, |e| e.opts.settings.direction = ReadDirection::RightToLeft), "Right to left" }
                        button { id: "export-dir-ltr", disabled: running, class: on(!rtl), onclick: move |_| edit(ctx, |e| e.opts.settings.direction = ReadDirection::LeftToRight), "Left to right" }
                    }
                }
                if offers_color {
                    div { class: "panel-section", style: "margin-top: 14px",
                        span { class: "panel-label", "Colour" }
                        div { class: "seg", role: "group", "aria-label": "Colour",
                            button { id: "export-color", disabled: running, class: on(keeps_color), onclick: move |_| edit(ctx, |e| e.opts.settings.black_and_white = false), "Colour" }
                            button { id: "export-bw", disabled: running, class: on(!keeps_color), onclick: move |_| edit(ctx, |e| e.opts.settings.black_and_white = true), "Black & white" }
                        }
                    }
                } else if !original {
                    p { class: "muted small panel-note", id: "export-color-note", style: "margin-top: 10px",
                        if ftype == FileType::Xtc || ftype == FileType::Xtch {
                            "XTC and XTCH books are black and white (gray levels). Choose EPUB or CBZ on a colour reader to keep colour."
                        } else {
                            "This reader shows grays, so the pages are made in black and white."
                        }
                    }
                }
                div { class: "panel-section", style: "margin-top: 14px",
                    span { class: "panel-label", "Files" "" Help { text: "One file with every chapter, one file per chapter, or one file for every few chapters. Smaller files are easier on this tab's memory and on the reader." } }
                    div { class: "seg", role: "group", "aria-label": "Files",
                        button { id: "export-group-one", disabled: running, class: on(grouping == Group::One), onclick: move |_| edit(ctx, |e| e.opts.grouping = Grouping::Single), "One file" }
                        button { id: "export-group-chapter", disabled: running, class: on(grouping == Group::PerChapter), onclick: move |_| edit(ctx, |e| e.opts.grouping = Grouping::PerChapter), "Per chapter" }
                        button { id: "export-group-every", disabled: running, class: on(grouping == Group::EveryN), onclick: move |_| edit(ctx, |e| { let n = e.every; e.opts.grouping = Grouping::EveryN { n } }), "Every few" }
                    }
                    if grouping == Group::EveryN {
                        div { class: "row", style: "margin-top: 8px",
                            span { class: "muted small", "Chapters per file" }
                            input { r#type: "number", min: "1", max: "999", value: "{e.every}", style: "width: 80px", id: "export-every", "aria-label": "Chapters per file", disabled: running,
                                oninput: move |ev| { let n = ev.value().parse::<usize>().unwrap_or(1).clamp(1, 999); edit(ctx, |e| { e.every = n; e.opts.grouping = Grouping::EveryN { n }; }); } }
                        }
                    }
                }
            }
        }

        if big && !running {
            div { class: "banner warn small", id: "export-big",
                Icon { name: "triangle-alert", size: 16 }
                div { class: "grow",
                    "This is a big export ({plural(pages, \"page\")}, about {human_bytes(bytes)} of pictures). The files are made in this tab's memory, and a phone may run out. "
                    if grouping == Group::One { "One file per chapter is safer." } else { "Per-chapter files keep each one small." }
                }
                if grouping == Group::One {
                    button { class: "btn small primary", id: "export-use-chapters", onclick: move |_| edit(ctx, |e| e.opts.grouping = Grouping::PerChapter), "Use one file per chapter" }
                }
            }
        }

        div { class: "export-run",
            {match &e.run {
                Run::Idle | Run::Failed(_) => rsx! {
                    if let Run::Failed(message) = &e.run {
                        div { class: "banner bad", id: "export-error", Icon { name: "circle-alert", size: 16 } div { "{message}" } }
                    }
                    div { class: "row wrap",
                        button { class: "btn primary", id: "export-start", disabled: chosen_n == 0, onclick: move |_| start_export(ctx),
                            Icon { name: "file-down", size: 16 } "Export {plural(chosen_n, \"chapter\")} \u{b7} {plural(pages, \"page\")}"
                        }
                        Chip { text: format!("{} file", ftype.extension().to_uppercase()) }
                    }
                },
                Run::Running { fraction, status, elapsed, .. } => rsx! {
                    div { class: "card export-progress", id: "export-progress",
                        div { class: "row",
                            strong { class: "grow", "Making your files\u{2026}" }
                            span { class: "muted small", id: "export-elapsed", "{human_duration((*elapsed / 1000.0) as u64)}" }
                            button { class: "btn", id: "export-cancel", onclick: move |_| cancel_export(ctx), Icon { name: "x", size: 15 } "Cancel" }
                        }
                        ProgressBar { fraction: Some(*fraction) }
                        div { class: "muted small", id: "export-status", "{status} \u{b7} {(*fraction * 100.0).round()}%" }
                        p { class: "muted small", style: "margin: 6px 0 0", "Keep this tab open. The page stays usable while the files are made." }
                    }
                },
                Run::Done { files, pages } => rsx! {
                    div { class: "card export-done", id: "export-done",
                        div { class: "row",
                            Icon { name: "circle-check", size: 18 }
                            strong { class: "grow", "Done: {plural(files.len(), \"file\")}, {plural(*pages, \"page\")}" }
                            if files.len() > 1 {
                                button { class: "btn", id: "export-download-all", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call_ok("download_all", json!({})).await; }); },
                                    Icon { name: "download", size: 15 } "Download all"
                                }
                            }
                        }
                        div { class: "export-files",
                            for f in files.iter() {
                                {
                                    let index = f.index;
                                    rsx! {
                                        div { class: "list-row export-file", key: "{f.name}", "data-file": "{f.name}",
                                            Icon { name: "file-down", size: 18 }
                                            div { class: "grow", strong { class: "export-file-name", "{f.name}" } div { class: "muted small", "{human_bytes(f.size)}" } }
                                            button { class: "btn primary export-download", onclick: move |_| bridge::send("download_result", json!({ "index": index })),
                                                Icon { name: "download", size: 15 } "Download"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "muted small", style: "margin: 8px 0 0", "The files live in this tab until you close it or export again." }
                        div { class: "row", style: "margin-top: 8px",
                            button { class: "btn ghost", id: "export-again", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call_ok("free_export", json!({})).await; }); edit(ctx, |e| e.run = Run::Idle) }, "Export something else" }
                        }
                    }
                },
            }}
        }
    }
}

fn on(yes: bool) -> &'static str {
    if yes { "on" } else { "" }
}

/// Choose a book without leaving the page or changing the reader.
fn export_book_quiet(ctx: Ctx, id: &str) {
    let Some(book) = ctx.book(id) else { return };
    let mut export = ctx.export;
    let mut e = export.write();
    if e.opts == ExportOptions::default() {
        let settings = ctx.settings.peek().clone();
        let device = if settings.device.is_empty() { device_profiles::X4_PRO_ID.to_string() } else { settings.device };
        let preset = if settings.preset.is_empty() { "manga".to_string() } else { settings.preset };
        if let Ok(opts) = options_for(&device, &preset) {
            e.opts = opts;
        }
        e.preset = preset;
    }
    e.book = Some(book.id.clone());
    e.chosen = (0..book.chapters.len()).collect();
    e.started = true;
}

#[component]
fn DeviceNote(d: DeviceInfo) -> Element {
    rsx! {
        div { class: "muted small device-note", id: "export-device-note",
            "{d.width} \u{d7} {d.height} \u{b7} {d.ppi} ppi \u{b7} "
            if d.color { "colour" } else { "{d.grays} grays" }
            if !d.note.is_empty() { div { "{d.note}" } }
        }
    }
}
