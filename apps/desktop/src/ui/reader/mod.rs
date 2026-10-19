//! The reader: a downloaded series read in the app, in color, as it was
//! downloaded. Paged (one page at a time, the sides of the page turn it) or as
//! a long strip (chapters flow into each other), either direction, fitted to
//! the window as the reader likes. Where the reader is, and how they like to
//! read each series, is remembered (see `library::reading`).
//!
//! The pages are not read into the app: the web view asks for each by address
//! (see `pages`). The keys, the wheel, taps and the scrolling of the strip are
//! seen by a script (`js/reader.js`) that reports to this file, which decides
//! what they mean with the functions of `logic::reader`.

mod load;

use crate::logic::{self, Action, ReaderLook};
use crate::state::{AppState, PendingReading, ReaderOpen};
use crate::ui::widgets::{EmptyState, Icon, Loading};
use dioxus::document::Eval;
use dioxus::prelude::*;
use library::{DownloadedChapter, PageDirection, ReadFit, ReadMode, ReaderPrefs, SeriesInfo};
use load::{ChapterPages, PageInfo};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const READER_JS: &str = include_str!("../../js/reader.js");

/// What the reader shows of the series: everything except where the user is,
/// which changes all the time and must not redraw the pages.
#[derive(Clone, PartialEq)]
struct ReaderData {
    title: String,
    chapters: Vec<DownloadedChapter>,
    info: Option<SeriesInfo>,
    prefs: ReaderPrefs,
    /// The keys of the chapters read.
    read: Vec<String>,
}

fn read_data(state: AppState, series_id: &str) -> Option<ReaderData> {
    let library = state.library.read();
    let series = library.find(series_id)?;
    let progress = library.reading_of(series_id);
    Some(ReaderData {
        title: series.title.clone(),
        chapters: series.chapters.clone(),
        info: series.info.clone(),
        prefs: progress.map(|p| p.prefs).unwrap_or_default(),
        read: progress
            .map(|p| p.chapters.iter().filter(|(_, c)| c.read).map(|(k, _)| k.clone()).collect())
            .unwrap_or_default(),
    })
}

/// A chapter's pages: being listed, listed, or not listable.
#[derive(Clone, PartialEq)]
enum Loaded {
    Loading,
    Ready(Rc<ChapterPages>),
    Failed,
}

/// Where the reader is in the series.
#[derive(Clone, Debug, PartialEq)]
struct View {
    /// The chapter, by position in the series' chapters.
    chapter: usize,
    /// In paged reading, the page; the number of pages is the end of the chapter.
    page: usize,
    /// How far down the chapter, 0 to 1.
    frac: f32,
    /// The chapters in the strip, in scroll reading.
    strip: Vec<usize>,
}

/// The page counter's: where in the chapter, kept apart so scrolling redraws
/// only the counter.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    page: usize,
    pages: usize,
    frac: f32,
}

/// Everything the reader's handlers need. All of it is a handle, so this is
/// cheap to copy into any closure.
#[derive(Clone, Copy)]
struct Ctx {
    state: AppState,
    open: Signal<ReaderOpen>,
    data: Memo<Option<ReaderData>>,
    view: Signal<View>,
    pages: Signal<HashMap<usize, Loaded>>,
    failed: Signal<HashSet<(String, usize)>>,
    retries: Signal<HashMap<(String, usize), u32>>,
    pos: Signal<Pos>,
    fullscreen: Signal<bool>,
    /// The mode picked from the first chapter's pages, so it does not change
    /// under the reader when another chapter looks different.
    auto_mode: Signal<Option<ReadMode>>,
    /// Where to put the strip once its chapter is on the screen.
    restore: Signal<Option<(String, f32)>>,
    js: Signal<Eval>,
}

impl Ctx {
    fn series_id(&self) -> String {
        self.open.peek().series_id.clone()
    }

    fn send(&self, message: serde_json::Value) {
        let _ = self.js.peek().send(message);
    }

    fn chapters(&self) -> Vec<DownloadedChapter> {
        self.data.peek().as_ref().map(|d| d.chapters.clone()).unwrap_or_default()
    }

    /// The reader's settings now: what the user chose, then the defaults.
    fn look(&self) -> ReaderLook {
        let data = self.data.peek();
        let Some(d) = data.as_ref() else {
            return logic::resolve_look(&ReaderPrefs::default(), None, None);
        };
        let chapter = self.view.peek().chapter;
        let aspect = match self.pages.peek().get(&chapter) {
            Some(Loaded::Ready(p)) => {
                let sizes: Vec<(u32, u32)> = p.pages.iter().map(|p| (p.w, p.h)).collect();
                logic::typical_aspect(&sizes)
            }
            _ => None,
        };
        let mut look = logic::resolve_look(&d.prefs, d.info.as_ref(), aspect);
        if d.prefs.mode.is_none() {
            if let Some(mode) = *self.auto_mode.peek() {
                look.mode = mode;
            }
        }
        look
    }

