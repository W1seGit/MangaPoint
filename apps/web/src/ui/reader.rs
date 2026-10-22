//! The reader: a long strip, with keys, taps and pinch. The page pictures are
//! filled in by the browser's glue (`web/js/reader.js`) only while they are near
//! the screen.

use super::widgets::{EmptyState, Icon, SettingRow};
use crate::app::{Ctx, Reading};
use crate::bridge::{self, Event};
use crate::model::Book;
use crate::reader_logic::{self as logic, Action, ReaderLook, ZoomChoice};
use dioxus::prelude::*;
use serde_json::json;
use web_engine::state::Prefs;

// ---- opening and closing ----------------------------------------------------------------------

pub fn open_reader(ctx: Ctx, book_id: &str, wanted: Option<usize>) {
    let Some(book) = ctx.book(book_id) else { return };
    if book.needs_file {
        ctx.warn(format!("Import {} again to read it.", book.file_name));
        return;
    }
    let Some(start) = ctx.library.peek().start_place(&book, wanted) else {
        ctx.warn("This book has no pages.");
        return;
    };
    let first = book.chapters.get(start.chapter).map_or(0, |c| c.first);
    let count = book.chapters.get(start.chapter).map_or(0, |c| c.count);
    // In the strip `scroll` says how far down the chapter the reader was.
    let (sp, off) = if start.scroll > 0.0 && count > 0 {
        let p = start.scroll * count as f32;
        let page = (p.floor() as usize).min(count - 1);
        (page, (p - page as f32).clamp(0.0, 0.999))
    } else {
        (start.page, 0.0)
    };
    let id = ctx.id();
    let mut reading = ctx.reading;
    reading.set(Some(Reading {
        book_id: book.id.clone(),
        chapter: start.chapter,
        page: start.page,
        scroll: start.scroll,
        bar: true,
        opened: id,
        scroll_to: Some((first + sp, off)),
    }));
    bridge::send("reader", json!({ "cmd": "opened", "args": [] }));
    after_move(ctx);
}

pub fn close_reader(ctx: Ctx) {
    let mut reading = ctx.reading;
    if reading.peek().is_none() {
        return;
    }
    flush_position(ctx);
    reading.set(None);
    bridge::send("reader", json!({ "cmd": "closed", "args": [] }));
    dioxus::core::spawn_forever(async move {
        let _ = bridge::call_ok("flush", json!({})).await;
    });
}

fn look_of(ctx: Ctx, book: &Book) -> ReaderLook {
    let lib = ctx.library.peek();
    let prefs = lib.state_of(&book.id).map(|s| s.prefs).unwrap_or_default();
    logic::resolve_look(&prefs)
}

/// Remember where the reader is, in the library and on disk.
fn flush_position(ctx: Ctx) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let count = book.chapters.get(r.chapter).map_or(0, |c| c.count);
    let page = r.page.min(count.saturating_sub(1));
    let mut lib = ctx.library;
    lib.write().record_position(&book, r.chapter, page, r.scroll, bridge::now_ms());
    ctx.save_state_of(&book.id);
}

/// After the place changed: remember it.
fn after_move(ctx: Ctx) {
    flush_position(ctx);
}

fn set_reading(ctx: Ctx, f: impl FnOnce(&mut Reading)) {
    let mut reading = ctx.reading;
    let mut guard = reading.write();
    if let Some(r) = guard.as_mut() {
        f(r);
    }
}

fn set_prefs(ctx: Ctx, f: impl FnOnce(&mut Prefs)) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let mut lib = ctx.library;
    lib.write().set_prefs(&book, f);
    ctx.save_state_of(&book.id);
}

pub fn open_chapter(ctx: Ctx, index: usize) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    if index >= book.chapters.len() {
        return;
    }
    let first = book.chapters[index].first;
    set_reading(ctx, |r| {
        r.chapter = index;
        r.page = 0;
        r.scroll = 0.0;
        r.scroll_to = Some((first, 0.0));
    });
    bridge::send("reader", json!({ "cmd": "scrollTop", "args": [] }));
    after_move(ctx);
}

// ---- doing things ---------------------------------------------------------------------------------

