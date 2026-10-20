//! The panels of the Export screen. Each is the desktop app's panel of the same name, with what
//! only makes sense on a computer (USB and Wi-Fi sending, the page editor) left out: here the
//! files are downloaded.

use super::conversion::ConversionControls;
use super::{
    cancel_export, chosen_size, edit, export_book_quiet, group_of, is_big, is_edited, keep_output, on, set_format,
    start_export, use_preset, Group, LastExport, Panel, Run, LAST,
};
use crate::app::{Ctx, Modal, ModalAction};
use crate::bridge::{self};
use crate::ui::readers::ReaderPicker;
use crate::format::{human_bytes, human_duration, plural};
use crate::model::Book;
use crate::ranges::select_by_text;
use crate::ui::widgets::{Chip, Cover, Help, Icon, ProgressBar};
use dioxus::prelude::*;
use library::PresetEntry;
use serde_json::json;
use std::collections::BTreeSet;
use web_engine::options::{FileType, Grouping};

// ---- presets ---------------------------------------------------------------------------------

/// What a preset in the list is.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// How the book was last exported.
    Last,
    /// One that comes with the app.
    Builtin,
    /// One the user made: it can be saved to, renamed and deleted.
    Mine,
}

/// One preset in the list. Choosing it uses it, showing what it is for; one of your own shows its
/// name as a box to rename it by, and a save and a delete button while it is chosen or pointed at.
#[component]
fn PresetItem(kind: Kind, id: String, name: String, description: String, settings: library::ConversionSettings, on: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let current = ctx.export.read().opts.settings.clone();
    let edited = on && is_edited(&current, &settings);
    let class = match (kind, on) {
        (Kind::Last, true) => "px-preset last on",
        (Kind::Last, false) => "px-preset last",
        (_, true) => "px-preset on",
        (_, false) => "px-preset",
    };
    let (use_id, save_id, delete_id, rename_id, delete_name) = (id.clone(), id.clone(), id.clone(), id.clone(), name.clone());
    rsx! {
        div { class: "{class}", id: "export-preset-{id}", onclick: move |_| use_preset(ctx, &use_id),
            div { class: "px-preset-head",
                if kind == Kind::Mine {
                    input {
                        class: "px-name", r#type: "text", maxlength: "40", value: "{name}",
                        readonly: !on, "aria-label": "Preset name",
                        title: if on { "Click to rename" } else { "" },
                        onclick: move |e| { if on { e.stop_propagation(); } },
                        onchange: move |e| {
                            let mut presets = ctx.presets;
                            let result = presets.write().rename(&rename_id, e.value().trim());
                            match result {
                                Ok(()) => ctx.save_presets(),
                                Err(err) => ctx.error(err.to_string()),
                            }
                        },
                    }
                } else {
                    span { class: "grow px-item-name", "{name}" }
                }
                if edited { Chip { text: "edited", kind: Some("warn".to_string()) } }
                if kind == Kind::Mine {
                    div { class: "px-actions",
                        button { class: "px-icon", title: "Save the current settings to this preset",
                            onclick: move |e| {
                                e.stop_propagation();
                                let now = ctx.export.peek().opts.settings.clone();
                                let mut presets = ctx.presets;
                                let result = presets.write().overwrite(&save_id, now);
                                match result {
                                    Ok(()) => { ctx.save_presets(); ctx.notify("Preset saved."); }
                                    Err(err) => ctx.error(err.to_string()),
                                }
                            },
                            Icon { name: "save", size: 15 }
                        }
                        button { class: "px-icon danger", title: "Delete this preset",
                            onclick: move |e| {
                                e.stop_propagation();
                                ctx.ask(Modal {
                                    title: format!("Delete \u{201c}{delete_name}\u{201d}?"),
                                    body: "The preset is removed. Books exported with it keep their settings.".into(),
                                    confirm: "Delete".into(), cancel: "Keep".into(), danger: true,
                                    action: ModalAction::DeletePreset(delete_id.clone()),
                                });
                            },
                            Icon { name: "trash", size: 15 }
                        }
                    }
                }
                if on { Icon { name: "check", size: 14 } }
            }
            if on && !description.is_empty() {
                p { class: "px-desc", "{description}" }
            }
        }
    }
}