    /// How many pages each chapter has: as listed if it has been, else as the
    /// library says.
    fn counts(&self) -> Vec<usize> {
        let pages = self.pages.peek();
        self.chapters()
            .iter()
            .enumerate()
            .map(|(i, c)| match pages.get(&i) {
                Some(Loaded::Ready(p)) => p.pages.len(),
                _ => c.pages,
            })
            .collect()
    }

    /// List a chapter's pages (off the UI thread) unless that is done or under way.
    fn ensure_loaded(&self, index: usize) {
        let Some(chapter) = self.chapters().get(index).cloned() else { return };
        let ctx = *self;
        let mut pages = self.pages;
        {
            let have = pages.peek();
            if let Some(Loaded::Ready(p)) = have.get(&index) {
                if p.version == load::version_of(&chapter) {
                    return;
                }
            } else if have.contains_key(&index) {
                return;
            }
        }
        pages.write().insert(index, Loaded::Loading);
        let series_id = self.series_id();
        spawn(async move {
            let listed = tokio::task::spawn_blocking(move || {
                load::list_pages(&crate::state::library_files(), &series_id, &chapter)
            })
            .await;
            let mut pages = ctx.pages;
            match listed {
                Ok(listed) => {
                    pages.write().insert(index, Loaded::Ready(Rc::new(listed)));
                    if ctx.auto_mode.peek().is_none() && index == ctx.view.peek().chapter {
                        let mode = ctx.look().mode;
                        let mut auto = ctx.auto_mode;
                        auto.set(Some(mode));
                    }
                }
                Err(_) => {
                    pages.write().insert(index, Loaded::Failed);
                }
            }
        });
    }

    /// Remember where the user is (it reaches the disk a little later).
    fn note(&self, chapter: usize, page: usize, frac: f32, pages: usize) {
        let Some(key) = self.chapters().get(chapter).map(|c| c.key.clone()) else { return };
        let mut pos = self.pos;
        pos.set(Pos { page: page.min(pages.saturating_sub(1)), pages, frac });
        self.state.note_reading(PendingReading {
            series_id: self.series_id(),
            chapter: key,
            page,
            scroll: frac,
            pages,
        });
    }

    /// Remember where the reader is as it stands (when a chapter has just opened).
    fn note_current(&self) {
        let v = self.view.peek().clone();
        let count = self.counts().get(v.chapter).copied().unwrap_or(0);
        let page = v.page.min(count);
        let frac = if count > 1 { page.min(count - 1) as f32 / (count - 1) as f32 } else { 0.0 };
        self.note(v.chapter, page, frac, count);
    }

    /// Show page `page` of chapter `chapter` (paged reading).
    fn show_page(&self, chapter: usize, page: usize) {
        let counts = self.counts();
        let Some((chapter, page)) = logic::clamp_place(&counts, chapter, page) else { return };
        let mut view = self.view;
        view.set(View { chapter, page, frac: 0.0, strip: vec![chapter] });
        self.ensure_loaded(chapter);
        self.send(serde_json::json!({ "c": "top" }));
        let count = counts[chapter];
        let frac = if count > 1 { page.min(count - 1) as f32 / (count - 1) as f32 } else { 0.0 };
        self.note(chapter, page, frac, count);
        self.preload();
    }

    /// Ask the web view for the next two pages ahead of time, and list the next
    /// chapter when the end of this one is close.
    fn preload(&self) {
        let v = self.view.peek().clone();
        let chapters = self.chapters();
        let counts = self.counts();
        let series_id = self.series_id();
        let pages = self.pages.peek().clone();
        let mut urls = Vec::new();
        let (mut c, mut p) = (v.chapter, v.page);
        for _ in 0..2 {
            let Some((nc, np)) = logic::paged_next(&counts, c, p) else { break };
            // The end of a chapter is not a page: step over it.
            let (nc, np) = if np >= counts[nc] {
                match logic::paged_next(&counts, nc, np) {
                    Some(next) => next,
                    None => break,
                }
            } else {
                (nc, np)
            };
            if let Some(Loaded::Ready(listed)) = pages.get(&nc) {
                if let Some(page) = listed.pages.get(np) {
                    urls.push(crate::pages::url(&series_id, &listed.key, &page.file, &listed.version));
                }
            } else {
                self.ensure_loaded(nc);
            }
            (c, p) = (nc, np);
        }
        let count = counts.get(v.chapter).copied().unwrap_or(0);
        if v.page + 3 >= count && v.chapter + 1 < chapters.len() {
            self.ensure_loaded(v.chapter + 1);
        }
        if !urls.is_empty() {
            self.send(serde_json::json!({ "c": "preload", "urls": urls }));
        }
    }