pub fn apply_action(ctx: Ctx, action: Action) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let look = look_of(ctx, &book);
    let counts = book.counts();
    match action {
        Action::NextChapter | Action::PrevChapter => {
            if let Some(c) = logic::chapter_step(counts.len(), r.chapter, action == Action::NextChapter) {
                if action == Action::NextChapter {
                    let mut lib = ctx.library;
                    lib.write().mark_read(&book, r.chapter, bridge::now_ms());
                }
                open_chapter(ctx, c);
            }
        }
        Action::First => open_chapter(ctx, r.chapter),
        Action::Last => {
            let last = counts.get(r.chapter).copied().unwrap_or(1).saturating_sub(1);
            let first = book.chapters.get(r.chapter).map_or(0, |c| c.first);
            bridge::send("reader", json!({ "cmd": "scrollToPage", "args": [first + last, 0.0] }));
        }
        Action::ScrollPages(n) => bridge::send("reader", json!({ "cmd": "scrollPages", "args": [n] })),
        Action::ScrollLines(n) => bridge::send("reader", json!({ "cmd": "scrollLines", "args": [n] })),
        Action::ToggleBar => set_reading(ctx, |r| r.bar = !r.bar),
        Action::ZoomIn => set_prefs(ctx, |p| p.zoom = Some(logic::zoom_step(look.zoom, true))),
        Action::ZoomOut => set_prefs(ctx, |p| p.zoom = Some(logic::zoom_step(look.zoom, false))),
        Action::ZoomReset => set_prefs(ctx, |p| p.zoom = Some(100)),
        Action::Fullscreen => bridge::send("reader", json!({ "cmd": "fullscreen", "args": [] })),
        Action::Back => close_reader(ctx),
    }
}

/// Keys, taps, pinches and where the strip has got to.
pub fn handle_reader_event(ctx: Ctx, ev: Event) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let look = look_of(ctx, &book);
    match ev {
        Event::Key { key, shift, ctrl } => {
            if let Some(a) = logic::key_action(&key, shift, ctrl) {
                apply_action(ctx, a);
            }
        }
        // A tap anywhere, even beside the page, shows or hides the bars; it never turns a page.
        Event::Tap { .. } => apply_action(ctx, Action::ToggleBar),
        Event::Zoom { pct } => {
            let z = logic::zoom_from_pinch(pct);
            set_prefs(ctx, |p| p.zoom = Some(z));
        }
        Event::Scrolled { up } => {
            // Scrolling down gets the bars out of the way, scrolling up brings them back.
            if r.bar != up {
                set_reading(ctx, |r| r.bar = up);
            }
        }
        Event::EdgeBack => close_reader(ctx),
        Event::Wheel { dy } => {
            let z = logic::zoom_wheel(look.zoom, dy);
            set_prefs(ctx, |p| p.zoom = Some(z));
        }
        Event::Pos { chapter, page, off, near_end } => {
            let Ok(c) = chapter.parse::<usize>() else { return };
            let Some(ch) = book.chapters.get(c) else { return };
            let in_chapter = page.saturating_sub(ch.first).min(ch.count.saturating_sub(1));
            let moved_on = c != r.chapter && c > r.chapter;
            let along = ((in_chapter as f32 + off as f32) / ch.count.max(1) as f32).clamp(0.0, 1.0);
            set_reading(ctx, |r| {
                r.chapter = c;
                r.page = in_chapter;
                r.scroll = along;
            });
            if moved_on || (near_end && in_chapter + 1 >= ch.count) {
                let finished = if moved_on { r.chapter } else { c };
                let mut lib = ctx.library;
                lib.write().mark_read(&book, finished, bridge::now_ms());
            }
            flush_position(ctx);
        }
        _ => {}
    }
}

// ---- the screen ---------------------------------------------------------------------------------------

#[component]
pub fn ReaderHost() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(r) = ctx.reading.read().clone() else { return rsx! {} };
    rsx! { ReaderScreen { key: "{r.opened}", opened: r.opened } }
}

fn on(yes: bool) -> &'static str {
    if yes { "on" } else { "" }
}

