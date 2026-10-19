//! The reader: one page at a time or a long strip, left to right or right to
//! left, with keys, taps, swipes and pinch. The page pictures are filled in by
//! the browser's glue (`web/js/reader.js`) only while they are near the screen.

use super::widgets::{EmptyState, Icon};
use crate::app::{Ctx, Reading};
use crate::bridge::{self, Event};
use crate::model::Book;
use crate::reader_logic::{self as logic, Action, ReaderLook, Zone};
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

/// The bar across the top: it hides while reading and shows on a tap in the middle.
#[component]
fn ReaderBar(
    title: String,
    chapter_label: String,
    items: Vec<(usize, String, bool)>,
    current: usize,
    look: ReaderLook,
    page: Option<(usize, usize)>,
    strip_page: Option<(usize, usize)>,
) -> Element {
    let ctx = use_context::<Ctx>();
    let mut menu = use_signal(|| false);
    let mut view = use_signal(|| false);
    let counter = page.or(strip_page);
    rsx! {
        header { class: "reader-bar",
            button { class: "btn", id: "reader-back", "aria-label": "Back to library", title: "Back (Esc)",
                onclick: move |_| close_reader(ctx),
                Icon { name: "arrow-left", size: 16 } span { class: "reader-back-text", "Back" }
            }
            div { class: "reader-title grow",
                strong { id: "reader-title", title: "{title}", "{title}" }
            }
            div { class: "menu-anchor",
                button { class: "btn", id: "reader-chapter-picker", "aria-label": "Chapter", title: "Choose a chapter ( [ and ] )",
                    onclick: move |_| menu.set(!menu()),
                    "{chapter_label}" Icon { name: "chevron-down", size: 14 }
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
            span { class: "reader-counter", id: "reader-page-counter", "aria-label": "Page", "aria-live": "polite",
                {match counter { Some((p, n)) => format!("{p} / {n}"), None => "\u{2013}".to_string() }}
            }
            div { class: "reader-inline",
                ViewControls { look }
            }
            div { class: "menu-anchor reader-view",
                button { class: "btn", id: "reader-view-button", "aria-label": "View options", title: "Mode, direction, fit and zoom",
                    onclick: move |_| view.set(!view()),
                    Icon { name: "sliders-horizontal", size: 16 }
                }
                if view() {
                    div { class: "menu-scrim", onclick: move |_| view.set(false) }
                    div { class: "menu down reader-view-menu", id: "reader-view-menu",
                        ViewControls { look }
                    }
                }
            }
        }
    }
}

/// Mode, direction, fit and zoom.
#[component]
fn ViewControls(look: ReaderLook) -> Element {
    let ctx = use_context::<Ctx>();
    rsx! {
        div { class: "reader-controls",
            div { class: "seg", role: "group", "aria-label": "View mode",
                button { id: "reader-mode-paged", class: on(look.mode == ReadMode::Paged), "aria-label": "Paged", "aria-pressed": "{look.mode == ReadMode::Paged}",
                    title: "One page at a time", onclick: move |_| set_mode(ctx, ReadMode::Paged), "Paged" }
                button { id: "reader-mode-scroll", class: on(look.mode == ReadMode::Scroll), "aria-label": "Scroll", "aria-pressed": "{look.mode == ReadMode::Scroll}",
                    title: "One long strip", onclick: move |_| set_mode(ctx, ReadMode::Scroll), "Scroll" }
            }
            if look.mode == ReadMode::Paged {
                div { class: "seg", role: "group", "aria-label": "Reading direction",
                    button { id: "reader-dir-ltr", class: on(look.direction == PageDirection::LeftToRight), "aria-label": "Left to right",
                        "aria-pressed": "{look.direction == PageDirection::LeftToRight}",
                        onclick: move |_| set_prefs(ctx, |p| p.direction = Some(PageDirection::LeftToRight)), "Left to right" }
                    button { id: "reader-dir-rtl", class: on(look.direction == PageDirection::RightToLeft), "aria-label": "Right to left",
                        "aria-pressed": "{look.direction == PageDirection::RightToLeft}",
                        onclick: move |_| set_prefs(ctx, |p| p.direction = Some(PageDirection::RightToLeft)), "Right to left" }
                }
            }
            div { class: "seg", role: "group", "aria-label": "Fit",
                button { id: "reader-fit-width", class: on(look.fit == ReadFit::Width), "aria-label": "Fit width", "aria-pressed": "{look.fit == ReadFit::Width}",
                    onclick: move |_| set_prefs(ctx, |p| p.fit = Some(ReadFit::Width)), "Width" }
                button { id: "reader-fit-height", class: on(look.fit == ReadFit::Height), "aria-label": "Fit height", "aria-pressed": "{look.fit == ReadFit::Height}",
                    onclick: move |_| set_prefs(ctx, |p| p.fit = Some(ReadFit::Height)), "Height" }
                button { id: "reader-fit-original", class: on(look.fit == ReadFit::Original), "aria-label": "Original size", "aria-pressed": "{look.fit == ReadFit::Original}",
                    onclick: move |_| set_prefs(ctx, |p| p.fit = Some(ReadFit::Original)), "Original" }
            }
            div { class: "reader-zoom",
                button { class: "btn ghost", id: "reader-zoom-out", "aria-label": "Zoom out", title: "Zoom out ( - )",
                    onclick: move |_| apply_action(ctx, Action::ZoomOut), Icon { name: "zoom-out", size: 16 } }
                span { id: "reader-zoom-level", "aria-label": "Zoom", "{look.zoom}%" }
                button { class: "btn ghost", id: "reader-zoom-in", "aria-label": "Zoom in", title: "Zoom in ( + )",
                    onclick: move |_| apply_action(ctx, Action::ZoomIn), Icon { name: "zoom-in", size: 16 } }
            }
            button { class: "btn ghost reader-fullscreen", id: "reader-fullscreen", "aria-label": "Full screen", title: "Full screen (F11)",
                onclick: move |_| apply_action(ctx, Action::Fullscreen),
                Icon { name: "maximize", size: 16 }
            }
        }
    }
}

pub fn zone_label(z: Zone) -> &'static str {
    match z {
        Zone::Left => "left",
        Zone::Middle => "middle",
        Zone::Right => "right",
    }
}