    /// Open a chapter at its beginning.
    fn open_chapter(&self, index: usize) {
        let look = self.look();
        let total = self.chapters().len();
        if index >= total {
            return;
        }
        match look.mode {
            ReadMode::Paged => self.show_page(index, 0),
            ReadMode::Scroll => {
                let mut view = self.view;
                view.set(View { chapter: index, page: 0, frac: 0.0, strip: vec![index] });
                self.ensure_loaded(index);
                self.send(serde_json::json!({ "c": "top" }));
                let count = self.counts()[index];
                self.note(index, 0, 0.0, count);
            }
        }
    }

    /// Change what the user chose for this series.
    fn set_prefs(&self, change: impl FnOnce(&mut ReaderPrefs)) {
        let mut library = self.state.library;
        let id = self.series_id();
        library.write().with_reading(&id, |p| change(&mut p.prefs));
        self.state.save_library();
    }

    fn set_mode(&self, mode: ReadMode) {
        let look = self.look();
        if look.mode == mode {
            return;
        }
        let v = self.view.peek().clone();
        let pos = *self.pos.peek();
        let counts = self.counts();
        let mut auto = self.auto_mode;
        auto.set(Some(mode));
        self.set_prefs(|p| p.mode = Some(mode));
        let chapters = self.chapters();
        let Some(key) = chapters.get(v.chapter).map(|c| c.key.clone()) else { return };
        let mut view = self.view;
        match mode {
            ReadMode::Scroll => {
                let count = counts.get(v.chapter).copied().unwrap_or(0);
                let frac = if look.mode == ReadMode::Paged && count > 1 {
                    v.page.min(count - 1) as f32 / (count - 1) as f32
                } else {
                    pos.frac
                };
                view.set(View { chapter: v.chapter, page: v.page, frac, strip: vec![v.chapter] });
                let mut restore = self.restore;
                restore.set(Some((key, frac)));
            }
            ReadMode::Paged => {
                view.set(View { chapter: v.chapter, page: pos.page, frac: pos.frac, strip: vec![v.chapter] });
                self.preload();
            }
        }
    }

    fn set_fullscreen(&self, on: bool) {
        let mut fullscreen = self.fullscreen;
        fullscreen.set(on);
        dioxus::desktop::window().set_fullscreen(on);
    }

    fn act(&self, action: Action) {
        let look = self.look();
        let v = self.view.peek().clone();
        let counts = self.counts();
        match action {
            Action::Next | Action::Prev if look.mode == ReadMode::Paged => {
                let step = if action == Action::Next {
                    logic::paged_next(&counts, v.chapter, v.page)
                } else {
                    logic::paged_prev(&counts, v.chapter, v.page)
                };
                if let Some((c, p)) = step {
                    self.show_page(c, p);
                }
            }
            Action::Next | Action::Prev => {}
            Action::NextChapter | Action::PrevChapter => {
                if let Some(i) = logic::chapter_step(counts.len(), v.chapter, action == Action::NextChapter) {
                    self.open_chapter(i);
                }
            }
            Action::First | Action::Last => match look.mode {
                ReadMode::Paged => {
                    let count = counts.get(v.chapter).copied().unwrap_or(0);
                    let page = if action == Action::First { 0 } else { count.saturating_sub(1) };
                    self.show_page(v.chapter, page);
                }
                ReadMode::Scroll => self.send(serde_json::json!({
                    "c": "scrollTo",
                    "where": if action == Action::First { "top" } else { "bottom" },
                })),
            },
            Action::ScrollPages(n) => self.send(serde_json::json!({ "c": "scrollBy", "unit": "page", "n": n })),
            Action::ScrollLines(n) => self.send(serde_json::json!({ "c": "scrollBy", "unit": "line", "n": n })),
            Action::ToggleBar => self.send(serde_json::json!({ "c": "bar" })),
            Action::ZoomIn => self.set_prefs(|p| p.zoom = Some(logic::zoom_step(look.zoom, true))),
            Action::ZoomOut => self.set_prefs(|p| p.zoom = Some(logic::zoom_step(look.zoom, false))),
            Action::ZoomReset => self.set_prefs(|p| p.zoom = Some(100)),
            Action::Fullscreen => self.set_fullscreen(!*self.fullscreen.peek()),
            Action::Back => {
                if *self.fullscreen.peek() {
                    self.set_fullscreen(false);
                } else {
                    self.state.close_reader();
                }
            }
        }
    }

