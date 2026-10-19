//! The reader: one page at a time or a long strip, left to right or right to
//! left, with keys, taps, swipes and pinch. The page pictures are filled in by
//! the browser's glue (`web/js/reader.js`) only while they are near the screen.

use super::widgets::{EmptyState, Icon, SettingRow};
use crate::app::{Ctx, Reading};
use crate::bridge::{self, Event};
use crate::model::Book;
use crate::reader_logic::{self as logic, Action, ReaderLook, Zone, ZoomChoice};
use dioxus::prelude::*;
use library::{PageDirection, ReadFit, ReadMode};
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
    let look = look_of(ctx, &book);
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
        strip: vec![start.chapter],
        bar: true,
        opened: id,
        scroll_to: (look.mode == ReadMode::Scroll).then_some((first + sp, off)),
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
    bridge::send("pin", json!({ "book": "", "indexes": [] }));
    dioxus::core::spawn_forever(async move {
        let _ = bridge::call_ok("flush", json!({})).await;
    });
}

fn look_of(ctx: Ctx, book: &Book) -> ReaderLook {
    let lib = ctx.library.peek();
    let prefs = lib.state_of(&book.id).map(|s| s.prefs).unwrap_or_default();
    logic::resolve_look(&prefs, book)
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

/// After the place changed: remember it and keep the next pages ready.
fn after_move(ctx: Ctx) {
    flush_position(ctx);
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    if look_of(ctx, &book).mode == ReadMode::Paged {
        let pages = logic::preload_pages(&book, r.chapter, r.page);
        bridge::send("pin", json!({ "book": book.id, "indexes": pages }));
    }
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

fn open_chapter(ctx: Ctx, index: usize) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    if index >= book.chapters.len() {
        return;
    }
    let look = look_of(ctx, &book);
    let first = book.chapters[index].first;
    set_reading(ctx, |r| {
        r.chapter = index;
        r.page = 0;
        r.scroll = 0.0;
        r.strip = vec![index];
        r.scroll_to = (look.mode == ReadMode::Scroll).then_some((first, 0.0));
    });
    if look.mode == ReadMode::Scroll {
        bridge::send("reader", json!({ "cmd": "scrollTop", "args": [] }));
    }
    after_move(ctx);
}

fn set_mode(ctx: Ctx, mode: ReadMode) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let before = look_of(ctx, &book);
    if before.mode == mode {
        return;
    }
    let first = book.chapters.get(r.chapter).map_or(0, |c| c.first);
    // Each mode has its own natural fit; the visitor's explicit fit is dropped
    // when it would be odd in the new mode.
    set_prefs(ctx, |p| {
        p.mode = Some(mode);
        p.fit = None;
    });
    let count = book.chapters.get(r.chapter).map_or(0, |c| c.count);
    set_reading(ctx, |r| {
        // The end card is not a page of the strip.
        r.page = r.page.min(count.saturating_sub(1));
        r.strip = vec![r.chapter];
        r.scroll_to = (mode == ReadMode::Scroll).then_some((first + r.page, 0.0));
    });
    after_move(ctx);
}

// ---- doing things ---------------------------------------------------------------------------------