#[component]
fn ReaderScreen(opened: u64) -> Element {
    let ctx = use_context::<Ctx>();
    let Some(r) = ctx.reading.read().clone() else { return rsx! {} };
    let book = ctx.library.read().find(&r.book_id).cloned();
    // Put the strip where the reader was, once it is drawn.
    {
        let target = r.scroll_to;
        use_effect(move || {
            if let Some((page, off)) = target {
                bridge::send("reader", json!({ "cmd": "scrollToPage", "args": [page, off] }));
                set_reading(ctx, |r| r.scroll_to = None);
            }
        });
    }
    let Some(book) = book else {
        return rsx! {
            div { class: "reader show-bar", id: "reader",
                div { class: "reader-gone",
                    EmptyState {
                        title: "This book is not in your library any more",
                        message: "It was removed while you were reading it.",
                        button { class: "btn primary", id: "reader-back", onclick: move |_| close_reader(ctx),
                            Icon { name: "arrow-left", size: 16 } "Back"
                        }
                    }
                }
            }
        };
    };
    if book.chapters.is_empty() {
        return rsx! {
            div { class: "reader show-bar", id: "reader",
                div { class: "reader-gone",
                    EmptyState {
                        title: "Nothing to read",
                        message: "This book has no chapters.",
                        button { class: "btn primary", id: "reader-back", onclick: move |_| close_reader(ctx),
                            Icon { name: "arrow-left", size: 16 } "Back"
                        }
                    }
                }
            }
        };
    }
    let look = look_of(ctx, &book);
    let total = book.chapters.len();
    let current = r.chapter.min(total - 1);
    let chapter = book.chapters[current].clone();
    let zoom = f32::from(look.zoom) / 100.0;
    let items: Vec<(usize, String, bool)> = {
        let lib = ctx.library.read();
        book.chapters
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.label(i), lib.is_read(&book, i)))
            .collect()
    };
    let label = chapter.label(current);
    let next_label = (current + 1 < total).then(|| book.chapters[current + 1].label(current + 1));
    let prev_label = (current > 0).then(|| book.chapters[current - 1].label(current - 1));
    let counts = book.counts();
    let always = bars_always(ctx);
    let bar_class = match (always, r.bar) {
        (true, _) => "reader show-bar pinned",
        (false, true) => "reader show-bar",
        (false, false) => "reader",
    };
    let book_id = book.id.clone();

    rsx! {
        div {
            class: "{bar_class}", id: "reader",
            "data-book": "{book_id}",
            "data-hpan": if look.zoom > 100 { "yes" } else { "no" },
            style: "--zoom: {zoom};",
            ReaderBar {
                title: book.title.clone(), chapter_label: label.clone(), items, current, look,
                percent: logic::percent_finished(r.scroll),
                page: (r.page + 1, chapter.count),
            }
            main { class: "reader-stage", id: "reader-stage",
                {
                    rsx! {
                        div { class: "reader-strip", id: "reader-strip",
                            for index in [current].into_iter().filter(|i| *i < total) {
                                {
                                    let c = book.chapters[index].clone();
                                    let name = c.label(index);
                                    rsx! {
                                        section { class: "reader-chapter", key: "{index}", "data-chapter": "{index}",
                                            if c.count == 0 {
                                                div { class: "reader-divider", "{name} has no pages" }
                                            }
                                            for n in 0..c.count {
                                                div { class: "reader-tile", key: "{c.first + n}", "data-book": "{book_id}", "data-page": "{c.first + n}" }
                                            }
                                        }
                                    }
                                }
                            }
                            {
                                rsx! {
                                    EndCard {
                                        title: "End of {label}",
                                        note: if next_label.is_some() { "" } else { "That was the last chapter." },
                                        prev: prev_label.clone(), next: next_label.clone(),
                                        on_prev: move |_| open_chapter(ctx, current.saturating_sub(1)),
                                        on_next: move |_| open_chapter(ctx, current + 1),
                                        on_back: move |_| close_reader(ctx),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Counts only for the compiler: the book's chapter counts drive the logic above.
            span { class: "reader-meta", hidden: true, "data-chapters": "{counts.len()}" }
        }
    }
}

/// The note at the end of a chapter, with the way on.
#[component]
fn EndCard(
    title: String,
    note: String,
    prev: Option<String>,
    next: Option<String>,
    on_prev: EventHandler<()>,
    on_next: EventHandler<()>,
    on_back: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "reader-end",
            h2 { "{title}" }
            if !note.is_empty() { p { class: "muted", "{note}" } }
            div { class: "reader-end-buttons",
                if let Some(prev) = prev {
                    button { class: "btn", id: "reader-prev-chapter", "aria-label": "Previous chapter",
                        onclick: move |_| on_prev.call(()),
                        Icon { name: "arrow-left", size: 16 } "Previous \u{b7} {prev}"
                    }
                }
                if let Some(next) = next {
                    button { class: "btn primary", id: "reader-next-chapter", "aria-label": "Next chapter",
                        onclick: move |_| on_next.call(()),
                        "Next \u{b7} {next}" Icon { name: "arrow-right", size: 16 }
                    }
                } else {
                    button { class: "btn", id: "reader-end-back", onclick: move |_| on_back.call(()),
                        Icon { name: "book-open", size: 16 } "Back to your library"
                    }
                }
            }
        }
    }
}

/// Chrome over the page: arrows and the chapter across the top, the way out,
/// the title and how far along you are at the bottom, and the settings button.
/// Both bars fade into the page, and slide away while reading.
#[component]
fn ReaderBar(
    title: String,
    chapter_label: String,
    items: Vec<(usize, String, bool)>,
    current: usize,
    look: ReaderLook,
    percent: u32,
    /// The page at the top of the screen, and how many the chapter has.
    page: (usize, usize),
) -> Element {
    let ctx = use_context::<Ctx>();
    let mut menu = use_signal(|| false);
    let mut settings = use_signal(|| false);
    rsx! {
        header { class: "reader-bar reader-top",
            button { class: "reader-round", id: "reader-back", "aria-label": "Back to library", title: "Back to library (Esc)",
                onclick: move |_| close_reader(ctx),
                Icon { name: "book-open", size: 20 }
            }
            div { class: "reader-chapter-nav",
            button { class: "reader-arrow", id: "reader-arrow-left", "aria-label": "Previous chapter", title: "Previous chapter",
                onclick: move |_| { apply_action(ctx, Action::PrevChapter); set_reading(ctx, |r| r.bar = true); },
                Icon { name: "chevron-left", size: 26 }
            }
            div { class: "menu-anchor reader-chapter-anchor",
                button { class: "reader-chapter-link", id: "reader-chapter-picker", "aria-label": "Chapter", title: "Choose a chapter ( [ and ] )",
                    onclick: move |_| menu.set(!menu()),
                    span { class: "reader-chapter-text", "{chapter_label}" } Icon { name: "chevron-down", size: 14 }
                }
                if menu() {
                    div { class: "menu-scrim", onclick: move |_| menu.set(false) }
                    div { class: "menu down reader-menu", id: "reader-chapter-menu", role: "listbox",
                        for (index, name, read) in items.iter().cloned() {
                            button {
                                key: "{index}", class: if index == current { "menu-item reader-chapter-item on" } else { "menu-item reader-chapter-item" },
                                "data-index": "{index}", role: "option", "aria-selected": "{index == current}",
                                onclick: move |_| { menu.set(false); open_chapter(ctx, index); },
                                span { class: "grow", "{name}" }
                                if read { span { class: "reader-read", title: "Read", Icon { name: "check", size: 14 } } }
                            }
                        }
                    }
                }
            }
            button { class: "reader-arrow", id: "reader-arrow-right", "aria-label": "Next chapter", title: "Next chapter",
                onclick: move |_| { apply_action(ctx, Action::NextChapter); set_reading(ctx, |r| r.bar = true); },
                Icon { name: "chevron-right", size: 26 }
            }
            }
            button { class: "reader-round", id: "reader-view-button", "aria-label": "Reading settings", title: "Reading settings",
                onclick: move |_| settings.set(true),
                Icon { name: "settings", size: 20 }
            }
        }
        footer { class: "reader-bar reader-bottom",
            div { class: "reader-caption",
                strong { id: "reader-title", title: "{title}", "{title}" }
                span { class: "reader-percent", id: "reader-percent", "{percent}%" }
                span { class: "sr-only", id: "reader-page-counter", "aria-label": "Page", "aria-live": "polite",
                    "{page.0} / {page.1}"
                }
            }
        }
        if settings() {
            ReaderSettings { look, on_close: move |_| settings.set(false) }
        }
    }
}

/// The reading settings, full screen: how the book is shown and how it is read.
#[component]
fn ReaderSettings(look: ReaderLook, on_close: EventHandler<()>) -> Element {
    let ctx = use_context::<Ctx>();
    let always = bars_always(ctx);
    rsx! {
        div { class: "reader-settings", id: "reader-settings", role: "dialog", "aria-label": "Reading settings",
          div { class: "reader-settings-inner",
            header { class: "reader-settings-head",
                button { class: "reader-round", id: "reader-settings-close", "aria-label": "Back to the book", title: "Back to the book",
                    onclick: move |_| on_close.call(()), Icon { name: "arrow-left", size: 20 }
                }
                h2 { "Reading settings" }
            }
            div { class: "reader-settings-body",
                SettingRow { label: "Zoom", help: "Fit page suits the window: the full width of a phone, a comfortable column on a big screen. Pick a percentage to make it smaller or larger. Pinching, or Ctrl and the mouse wheel, also zoom.",
                    ZoomMenu { look }
                }
                SettingRow { label: "Controls", help: "The bars with the chapter, the settings and your place can hide while you read, or stay on the screen.",
                    div { class: "seg", role: "group", "aria-label": "Controls",
                        button { id: "reader-bars-auto", class: on(!always), "aria-pressed": "{!always}",
                            onclick: move |_| set_bars_always(ctx, false), "Hide while reading" }
                        button { id: "reader-bars-always", class: on(always), "aria-pressed": "{always}",
                            onclick: move |_| set_bars_always(ctx, true), "Always show" }
                    }
                }
            }
          }
        }
    }
}

/// The page zoom: a box showing it (type a percentage in it) and a list of ready-made ones,
/// the same menu as the page preview in the desktop app.
#[component]
fn ZoomMenu(look: ReaderLook) -> Element {
    let ctx = use_context::<Ctx>();
    let mut open = use_signal(|| false);
    let selected = logic::zoom_choice(look.zoom);
    let mut choose = move |choice: ZoomChoice| {
        let zoom = logic::apply_zoom_choice(choice);
        set_prefs(ctx, |p| p.zoom = Some(zoom));
        open.set(false);
    };
    let item = |c: ZoomChoice| if selected == c { "menu-item on" } else { "menu-item" };
    rsx! {
        div { class: if open() { "zoom-menu is-open" } else { "zoom-menu" },
            div { class: "zoom-field",
                span { class: "zoom-icon", Icon { name: "zoom-in", size: 15 } }
                input {
                    id: "reader-zoom-box", r#type: "text", value: "{selected.label()}", "aria-label": "Zoom", spellcheck: "false",
                    onfocus: move |_| open.set(true),
                    onchange: move |e| {
                        if let Some(choice) = ZoomChoice::parse(&e.value()) { choose(choice); }
                    },
                }
                button { class: "zoom-chevron", id: "reader-zoom-toggle", "aria-label": "Zoom choices", title: "Zoom choices", onclick: move |_| open.set(!open()),
                    Icon { name: "chevron-down", size: 15 }
                }
            }
            if open() {
                div { class: "menu-scrim", onclick: move |_| open.set(false) }
                div { class: "menu down zoom-list", id: "reader-zoom-list",
                    button { class: item(ZoomChoice::Fit), id: "reader-fit-height", onclick: move |_| choose(ZoomChoice::Fit), "Fit page" }
                    div { class: "menu-sep" }
                    for percent in [25u16, 50, 75, 90, 125, 150, 200, 300, 400, 500] {
                        button {
                            key: "{percent}", class: item(ZoomChoice::Percent(percent)),
                            onclick: move |_| choose(ZoomChoice::Percent(percent)),
                            "{percent}%"
                        }
                    }
                }
            }
        }
    }
}

fn set_bars_always(ctx: Ctx, yes: bool) {
    let mut settings = ctx.settings;
    settings.write().extra.insert("reader_bars_always".to_string(), serde_json::Value::Bool(yes));
    ctx.save_settings();
}

/// Whether the bars stay on the screen while reading (a setting of the visitor, not of the book).
fn bars_always(ctx: Ctx) -> bool {
    ctx.settings.read().extra.get("reader_bars_always").and_then(|v| v.as_bool()).unwrap_or(false)
}