    /// Something the script reported.
    fn on_message(&self, message: serde_json::Value) {
        let look = self.look();
        let text = |k: &str| message.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let number = |k: &str| message.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        match message.get("t").and_then(|t| t.as_str()) {
            Some("key") => {
                let flag = |k: &str| message.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(action) = logic::key_action(&text("k"), flag("shift"), flag("ctrl"), look.mode, look.direction) {
                    self.act(action);
                }
            }
            Some("zone") => {
                let zone = logic::zone_at(number("x"), number("w"));
                self.act(logic::zone_action(zone, look.direction));
            }
            Some("zoom") => {
                let zoom = logic::zoom_wheel(look.zoom, number("d"));
                if zoom != look.zoom {
                    self.set_prefs(|p| p.zoom = Some(zoom));
                }
            }
            Some("scroll") if look.mode == ReadMode::Scroll => self.on_scroll(&text("chapter"), number("frac") as f32, number("page") as usize, number("pages") as usize, message.get("near_end").and_then(|v| v.as_bool()).unwrap_or(false)),
            _ => {}
        }
    }

    /// The strip was scrolled: this chapter is in view, this far down.
    fn on_scroll(&self, key: &str, frac: f32, page: usize, pages: usize, near_end: bool) {
        let chapters = self.chapters();
        let Some(index) = chapters.iter().position(|c| c.key == key) else { return };
        let mut view = self.view;
        if self.view.peek().chapter != index {
            let mut v = self.view.peek().clone();
            v.chapter = index;
            view.set(v);
        }
        self.note(index, page, frac, pages);
        // The next chapter joins the strip before the end is reached.
        let last = self.view.peek().strip.last().copied().unwrap_or(index);
        if near_end && last + 1 < chapters.len() {
            let mut v = self.view.peek().clone();
            if !v.strip.contains(&(last + 1)) {
                v.strip.push(last + 1);
                view.set(v);
                self.ensure_loaded(last + 1);
            }
        }
    }
}

/// The screen: the series the reader was opened on.
#[component]
pub fn ReaderPage() -> Element {
    let state = use_context::<AppState>();
    let Some(open) = state.reader.read().clone() else {
        return rsx! {};
    };
    rsx! { ReaderScreen { key: "{open.opened}", open } }
}

/// Where reading starts, from the library as it is now.
fn initial(state: AppState, open: &ReaderOpen) -> View {
    let library = state.library.peek();
    let Some(series) = library.find(&open.series_id) else {
        return View { chapter: 0, page: 0, frac: 0.0, strip: vec![0] };
    };
    let list: Vec<(&str, usize)> = series.chapters.iter().map(|c| (c.key.as_str(), c.pages)).collect();
    let start = logic::start_place(&list, library.reading_of(&open.series_id), open.chapter.as_deref())
        .unwrap_or(logic::Start { chapter: 0, page: 0, scroll: 0.0 });
    View { chapter: start.chapter, page: start.page, frac: start.scroll, strip: vec![start.chapter] }
}