fn ago(ms_then: u64) -> String {
    let secs = bridge::now_ms().saturating_sub(ms_then) / 1000;
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} minutes ago", secs / 60),
        3600..=86_399 => format!("{} hours ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[component]
pub fn PresetsPanel(book: Book, open: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let entries: Vec<PresetEntry> = ctx.presets.read().all();
    let e = ctx.export.read().clone();
    let last: Option<LastExport> = ctx.last_exports.read().get(&book.id).cloned();
    let builtins: Vec<_> = entries.iter().filter(|p| p.builtin).cloned().collect();
    let mine: Vec<_> = entries.iter().filter(|p| !p.builtin).cloned().collect();
    // A name for a new preset that no other has.
    let new_name = {
        let mut name = "My preset".to_string();
        let mut n = 2;
        while entries.iter().any(|p| p.preset.name.eq_ignore_ascii_case(&name)) {
            name = format!("My preset {n}");
            n += 1;
        }
        name
    };
    rsx! {
        Panel { title: "Presets", icon: "layers", id: "presets", height: 280, open,
            div { class: "px-list", id: "export-preset",
                if let Some(last) = last {
                    PresetItem {
                        key: "{book.id}-last",
                        kind: Kind::Last, id: LAST.to_string(), name: "Last export",
                        description: format!("The settings this book was last exported with ({}), so the next part matches the first. Only an export changes it.", ago(last.at)),
                        settings: last.settings.clone(), on: e.preset == LAST,
                    }
                }
                for p in builtins.iter() {
                    PresetItem {
                        key: "{p.preset.id}", kind: Kind::Builtin, id: p.preset.id.clone(), name: p.preset.name.clone(),
                        description: p.preset.description.clone(), settings: p.preset.settings.clone(), on: e.preset == p.preset.id,
                    }
                }
                for p in mine.iter() {
                    PresetItem {
                        key: "{p.preset.id}", kind: Kind::Mine, id: p.preset.id.clone(), name: p.preset.name.clone(),
                        description: p.preset.description.clone(), settings: p.preset.settings.clone(), on: e.preset == p.preset.id,
                    }
                }
                button { class: "px-new", id: "export-preset-new", title: "Keep the current settings as a preset of your own",
                    onclick: move |_| {
                        let now = ctx.export.peek().opts.settings.clone();
                        let mut presets = ctx.presets;
                        let result = presets.write().save_new(&new_name, now);
                        match result {
                            Ok(id) => { ctx.save_presets(); edit(ctx, |e| e.preset = id); }
                            Err(err) => ctx.error(err.to_string()),
                        }
                    },
                    Icon { name: "plus", size: 14 } "New preset"
                }
            }
        }
    }
}

// ---- edit ------------------------------------------------------------------------------------

#[component]
pub fn EditPanel(open: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let value = ctx.export.read().opts.settings.clone();
    rsx! {
        Panel { title: "Edit", icon: "sliders-horizontal", id: "edit", open: open,
            ConversionControls {
                compact: true,
                value: value.clone(),
                onchange: move |v: library::ConversionSettings| edit(ctx, |e| {
                    e.opts.settings = keep_output(v, &e.opts.settings);
                }),
            }
        }
    }
}

// ---- the book --------------------------------------------------------------------------------

#[component]
pub fn BookPanel(books: Vec<Book>, book: Book, open: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let running = matches!(ctx.export.read().run, Run::Running { .. });
    rsx! {
        Panel { title: "Book", icon: "book-open", id: "series", open,
            div { class: "px-layer on",
                div { class: "px-layer-cover", Cover { id: book.id.clone(), has_cover: book.has_cover } }
                div { class: "px-layer-text",
                    strong { "{book.title}" }
                    span { class: "muted small", "{plural(book.chapters.len(), \"chapter\")} \u{b7} {plural(book.page_count, \"page\")}" }
                }
            }
            if books.len() > 1 {
                div { class: "panel-section", style: "margin-top: 10px",
                    span { class: "panel-label", "Another book" }
                    select { id: "export-book", "aria-label": "Book", disabled: running,
                        value: "{book.id}",
                        onchange: move |ev| export_book_quiet(ctx, &ev.value()),
                        for b in books.iter() {
                            option { key: "{b.id}", value: "{b.id}", selected: b.id == book.id, "{b.title}" }
                        }
                    }
                }
            }
        }
    }
}

// ---- chapters --------------------------------------------------------------------------------

/// The bar at the top of the Chapters panel: Every chapter, and a range to export.
#[component]
fn ChapterRangeBar(all_on: bool, count: usize, numbers: Vec<Option<f64>>) -> Element {
    let ctx = use_context::<Ctx>();
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    rsx! {
        div { class: "px-sticky",
            div { class: "px-all-row",
                label { class: "px-all",
                    input {
                        r#type: "checkbox", id: "export-all", checked: all_on,
                        onchange: move |_| {
                            edit(ctx, |e| e.chosen = if all_on { BTreeSet::new() } else { (0..count).collect() });
                            range_error.set(None);
                        },
                    }
                    span { "Every chapter" }
                }
                form {
                    class: "px-range",
                    onsubmit: move |e| {
                        e.prevent_default();
                        match select_by_text(&range_text.read(), &numbers) {
                            Ok(found) if found.is_empty() => {
                                // Nothing changes: leaving the export empty by accident is worse.
                                range_error.set(Some("no chapter of this book has a number in that range".into()));
                            }
                            Ok(found) => {
                                range_error.set(None);
                                edit(ctx, |e| e.chosen = found.into_iter().collect());
                            }
                            Err(msg) => range_error.set(Some(msg)),
                        }
                    },
                    input {
                        r#type: "text", id: "export-range", placeholder: "Range: 1-20, 25, 30-",
                        title: "Export only these chapters. Press Enter or Select.",
                        value: "{range_text}",
                        oninput: move |e| range_text.set(e.value()),
                    }
                    button { class: "btn small", r#type: "submit", "Select" }
                }
            }
            if let Some(msg) = range_error() {
                p { class: "px-range-error", "{msg}" }
            }
        }
    }
}

#[component]
pub fn ChaptersPanel(book: Book, open: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let done = ctx.last_exports.read().get(&book.id).map(|l| l.done.clone()).unwrap_or_default();
    let included = e.chosen.len();
    let all_on = included == book.chapters.len();
    let numbers: Vec<Option<f64>> = book.chapters.iter().map(|c| c.number).collect();
    let running = matches!(e.run, Run::Running { .. });
    rsx! {
        Panel {
            title: "Chapters", icon: "list-checks", id: "chapters", open,
            actions: rsx! { span { class: "muted small", "{included} of {book.chapters.len()}" } },
            ChapterRangeBar { all_on, count: book.chapters.len(), numbers }
            div { class: "px-chapters", id: "export-chapters",
                for (i, c) in book.chapters.iter().enumerate() {
                    {
                        let on_now = e.chosen.contains(&i);
                        let showing = e.shown == i;
                        let class = match (on_now, showing) {
                            (true, true) => "px-chapter on showing",
                            (true, false) => "px-chapter on",
                            (false, true) => "px-chapter showing",
                            _ => "px-chapter",
                        };
                        rsx! {
                            div { key: "{i}", class: "{class}",
                                input {
                                    r#type: "checkbox", checked: on_now, disabled: running, title: "Include this chapter in the export",
                                    onchange: move |ev| edit(ctx, |e| { if ev.checked() { e.chosen.insert(i); } else { e.chosen.remove(&i); } }),
                                }
                                button {
                                    class: "px-chapter-open", title: "Show this chapter in the preview",
                                    onclick: move |_| edit(ctx, |e| e.shown = i),
                                    span { class: "px-chapter-no", { c.number.map(crate::format::format_number).unwrap_or_else(|| "\u{2013}".into()) } }
                                    span { class: "grow px-chapter-name", title: "{c.name}", "{c.label(i)}" }
                                }
                                if done.contains(&c.name) {
                                    span { class: "chip tiny", title: "Already in an export", "exported" }
                                }
                                span { class: "muted small", "{plural(c.count, \"page\")}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- the reader and the files ----------------------------------------------------------------

/// The app's "Send to" panel: the reader (a short list, and a way to Settings to add one), the
/// format, the colour of the pages and how the files are split. The files are downloaded, so the
/// app's "Send to" (folder, USB, Wi-Fi) has nothing to choose here.
#[component]
pub fn SendPanel(book: Book, open: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let setup = ctx.readers.read().reader_setup();
    let profile = super::device_of(&e.opts);
    let ftype = e.opts.file_type();
    let phone = device_profiles::is_original(&e.opts.device);
    let offers_color = profile.as_ref().is_some_and(|p| e.opts.settings.offers_color(p));
    let keeps_color = profile.as_ref().is_some_and(|p| e.opts.settings.keeps_color(p));
    let grouping = group_of(e.opts.grouping);
    let running = matches!(e.run, Run::Running { .. });
    let every_n_valid = e.every >= 1;
    let _ = &book;
    // What the book is made as: only the formats this reader lists (all of them if it lists none,
    // so there is always something to pick). Each has its explanation behind a question mark.
    let reader_formats = setup.formats.clone();
    let formats: Vec<FileType> = [FileType::Xtc, FileType::Xtch, FileType::Epub, FileType::Cbz]
        .into_iter()
        .filter(|f| reader_formats.is_empty() || reader_formats.iter().any(|r| r.eq_ignore_ascii_case(format_label(*f))))
        .collect();
    rsx! {
        Panel { title: "Send to", icon: "file-down", id: "send", height: 600, open,
            div { class: "px-label", "Reader" }
            ReaderPicker {}
            div { class: "px-label", style: "margin-top: 16px", "Format" }
            if phone {
                div { class: "seg", id: "export-format", role: "group", "aria-label": "Format",
                    for f in [FileType::Cbz, FileType::Pdf] {
                        button { key: "{format_label(f)}", id: "export-format-{format_label(f)}", disabled: running,
                            class: on(ftype == f), "aria-pressed": "{ftype == f}",
                            onclick: move |_| set_format(ctx, f),
                            "{format_label(f)}"
                            Help { text: format_help(f) }
                        }
                    }
                }
            } else {
                div { class: "seg", id: "export-format",
                    for f in formats {
                        button { key: "{format_label(f)}", id: "export-format-{format_label(f)}", disabled: running,
                            class: on(ftype == f),
                            onclick: move |_| set_format(ctx, f),
                            "{format_label(f)}"
                            Help { text: format_help(f) }
                        }
                    }
                }
            }
            if offers_color {
                div { class: "px-label", style: "margin-top: 16px",
                    "Pages"
                    Help { text: "Color keeps each page in color, resized to your reader's screen with no tone changes, dithering or sharpening: the reader does the rest. Black & white makes the usual high-contrast gray pages. The preview shows the pages in color too." }
                }
                div { class: "seg", role: "group", "aria-label": "Page colors",
                    button { id: "export-bw", disabled: running, class: on(!keeps_color), onclick: move |_| edit(ctx, |e| e.opts.settings.black_and_white = true), "Black & white" }
                    button { id: "export-color", disabled: running, class: on(keeps_color), onclick: move |_| edit(ctx, |e| e.opts.settings.black_and_white = false), "Color" }
                }
            }
            div { class: "px-label", style: "margin-top: 16px", "Files" }
            div { class: "seg stack", role: "group", "aria-label": "Files",
                button { id: "export-group-one", disabled: running, class: on(grouping == Group::One),
                    onclick: move |_| edit(ctx, |e| e.opts.grouping = Grouping::Single),
                    Icon { name: "book-open", size: 16 } "One file per book"
                }
                button { id: "export-group-chapter", disabled: running, class: on(grouping == Group::PerChapter),
                    onclick: move |_| edit(ctx, |e| e.opts.grouping = Grouping::PerChapter),
                    Icon { name: "layers", size: 16 } "One file per chapter"
                }
                button { id: "export-group-every", disabled: running, class: on(grouping == Group::EveryN),
                    onclick: move |_| edit(ctx, |e| { let n = e.every; e.opts.grouping = Grouping::EveryN { n } }),
                    Icon { name: "list-checks", size: 16 } "One file every N chapters"
                }
            }
            if grouping == Group::EveryN {
                div { class: "row", style: "margin-top: 8px",
                    span { class: "muted small", "Every" }
                    input {
                        r#type: "text", id: "export-every", style: "width: 64px; text-align: center", value: "{e.every}", "aria-label": "Chapters per file", disabled: running,
                        oninput: move |ev| {
                            if let Ok(n) = ev.value().trim().parse::<usize>() {
                                let n = n.clamp(1, 999);
                                edit(ctx, |e| { e.every = n; e.opts.grouping = Grouping::EveryN { n }; });
                            }
                        },
                    }
                    span { class: "muted small", "chapters" }
                    if !every_n_valid { span { class: "small", style: "color: var(--bad)", "Type a number" } }
                }
            }
        }
    }
}

/// How a file type is written on its button.
fn format_label(t: FileType) -> &'static str {
    match t {
        FileType::Xtc => "XTC",
        FileType::Xtch => "XTCH",
        FileType::Epub => "EPUB",
        FileType::Cbz => "CBZ",
        FileType::Pdf => "PDF",
    }
}

/// What a file type is, shown behind its pill.
fn format_help(t: FileType) -> &'static str {
    match t {
        FileType::Xtc => "Black and white pages, made for the Xteink readers. The smallest files, and the quickest to open and turn. The one to pick if you are not sure.",
        FileType::Xtch => "Four grays instead of two, also for the Xteink readers. Files are twice the size, and the reader is slower with them, so it is the less recommended choice.",
        FileType::Epub => "An ordinary e-book that nearly every reader opens: Kindle (through Send to Kindle), Kobo, Boox, PocketBook, and the Xteink too. Each page is a picture the size of the reader's screen.",
        FileType::Cbz => "A comic book archive with the series, the reading direction and a list of chapters, which comic readers show. Kobo, PocketBook, many Boox readers, KOReader and comic apps on phones open it; Kindle does not.",
        FileType::Pdf => "One PDF of the pages exactly as they are, in colour, with a list of chapters. Any phone, tablet or computer opens it.",
    }
}

// ---- the footer and what the export is doing -------------------------------------------------

/// Totals, and the button that starts the export.
#[component]
pub fn ExportFooter(book: Book, compact: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let running = matches!(e.run, Run::Running { .. });
    let (pages, bytes) = chosen_size(&book, &e.chosen);
    let chapters = e.chosen.len();
    let every_ok = !matches!(e.opts.grouping, Grouping::EveryN { .. }) || e.every >= 1;
    let sample = book.id == super::SAMPLE;
    let label = if running {
        "Exporting\u{2026}".to_string()
    } else if sample {
        "Import a book to export it".to_string()
    } else if chapters == 0 {
        "Nothing to export yet".to_string()
    } else {
        format!("Export {} \u{b7} {}", plural(chapters, "chapter"), plural(pages, "page"))
    };
    rsx! {
        div { class: if compact { "px-footer compact" } else { "px-footer" },
            if !compact {
                div { class: "summary-line", span { class: "muted", "Chapters" } span { "{chapters}" } }
                div { class: "summary-line", span { class: "muted", "Pages" } span { "{pages}" } }
                if !sample { div { class: "summary-line", span { class: "muted", "Source pictures, about" } span { "{human_bytes(bytes)}" } } }
            }
            button {
                class: "btn primary big", id: "export-start", style: if compact { "" } else { "margin-top: 10px" },
                disabled: sample || chapters == 0 || running || !every_ok,
                onclick: move |_| start_export(ctx),
                if running || sample || chapters == 0 { "{label}" } else { Icon { name: "file-down", size: 18 } "{label}" }
            }
        }
    }
}

/// Progress while the files are made, the files to download when they are, or what went wrong.
#[component]
pub fn RunCard(book: Book) -> Element {
    let ctx = use_context::<Ctx>();
    let e = ctx.export.read().clone();
    let (pages, bytes) = chosen_size(&book, &e.chosen);
    let big = is_big(pages, bytes);
    let grouping = group_of(e.opts.grouping);
    let running = matches!(e.run, Run::Running { .. });
    rsx! {
        div { class: "export-run",
            if big && !running && e.run == Run::Idle {
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
            {match &e.run {
                Run::Idle => rsx! {},
                Run::Failed(message) => rsx! {
                    div { class: "banner bad", id: "export-error", Icon { name: "circle-alert", size: 16 } div { "{message}" } }
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