pub fn apply_action(ctx: Ctx, action: Action) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let look = look_of(ctx, &book);
    let counts = book.counts();
    match action {
        Action::Next | Action::Prev => {
            if look.mode != ReadMode::Paged {
                let n = if action == Action::Next { 1 } else { -1 };
                bridge::send("reader", json!({ "cmd": "scrollPages", "args": [n] }));
                return;
            }
            let step = if action == Action::Next {
                logic::paged_next(&counts, r.chapter, r.page)
            } else {
                logic::paged_prev(&counts, r.chapter, r.page)
            };
            match step {
                Some((c, p)) => {
                    set_reading(ctx, |r| {
                        r.chapter = c;
                        r.page = p;
                        r.bar = false;
                    });
                    // The end of a chapter counts as having read it.
                    if p >= counts[c] {
                        let mut lib = ctx.library;
                        lib.write().mark_read(&book, c, bridge::now_ms());
                    }
                    after_move(ctx);
                }
                None => {
                    // The end of the last chapter: show the end card.
                    let last = counts.len().saturating_sub(1);
                    if action == Action::Next && r.chapter == last && r.page < counts[last] {
                        set_reading(ctx, |r| r.page = counts[last]);
                    }
                }
            }
        }
        Action::NextChapter | Action::PrevChapter => {
            if let Some(c) = logic::chapter_step(counts.len(), r.chapter, action == Action::NextChapter) {
                if action == Action::NextChapter {
                    let mut lib = ctx.library;
                    lib.write().mark_read(&book, r.chapter, bridge::now_ms());
                }
                open_chapter(ctx, c);
            }
        }
        Action::First => {
            if look.mode == ReadMode::Paged {
                set_reading(ctx, |r| r.page = 0);
                after_move(ctx);
            } else {
                open_chapter(ctx, r.chapter);
            }
        }
        Action::Last => {
            let last = counts.get(r.chapter).copied().unwrap_or(1).saturating_sub(1);
            if look.mode == ReadMode::Paged {
                set_reading(ctx, |r| r.page = last);
                after_move(ctx);
            } else {
                let first = book.chapters.get(r.chapter).map_or(0, |c| c.first);
                bridge::send("reader", json!({ "cmd": "scrollToPage", "args": [first + last, 0.0] }));
            }
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

/// Keys, taps, swipes, pinches and where the strip has got to.
pub fn handle_reader_event(ctx: Ctx, ev: Event) {
    let Some(r) = ctx.reading.peek().clone() else { return };
    let Some(book) = ctx.book(&r.book_id) else { return };
    let look = look_of(ctx, &book);
    match ev {
        Event::Key { key, shift, ctrl } => {
            if let Some(a) = logic::key_action(&key, shift, ctrl, look.mode, look.direction) {
                apply_action(ctx, a);
            }
        }
        Event::Tap { x, w } => {
            let zone = logic::zone_at(x, w);
            let action = match look.mode {
                ReadMode::Paged => logic::zone_action(zone, look.direction),
                ReadMode::Scroll => logic::strip_tap_action(zone),
            };
            apply_action(ctx, action);
        }
        Event::Swipe { dx } => {
            if look.mode == ReadMode::Paged {
                if let Some(a) = logic::swipe_action(dx, look.direction) {
                    apply_action(ctx, a);
                }
            }
        }
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
            if look.mode != ReadMode::Scroll {
                return;
            }
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
            // The next chapter joins the strip as the end comes near.
            if near_end {
                let last = r.strip.last().copied().unwrap_or(c);
                if last + 1 < book.chapters.len() && !r.strip.contains(&(last + 1)) {
                    set_reading(ctx, |r| r.strip.push(last + 1));
                }
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
    let mode_name = if look.mode == ReadMode::Scroll { "scroll" } else { "paged" };
    let fit_name = match look.fit {
        ReadFit::Width => "width",
        ReadFit::Height => "height",
        ReadFit::Original => "original",
    };
    let dir_name = if look.direction == PageDirection::RightToLeft { "rtl" } else { "ltr" };
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
    let counts = book.counts();
    let page = r.page.min(chapter.count);
    let bar_class = if r.bar { "reader show-bar" } else { "reader" };
    let book_id = book.id.clone();

    rsx! {
        div {
            class: "{bar_class}", id: "reader",
            "data-mode": "{mode_name}", "data-fit": "{fit_name}", "data-dir": "{dir_name}", "data-book": "{book_id}",
            "data-hpan": if look.zoom > 100 || look.fit == ReadFit::Original { "yes" } else { "no" },
            style: "--zoom: {zoom};",
            ReaderBar {
                title: book.title.clone(), chapter_label: label.clone(), items, current, look,
                percent: logic::percent_finished(look.mode == ReadMode::Paged, page, chapter.count, r.scroll),
                page: if look.mode == ReadMode::Paged { Some((page.min(chapter.count.saturating_sub(1)) + 1, chapter.count)) } else { None },
                strip_page: if look.mode == ReadMode::Scroll { Some((r.page + 1, chapter.count)) } else { None },
            }
            main { class: "reader-stage", id: "reader-stage",
                {if look.mode == ReadMode::Paged {
                    if chapter.count == 0 {
                        rsx! {
                            EndCard { title: "{label} has no pages", note: "", next: next_label.clone(), on_next: move |_| open_chapter(ctx, current + 1), on_back: move |_| close_reader(ctx) }
                        }
                    } else if page >= chapter.count {
                        rsx! {
                            EndCard {
                                title: "End of {label}",
                                note: if next_label.is_some() { "This chapter is marked as read." } else { "That was the last chapter. It is marked as read." },
                                next: next_label.clone(),
                                on_next: { let id = book_id.clone(); move |_| { let mut lib = ctx.library; if let Some(b) = ctx.book(&id) { lib.write().mark_read(&b, current, bridge::now_ms()); } open_chapter(ctx, current + 1) } },
                                on_back: move |_| close_reader(ctx),
                            }
                        }
                    } else {
                        let global = chapter.first + page;
                        rsx! {
                            div { class: "reader-page-wrap", key: "{book_id}-{current}-{page}",
                                div { class: "reader-tile", "data-book": "{book_id}", "data-page": "{global}" }
                            }
                        }
                    }
                } else {
                    rsx! {
                        div { class: "reader-strip", id: "reader-strip",
                            for index in r.strip.iter().copied().filter(|i| *i < total) {
                                {
                                    let c = book.chapters[index].clone();
                                    let name = c.label(index);
                                    rsx! {
                                        section { class: "reader-chapter", key: "{index}", "data-chapter": "{index}",
                                            if index != r.strip[0] {
                                                div { class: "reader-divider", "{name}" }
                                            }
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
                                let last = r.strip.last().copied().unwrap_or(current).min(total - 1);
                                let more = (last + 1 < total).then(|| book.chapters[last + 1].label(last + 1));
                                let last_label = book.chapters[last].label(last);
                                rsx! {
                                    EndCard {
                                        title: "End of {last_label}",
                                        note: if more.is_some() { "" } else { "That was the last chapter." },
                                        next: more, on_next: move |_| open_chapter(ctx, last + 1), on_back: move |_| close_reader(ctx),
                                    }
                                }
                            }
                        }
                    }
                }}
            }
            // Counts only for the compiler: the book's chapter counts drive the logic above.
            span { class: "reader-meta", hidden: true, "data-chapters": "{counts.len()}" }
        }
    }
}

/// The note at the end of a chapter, with the way on.
#[component]
fn EndCard(title: String, note: String, next: Option<String>, on_next: EventHandler<()>, on_back: EventHandler<()>) -> Element {
    rsx! {
        div { class: "reader-end",
            h2 { "{title}" }
            if !note.is_empty() { p { class: "muted", "{note}" } }
            if let Some(next) = next {
                button { class: "btn primary", id: "reader-next-chapter", "aria-label": "Next chapter",
                    onclick: move |_| on_next.call(()),
                    "Next chapter \u{b7} {next}" Icon { name: "arrow-right", size: 16 }
                }
            } else {
                button { class: "btn", id: "reader-end-back", onclick: move |_| on_back.call(()),
                    Icon { name: "arrow-left", size: 16 } "Back to your library"
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
    page: Option<(usize, usize)>,
    strip_page: Option<(usize, usize)>,
) -> Element {
    let ctx = use_context::<Ctx>();
    let mut menu = use_signal(|| false);
    let mut settings = use_signal(|| false);
    let counter = page.or(strip_page);
    let rtl = look.direction == PageDirection::RightToLeft && look.mode == ReadMode::Paged;
    // The left arrow is always on the left: in right-to-left reading it goes on.
    let (left_action, left_label, right_action, right_label) = if rtl {
        (Action::Next, "Next page", Action::Prev, "Previous page")
    } else {
        (Action::Prev, "Previous page", Action::Next, "Next page")
    };
    rsx! {
        header { class: "reader-bar reader-top",
            button { class: "reader-arrow", id: "reader-arrow-left", "aria-label": "{left_label}", title: "{left_label}",
                onclick: move |_| { apply_action(ctx, left_action); set_reading(ctx, |r| r.bar = true); },
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
            button { class: "reader-arrow", id: "reader-arrow-right", "aria-label": "{right_label}", title: "{right_label}",
                onclick: move |_| { apply_action(ctx, right_action); set_reading(ctx, |r| r.bar = true); },
                Icon { name: "chevron-right", size: 26 }
            }
        }
        footer { class: "reader-bar reader-bottom",
            button { class: "reader-round", id: "reader-back", "aria-label": "Back to library", title: "Back to library (Esc)",
                onclick: move |_| close_reader(ctx),
                Icon { name: "arrow-left", size: 20 }
            }
            div { class: "reader-caption",
                strong { id: "reader-title", title: "{title}", "{title}" }
                span { class: "reader-percent", id: "reader-percent", "{percent}%" }
                span { class: "sr-only", id: "reader-page-counter", "aria-label": "Page", "aria-live": "polite",
                    {match counter { Some((p, n)) => format!("{p} / {n}"), None => "\u{2013}".to_string() }}
                }
            }
            button { class: "reader-round", id: "reader-view-button", "aria-label": "Reading settings", title: "Reading settings",
                onclick: move |_| settings.set(true),
                Icon { name: "settings", size: 20 }
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
    rsx! {
        div { class: "reader-settings", id: "reader-settings", role: "dialog", "aria-label": "Reading settings",
            header { class: "reader-settings-head",
                h2 { "Reading settings" }
                button { class: "reader-round", id: "reader-settings-close", "aria-label": "Close", title: "Close",
                    onclick: move |_| on_close.call(()), Icon { name: "x", size: 20 }
                }
            }
            div { class: "reader-settings-body",
                SettingRow { label: "Reading mode", help: "Paged shows one page at a time. Scroll is one long strip, best for webtoons and manhwa.",
                    div { class: "seg", role: "group", "aria-label": "View mode",
                        button { id: "reader-mode-paged", class: on(look.mode == ReadMode::Paged), "aria-pressed": "{look.mode == ReadMode::Paged}",
                            onclick: move |_| set_mode(ctx, ReadMode::Paged), "Paged" }
                        button { id: "reader-mode-scroll", class: on(look.mode == ReadMode::Scroll), "aria-pressed": "{look.mode == ReadMode::Scroll}",
                            onclick: move |_| set_mode(ctx, ReadMode::Scroll), "Scroll" }
                    }
                }
                if look.mode == ReadMode::Paged {
                    SettingRow { label: "Reading direction", help: "Manga is read right to left: the next page is on the left.",
                        div { class: "seg", role: "group", "aria-label": "Reading direction",
                            button { id: "reader-dir-ltr", class: on(look.direction == PageDirection::LeftToRight),
                                "aria-pressed": "{look.direction == PageDirection::LeftToRight}",
                                onclick: move |_| set_prefs(ctx, |p| p.direction = Some(PageDirection::LeftToRight)), "Left to right" }
                            button { id: "reader-dir-rtl", class: on(look.direction == PageDirection::RightToLeft),
                                "aria-pressed": "{look.direction == PageDirection::RightToLeft}",
                                onclick: move |_| set_prefs(ctx, |p| p.direction = Some(PageDirection::RightToLeft)), "Right to left" }
                        }
                    }
                }
                SettingRow { label: "Zoom", help: "Fit the whole page, fit its width, show it at its original size, or pick a percentage. Pinching, or Ctrl and the mouse wheel, also zoom.",
                    ZoomMenu { look }
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
    let selected = logic::zoom_choice(look.fit, look.zoom);
    let mut choose = move |choice: ZoomChoice| {
        let (fit, zoom) = logic::apply_zoom_choice(choice, look_fit(ctx));
        set_prefs(ctx, |p| {
            p.fit = Some(fit);
            p.zoom = Some(zoom);
        });
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
                    button { class: item(ZoomChoice::Width), id: "reader-fit-width", onclick: move |_| choose(ZoomChoice::Width), "Fit width" }
                    button { class: item(ZoomChoice::Original), id: "reader-fit-original", onclick: move |_| choose(ZoomChoice::Original), "Original size" }
                    div { class: "menu-sep" }
                    for percent in [50u16, 75, 90, 100, 125, 150, 200, 300] {
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

/// The fit in use for the book being read.
fn look_fit(ctx: Ctx) -> ReadFit {
    let Some(r) = ctx.reading.peek().clone() else { return ReadFit::Height };
    let Some(book) = ctx.book(&r.book_id) else { return ReadFit::Height };
    look_of(ctx, &book).fit
}

pub fn zone_label(z: Zone) -> &'static str {
    match z {
        Zone::Left => "left",
        Zone::Middle => "middle",
        Zone::Right => "right",
    }
}