#[component]
fn ReaderScreen(open: ReaderOpen) -> Element {
    let state = use_context::<AppState>();
    let series_id = open.series_id.clone();
    let start_view = initial(state, &open);
    let start_key = state
        .library
        .peek()
        .find(&series_id)
        .and_then(|s| s.chapters.get(start_view.chapter).map(|c| c.key.clone()));
    let start_frac = start_view.frac;
    let open_sig = use_signal(|| open.clone());
    let data = {
        let id = series_id.clone();
        use_memo(move || read_data(state, &id))
    };
    let ctx = Ctx {
        state,
        open: open_sig,
        data,
        view: use_signal(move || start_view.clone()),
        pages: use_signal(HashMap::new),
        failed: use_signal(HashSet::new),
        retries: use_signal(HashMap::new),
        pos: use_signal(|| Pos { page: 0, pages: 0, frac: 0.0 }),
        fullscreen: use_signal(|| false),
        auto_mode: use_signal(|| None),
        restore: use_signal(move || start_key.clone().map(|k| (k, start_frac))),
        js: use_signal(|| document::eval(READER_JS)),
    };
    let mut banner = use_signal(|| None::<Vec<String>>);

    // Messages from the script.
    use_future(move || async move {
        let mut incoming = *ctx.js.peek();
        while let Ok(message) = incoming.recv::<serde_json::Value>().await {
            ctx.on_message(message);
        }
    });
    // The first chapter, and which chapters could be downloaded in better quality.
    use_hook(move || {
        let chapters = ctx.chapters();
        let id = ctx.series_id();
        spawn(async move {
            let index = ctx.view.peek().chapter;
            ctx.ensure_loaded(index);
            let found = tokio::task::spawn_blocking(move || {
                load::improvable(&crate::state::library_files(), &id, &chapters)
            })
            .await
            .unwrap_or_default();
            if !found.is_empty() {
                banner.set(Some(found));
            }
        });
    });
    // Leaving: the position goes to the library now, the script stops, the window
    // is not left in full screen.
    use_drop(move || {
        let _ = ctx.js.peek().send(serde_json::json!({ "c": "stop" }));
        if *ctx.fullscreen.peek() {
            dioxus::desktop::window().set_fullscreen(false);
        }
        state.flush_reading();
    });
    // A chapter downloaded again has new pages: list them again. A chapter
    // that was deleted meanwhile moves the reader to one that is there.
    use_effect(move || {
        let Some(d) = data.read().clone() else { return };
        let current_key = ctx.pages.peek().get(&ctx.view.peek().chapter).and_then(|l| match l {
            Loaded::Ready(p) => Some(p.key.clone()),
            _ => None,
        });
        if let Some(key) = current_key {
            match d.chapters.iter().position(|c| c.key == key) {
                None => {
                    // The chapter being read is gone.
                    let mut pages = ctx.pages;
                    pages.write().clear();
                    let keep = ctx.view.peek().chapter.min(d.chapters.len().saturating_sub(1));
                    ctx.show_page(keep, 0);
                    return;
                }
                Some(at) if at != ctx.view.peek().chapter => {
                    // Chapters before it were deleted: it is the same chapter at another place.
                    let mut pages = ctx.pages;
                    pages.write().clear();
                    let mut view = ctx.view;
                    let mut v = ctx.view.peek().clone();
                    v.chapter = at;
                    v.strip = vec![at];
                    view.set(v);
                    ctx.ensure_loaded(at);
                    return;
                }
                Some(_) => {}
            }
        }
        let stale: Vec<usize> = ctx
            .pages
            .peek()
            .iter()
            .filter(|(i, l)| match l {
                Loaded::Ready(p) => d.chapters.get(**i).is_none_or(|c| load::version_of(c) != p.version || c.key != p.key),
                _ => false,
            })
            .map(|(i, _)| *i)
            .collect();
        for i in stale {
            let mut pages = ctx.pages;
            pages.write().remove(&i);
            ctx.ensure_loaded(i);
        }
    });
    // The strip is put where the reader was once its chapter is on the screen.
    use_effect(move || {
        let Some((key, frac)) = ctx.restore.read().clone() else { return };
        let ready = ctx
            .chapters()
            .iter()
            .position(|c| c.key == key)
            .is_some_and(|i| matches!(ctx.pages.read().get(&i), Some(Loaded::Ready(_))));
        if !ready {
            return;
        }
        let mut restore = ctx.restore;
        restore.set(None);
        if ctx.look().mode == ReadMode::Scroll {
            ctx.send(serde_json::json!({ "c": "restore", "chapter": key, "frac": frac }));
        } else {
            ctx.note_current();
            ctx.preload();
        }
    });

    let Some(d) = data() else {
        return rsx! {
            div { class: "reader show-bar", id: "reader",
                div { class: "reader-gone",
                    EmptyState {
                        title: "This series is not in your library any more",
                        message: "It was removed while you were reading it.",
                        button { class: "btn primary", id: "reader-back", "aria-label": "Back to library",
                            onclick: move |_| state.close_reader(),
                            Icon { name: "arrow-left", size: 16 } "Back"
                        }
                    }
                }
            }
        };
    };
    if d.chapters.is_empty() {
        return rsx! {
            div { class: "reader show-bar", id: "reader",
                div { class: "reader-gone",
                    EmptyState {
                        title: "Nothing downloaded to read",
                        message: "Every chapter of this series was deleted from this computer.",
                        button { class: "btn primary", id: "reader-back", "aria-label": "Back to library",
                            onclick: move |_| state.close_reader(),
                            Icon { name: "arrow-left", size: 16 } "Back"
                        }
                    }
                }
            }
        };
    }

    let look = ctx.look();
    let v = ctx.view.read().clone();
    let total = d.chapters.len();
    let current = v.chapter.min(total - 1);
    let chapter = d.chapters[current].clone();
    let loaded = ctx.pages.read().get(&current).cloned();
    let mode_name = if look.mode == ReadMode::Scroll { "scroll" } else { "paged" };
    let fit_name = match look.fit {
        ReadFit::Width => "width",
        ReadFit::Height => "height",
        ReadFit::Original => "original",
    };
    let dir_name = if look.direction == PageDirection::RightToLeft { "rtl" } else { "ltr" };
    let zoom = f32::from(look.zoom) / 100.0;
    let items: Vec<(usize, String, bool)> = d
        .chapters
        .iter()
        .enumerate()
        .map(|(i, c)| (i, logic::chapter_label(c), d.read.contains(&c.key)))
        .collect();
    let label = logic::chapter_label(&chapter);
    let series_for_images = open.series_id.clone();
    let show_banner = !d.prefs.hide_small_banner;
    let banner_keys = banner.read().clone().filter(|_| show_banner);
    let next_label = (current + 1 < total).then(|| logic::chapter_label(&d.chapters[current + 1]));

    rsx! {
        div {
            class: "reader show-bar", id: "reader",
            "data-mode": "{mode_name}", "data-fit": "{fit_name}", "data-dir": "{dir_name}",
            style: "--zoom: {zoom};",
            ReaderBar {
                title: d.title.clone(), chapter_label: label.clone(), items, current, look,
                pos: ctx.pos, fullscreen: *ctx.fullscreen.read(),
                on_back: move |_| state.close_reader(),
                on_chapter: move |i: usize| ctx.open_chapter(i),
                on_mode: move |m: ReadMode| ctx.set_mode(m),
                on_direction: move |dir: PageDirection| ctx.set_prefs(|p| p.direction = Some(dir)),
                on_fit: move |fit: ReadFit| ctx.set_prefs(|p| p.fit = Some(fit)),
                on_zoom: move |up: bool| ctx.set_prefs(|p| p.zoom = Some(logic::zoom_step(look.zoom, up))),
                on_fullscreen: move |_| ctx.act(Action::Fullscreen),
            }
            main { class: "reader-stage",
                {match loaded {
                    None | Some(Loaded::Loading) => rsx! { Loading { label: "Opening {label}…" } },
                    Some(Loaded::Failed) => rsx! {
                        div { class: "reader-end",
                            h2 { "{label} could not be opened" }
                            p { class: "muted", "Its files could not be read from this computer." }
                            button { class: "btn", onclick: move |_| {
                                    let mut pages = ctx.pages;
                                    pages.write().remove(&current);
                                    ctx.ensure_loaded(current);
                                },
                                Icon { name: "rotate-cw", size: 16 } "Try again"
                            }
                        }
                    },
                    Some(Loaded::Ready(listed)) if look.mode == ReadMode::Paged => {
                        let count = listed.pages.len();
                        let page = v.page.min(count);
                        rsx! {
                            if count == 0 {
                                EndCard {
                                    title: "{label} has no pages", note: "Its pages are missing from this computer. You can download it again.",
                                    next: next_label.clone(), on_next: move |_| ctx.open_chapter(current + 1), on_back: move |_| state.close_reader(),
                                }
                            } else if page >= count {
                                EndCard {
                                    title: "End of {label}", note: if next_label.is_some() { "This chapter is marked as read." } else { "That was the last downloaded chapter." },
                                    next: next_label.clone(), on_next: move |_| ctx.open_chapter(current + 1), on_back: move |_| state.close_reader(),
                                }
                            } else {
                                div { class: "reader-page-wrap", key: "{listed.key}-{page}",
                                    PageImage {
                                        series_id: series_for_images.clone(), chapter: listed.key.clone(), version: listed.version.clone(),
                                        index: page, page: listed.pages[page].clone(), failed: ctx.failed, retries: ctx.retries,
                                    }
                                }
                            }
                        }
                    }
                    Some(Loaded::Ready(_)) => rsx! {
                        div { class: "reader-strip",
                            for index in v.strip.iter().copied().filter(|i| *i < total) {
                                {
                                    let listed = ctx.pages.read().get(&index).cloned();
                                    let c = d.chapters[index].clone();
                                    let chapter_name = logic::chapter_label(&c);
                                    rsx! {
                                        section { class: "reader-chapter", key: "{c.key}", "data-chapter": "{c.key}",
                                            if index != v.strip[0] {
                                                div { class: "reader-divider", "{chapter_name}" }
                                            }
                                            {match listed {
                                                Some(Loaded::Ready(p)) => rsx! {
                                                    if p.pages.is_empty() {
                                                        div { class: "reader-divider", "{chapter_name} has no pages" }
                                                    }
                                                    for (n, page) in p.pages.iter().enumerate() {
                                                        PageImage {
                                                            key: "{p.key}-{n}",
                                                            series_id: series_for_images.clone(), chapter: p.key.clone(), version: p.version.clone(),
                                                            index: n, page: page.clone(), failed: ctx.failed, retries: ctx.retries,
                                                        }
                                                    }
                                                },
                                                Some(Loaded::Failed) => rsx! { div { class: "reader-divider", "{chapter_name} could not be opened" } },
                                                _ => rsx! { div { class: "reader-divider", "Opening {chapter_name}…" } },
                                            }}
                                        }
                                    }
                                }
                            }
                            {
                                let last = v.strip.last().copied().unwrap_or(current).min(total - 1);
                                let more = (last + 1 < total).then(|| logic::chapter_label(&d.chapters[last + 1]));
                                rsx! {
                                    EndCard {
                                        title: "End of {logic::chapter_label(&d.chapters[last])}",
                                        note: if more.is_some() { "" } else { "That was the last downloaded chapter." },
                                        next: more, on_next: move |_| ctx.open_chapter(last + 1), on_back: move |_| state.close_reader(),
                                    }
                                }
                            }
                        }
                    },
                }}
            }
            if let Some(keys) = banner_keys {
                div { class: "banner warn reader-banner", id: "reader-banner",
                    Icon { name: "info", size: 16 }
                    div { class: "grow", "{logic::SMALL_PAGES_BANNER}" }
                    button { class: "btn small primary", id: "reader-banner-download",
                        onclick: move |_| {
                            let count = keys.len();
                            crate::ui::work::redownload_original(state, &ctx.series_id(), keys.clone());
                            banner.set(None);
                            state.notify(format!("Downloading {} again in original quality…", logic::plural(count, "chapter")));
                        },
                        Icon { name: "download", size: 15 } "Download again"
                    }
                    button { class: "btn small ghost", id: "reader-banner-dismiss", title: "Close this note", "aria-label": "Dismiss",
                        onclick: move |_| ctx.set_prefs(|p| p.hide_small_banner = true),
                        Icon { name: "x", size: 15 }
                    }
                }
            }
        }
    }
}

