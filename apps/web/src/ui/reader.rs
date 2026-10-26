//! The reader: a long strip, with keys, taps and pinch. The page pictures are
//! filled in by the browser's glue (`web/js/reader.js`) only while they are near
//! the screen.

use super::widgets::{EmptyState, Icon};
use mangapoint_ui::reader::{EndCard, ReadPos, ReaderBar};
use crate::app::{Ctx, Reading};
use crate::bridge::{self, Event};
use crate::model::Book;
use mangapoint_ui::logic::reader::{self as logic, Action, ReaderLook, ZoomChoice};
use dioxus::prelude::*;
use serde_json::json;
use web_engine::state::Prefs;

/// How the zoom of the reader is changed, besides the menu.
const ZOOM_HELP: &str = "Pinching, or Ctrl and the mouse wheel, also zoom.";

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
    logic::resolve_look(prefs.zoom)
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
    rsx! { BrowserReaderScreen { key: "{r.opened}", opened: r.opened } }
}

#[component]
fn BrowserReaderScreen(opened: u64) -> Element {
    let ctx = use_context::<Ctx>();
    // Where the reader is, for the caption at the bottom.
    let pos = use_memo(move || {
        let Some(r) = ctx.reading.read().clone() else { return ReadPos::default() };
        let pages = ctx.library.read().find(&r.book_id).and_then(|b| b.chapters.get(r.chapter).map(|c| c.count)).unwrap_or(0);
        ReadPos { page: r.page, pages, frac: r.scroll }
    });
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
                pos, always, zoom_help: ZOOM_HELP,
                on_back: move |_| close_reader(ctx),
                on_prev: move |_| { apply_action(ctx, Action::PrevChapter); set_reading(ctx, |r| r.bar = true); },
                on_next: move |_| { apply_action(ctx, Action::NextChapter); set_reading(ctx, |r| r.bar = true); },
                on_chapter: move |i: usize| open_chapter(ctx, i),
                on_zoom: move |choice: ZoomChoice| {
                    let zoom = logic::apply_zoom_choice(choice);
                    set_prefs(ctx, |p| p.zoom = Some(zoom));
                },
                on_always: move |yes: bool| set_bars_always(ctx, yes),
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

fn set_bars_always(ctx: Ctx, yes: bool) {
    let mut settings = ctx.choices;
    settings.write().extra.insert("reader_bars_always".to_string(), serde_json::Value::Bool(yes));
    ctx.save_settings();
}

/// Whether the bars stay on the screen while reading (a setting of the visitor, not of the book).
fn bars_always(ctx: Ctx) -> bool {
    ctx.choices.read().extra.get("reader_bars_always").and_then(|v| v.as_bool()).unwrap_or(false)
}