/// The note at the end of a chapter, with the way on.
#[component]
fn EndCard(
    title: String,
    note: String,
    next: Option<String>,
    on_next: EventHandler<()>,
    on_back: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "reader-end",
            h2 { "{title}" }
            if !note.is_empty() { p { class: "muted", "{note}" } }
            if let Some(next) = next {
                button { class: "btn primary", id: "reader-next-chapter", "aria-label": "Next chapter",
                    onclick: move |_| on_next.call(()),
                    "Next chapter · {next}" Icon { name: "arrow-right", size: 16 }
                }
            } else {
                button { class: "btn", id: "reader-end-back", onclick: move |_| on_back.call(()),
                    Icon { name: "arrow-left", size: 16 } "Back to your library"
                }
            }
        }
    }
}

/// One page: the picture, or a tile that says it could not be shown and lets
/// the reader try again. A page that fails never takes the screen with it.
#[component]
fn PageImage(
    series_id: String,
    chapter: String,
    version: String,
    index: usize,
    page: PageInfo,
    failed: Signal<HashSet<(String, usize)>>,
    retries: Signal<HashMap<(String, usize), u32>>,
) -> Element {
    let key = (chapter.clone(), index);
    let broken = failed.read().contains(&key);
    let attempt = retries.read().get(&key).copied().unwrap_or(0);
    let ratio = if page.w > 0 && page.h > 0 { format!("{} / {}", page.w, page.h) } else { "2 / 3".to_string() };
    let style = format!("--ar: {ratio}; --w: {}px;", page.w);
    let src = format!("{}&r={attempt}", crate::pages::url(&series_id, &chapter, &page.file, &version));
    let number = index + 1;
    let (on_error, on_retry) = (key.clone(), key);
    rsx! {
        div { class: if broken { "reader-tile failed" } else { "reader-tile" }, style: "{style}", "data-page": "{index}",
            if broken {
                div { class: "reader-fail",
                    Icon { name: "triangle-alert", size: 20 }
                    span { "Page {number} could not be shown." }
                    button { class: "btn small retry", "aria-label": "Retry page {number}",
                        onclick: move |e| {
                            e.stop_propagation();
                            retries.write().entry(on_retry.clone()).and_modify(|n| *n += 1).or_insert(1);
                            failed.write().remove(&on_retry);
                        },
                        Icon { name: "rotate-cw", size: 14 } "Retry"
                    }
                }
            } else {
                img {
                    src: "{src}", alt: "Page {number}", draggable: "false", decoding: "async",
                    onerror: move |_| { failed.write().insert(on_error.clone()); },
                }
            }
        }
    }
}

/// The page counter, apart from the rest of the bar so scrolling redraws only it.
#[component]
fn PageCounter(pos: Signal<Pos>) -> Element {
    let p = pos();
    let shown = p.page + 1;
    rsx! {
        span { class: "reader-counter", id: "reader-page-counter", "aria-label": "Page", "aria-live": "polite",
            if p.pages == 0 { "–" } else { "{shown} / {p.pages}" }
        }
    }
}

/// The class of a button of a group: lit when it is the one chosen.
fn on(yes: bool) -> &'static str {
    if yes {
        "on"
    } else {
        ""
    }
}

/// The bar across the top: it hides while reading and shows when the pointer moves.
#[component]
fn ReaderBar(
    title: String,
    chapter_label: String,
    items: Vec<(usize, String, bool)>,
    current: usize,
    look: ReaderLook,
    pos: Signal<Pos>,
    fullscreen: bool,
    on_back: EventHandler<()>,
    on_chapter: EventHandler<usize>,
    on_mode: EventHandler<ReadMode>,
    on_direction: EventHandler<PageDirection>,
    on_fit: EventHandler<ReadFit>,
    on_zoom: EventHandler<bool>,
    on_fullscreen: EventHandler<()>,
) -> Element {
    let mut menu = use_signal(|| false);
    let screen_icon = if fullscreen { "minimize" } else { "maximize" };
    rsx! {
        header { class: "reader-bar",
            button { class: "btn", id: "reader-back", "aria-label": "Back to library", title: "Back (Esc)",
                onclick: move |_| on_back.call(()),
                Icon { name: "arrow-left", size: 16 } "Back"
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
                                onclick: move |_| { menu.set(false); on_chapter.call(index); },
                                span { class: "grow", "{name}" }
                                if read { span { class: "reader-read", title: "Read", Icon { name: "check", size: 14 } } }
                            }
                        }
                    }
                }
            }
            PageCounter { pos }
            div { class: "seg", role: "group", "aria-label": "View mode",
                button { id: "reader-mode-paged", class: on(look.mode == ReadMode::Paged), "aria-label": "Paged", "aria-pressed": "{look.mode == ReadMode::Paged}",
                    title: "One page at a time", onclick: move |_| on_mode.call(ReadMode::Paged), "Paged" }
                button { id: "reader-mode-scroll", class: on(look.mode == ReadMode::Scroll), "aria-label": "Scroll", "aria-pressed": "{look.mode == ReadMode::Scroll}",
                    title: "One long strip", onclick: move |_| on_mode.call(ReadMode::Scroll), "Scroll" }
            }
            if look.mode == ReadMode::Paged {
                div { class: "seg", role: "group", "aria-label": "Reading direction",
                    button { id: "reader-dir-ltr", class: on(look.direction == PageDirection::LeftToRight), "aria-label": "Left to right",
                        "aria-pressed": "{look.direction == PageDirection::LeftToRight}",
                        onclick: move |_| on_direction.call(PageDirection::LeftToRight), "Left to right" }
                    button { id: "reader-dir-rtl", class: on(look.direction == PageDirection::RightToLeft), "aria-label": "Right to left",
                        "aria-pressed": "{look.direction == PageDirection::RightToLeft}",
                        onclick: move |_| on_direction.call(PageDirection::RightToLeft), "Right to left" }
                }
            }
            div { class: "seg", role: "group", "aria-label": "Fit",
                button { id: "reader-fit-width", class: on(look.fit == ReadFit::Width), "aria-label": "Fit width", "aria-pressed": "{look.fit == ReadFit::Width}",
                    onclick: move |_| on_fit.call(ReadFit::Width), "Width" }
                button { id: "reader-fit-height", class: on(look.fit == ReadFit::Height), "aria-label": "Fit height", "aria-pressed": "{look.fit == ReadFit::Height}",
                    onclick: move |_| on_fit.call(ReadFit::Height), "Height" }
                button { id: "reader-fit-original", class: on(look.fit == ReadFit::Original), "aria-label": "Original size", "aria-pressed": "{look.fit == ReadFit::Original}",
                    onclick: move |_| on_fit.call(ReadFit::Original), "Original" }
            }
            div { class: "reader-zoom",
                button { class: "btn ghost", id: "reader-zoom-out", "aria-label": "Zoom out", title: "Zoom out ( - )",
                    onclick: move |_| on_zoom.call(false), Icon { name: "zoom-out", size: 16 } }
                span { id: "reader-zoom-level", "aria-label": "Zoom", "{look.zoom}%" }
                button { class: "btn ghost", id: "reader-zoom-in", "aria-label": "Zoom in", title: "Zoom in ( + )",
                    onclick: move |_| on_zoom.call(true), Icon { name: "zoom-in", size: 16 } }
            }
            button { class: "btn ghost", id: "reader-fullscreen", "aria-label": "Full screen", title: "Full screen (F11)",
                onclick: move |_| on_fullscreen.call(()),
                Icon { name: screen_icon, size: 16 }
            }
        }
    }
}

/// The pages of a chapter as the Export screen's preview shows them for a phone
/// or tablet: the address of each and its shape. Blocks; for a thread of its own.
pub(crate) fn original_pages(series_id: &str, chapter: &DownloadedChapter) -> Vec<(String, String)> {
    let listed = load::list_pages(&crate::state::library_files(), series_id, chapter);
    listed
        .pages
        .iter()
        .map(|page| {
            let ratio = if page.w > 0 && page.h > 0 { format!("{} / {}", page.w, page.h) } else { "2 / 3".to_string() };
            (crate::pages::url(series_id, &listed.key, &page.file, &listed.version), ratio)
        })
        .collect()
}
