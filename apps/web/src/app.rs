//! The page: its state, the loop that listens to the browser, and the shell
//! (navigation, dialogs, toasts) around the screens.

use crate::art;
use crate::bridge::{self, Caps, Event};
use crate::format::plural;
use crate::model::{Book, Library};
use crate::review::{Failure, Review};
use crate::ui::{self, widgets::Icon};
use dioxus::prelude::*;
use mangapoint_ui::dialogs::{ToastHost, UnsavedHost};
use mangapoint_ui::host::{Ask, Host, Leave, LocalFuture, SharedAction, UiState};
use mangapoint_ui::shell::BarItem;
use serde_json::json;
use std::rc::Rc;
use web_engine::state::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Library,
    /// A book's overview (the book is `Ctx::book_page`).
    Book,
    Export,
    Settings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Boot {
    Starting,
    Ready,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportStage {
    Idle,
    Analyzing(String),
    Review,
    Keeping { title: String, done: usize, total: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportState {
    pub stage: ImportStage,
    pub reviews: Vec<Review>,
    pub failures: Vec<Failure>,
    /// The book the chapters go into (its overview's Import more chapters); none for new books.
    pub into: Option<String>,
}

impl Default for ImportState {
    fn default() -> Self {
        Self { stage: ImportStage::Idle, reviews: Vec::new(), failures: Vec::new(), into: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModalAction {
    /// Books and places waiting for their file, by id, taken out of the library with their reading state.
    Forget(Vec<String>),
    /// The files of these books deleted; the books stay in the library, waiting for their file.
    RemoveDownloads(Vec<String>),
    ClearAll,
    DeletePreset(String),
    /// Take a reader off the list.
    RemoveReader { id: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modal {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
    pub danger: bool,
    pub action: ModalAction,
}

/// An open reader.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub book_id: String,
    pub chapter: usize,
    /// The page at the top of the strip.
    pub page: usize,
    pub scroll: f32,
    pub bar: bool,
    /// Counts up so the screen starts fresh for every open.
    pub opened: u64,
    /// Ask the browser to scroll to this page of the book once it is drawn.
    pub scroll_to: Option<(usize, f32)>,
}

/// Everything the screens share.
#[derive(Clone, Copy)]
pub struct Ctx {
    /// What the website shares with the desktop app: the settings with the user's readers (kept
    /// in this browser), the toasts, the reader form's unsaved changes (see `mangapoint_ui::host`).
    pub ui: UiState,
    /// The visitor's choices for exports and the page, as the reading-state file keeps them.
    pub choices: Signal<Settings>,
    pub boot: Signal<Boot>,
    pub library: Signal<Library>,
    pub caps: Signal<Caps>,
    pub page: Signal<Page>,
    pub reading: Signal<Option<Reading>>,
    pub import: Signal<ImportState>,
    pub modal: Signal<Option<Modal>>,
    pub online: Signal<bool>,
    pub update_ready: Signal<bool>,
    pub offline_ready: Signal<bool>,
    pub can_install: Signal<bool>,
    pub export: Signal<ui::export::ExportUi>,
    /// The presets of the user's own (and changes to the built-in ones), kept in the browser.
    pub presets: Signal<library::PresetsDb>,
    /// How each book was last exported, by book id.
    pub last_exports: Signal<std::collections::BTreeMap<String, ui::export::LastExport>>,
    /// The window is as narrow as a phone's.
    pub narrow: Signal<bool>,
    /// The address bar has been read: from now on it follows the screen.
    pub routed: Signal<bool>,
    /// Which part of Settings is open.
    pub settings_section: Signal<ui::settings::Section>,
    /// The book whose overview is open (`Page::Book`).
    pub book_page: Signal<String>,
    pub next_id: Signal<u64>,
    pub state_file: Signal<Option<String>>,
}

impl Ctx {
    pub fn id(mut self) -> u64 {
        let n = *self.next_id.peek() + 1;
        self.next_id.set(n);
        n
    }

    /// Go to a page. Unsaved reader settings are asked about first.
    pub fn go(self, page: Page) {
        let go = move || {
            let mut p = self.page;
            p.set(page);
        };
        self.ui.leave(Leave::Elsewhere(Rc::new(go)), page == Page::Settings);
    }

    /// Open a part of Settings. Unsaved reader settings are asked about first.
    pub fn open_section(self, section: ui::settings::Section) {
        let go = move || {
            let mut current = self.settings_section;
            current.set(section);
        };
        self.ui.leave(Leave::Elsewhere(Rc::new(go)), section == ui::settings::Section::Readers);
    }

    pub fn notify(self, text: impl Into<String>) {
        self.ui.notify(text);
    }

    pub fn warn(self, text: impl Into<String>) {
        self.ui.warn(text);
    }

    pub fn error(self, text: impl Into<String>) {
        self.ui.error(text);
    }

    pub fn ask(mut self, modal: Modal) {
        self.modal.set(Some(modal));
    }

    // ---- keeping records ----

    /// Write the reading state of a book (the browser merges quick repeats).
    pub fn save_state_of(self, id: &str) {
        if let Some(s) = self.library.peek().state_of(id) {
            bridge::put_soon("state", id, s);
        }
    }

    /// Keep the settings with the readers in this browser.
    pub fn save_readers(self) {
        bridge::put_soon("meta", "readers", &*self.ui.settings.peek());
    }

    pub fn save_presets(self) {
        bridge::put_soon("meta", "presets", &*self.presets.peek());
    }

    pub fn save_last_exports(self) {
        bridge::put_soon("meta", "last_exports", &*self.last_exports.peek());
    }

    pub fn save_settings(self) {
        bridge::put_soon("meta", "settings", &*self.choices.peek());
    }

    pub fn book(self, id: &str) -> Option<Book> {
        if id == ui::export::SAMPLE {
            return Some(ui::export::sample_book());
        }
        self.library.peek().find(id).cloned()
    }
}

/// The website's side of the shared screens (`mangapoint_ui::host::Host`).
struct WebHost(Ctx);

impl Host for WebHost {
    fn sleep(&self, ms: u64) -> LocalFuture<()> {
        Box::pin(bridge::sleep(ms.min(u64::from(u32::MAX)) as u32))
    }

    fn save_settings(&self) {
        self.0.save_readers();
    }

    fn use_reader(&self, id: &str) {
        ui::export::use_reader(self.0, id);
    }

    fn reader_saved(&self) {
        ui::export::reader_changed(self.0);
    }

    fn open_readers(&self) {
        let mut section = self.0.settings_section;
        section.set(ui::settings::Section::Readers);
        self.0.go(Page::Settings);
    }

    fn ask(&self, ask: Ask) {
        let action = match ask.action {
            SharedAction::RemoveReader { id } => ModalAction::RemoveReader { id },
        };
        self.0.ask(Modal { title: ask.title, body: ask.body, confirm: ask.confirm, cancel: ask.cancel, danger: ask.danger, action });
    }

    fn run_script(&self, name: &str, args: String) {
        let args = serde_json::from_str(&args).unwrap_or(json!({}));
        bridge::send("run_script", json!({ "name": name, "file": mangapoint_ui::scripts::file_name(name), "args": args }));
    }
}

pub fn current_iso() -> String {
    crate::model::iso_of_ms(bridge::now_ms())
}

#[component]
pub fn BrowserApp() -> Element {
    let ui_state = use_hook(|| UiState::new(library::Settings::default()));
    let ctx = Ctx {
        ui: ui_state,
        boot: use_signal(|| Boot::Starting),
        library: use_signal(Library::default),
        choices: use_signal(Settings::default),
        caps: use_signal(Caps::default),
        page: use_signal(|| Page::Library),
        reading: use_signal(|| None),
        import: use_signal(ImportState::default),
        modal: use_signal(|| None),
        online: use_signal(|| true),
        update_ready: use_signal(|| false),
        offline_ready: use_signal(|| false),
        can_install: use_signal(|| false),
        export: use_signal(ui::export::ExportUi::default),
        presets: use_signal(library::PresetsDb::default),
        last_exports: use_signal(Default::default),
        narrow: use_signal(|| false),
        routed: use_signal(|| false),
        settings_section: use_signal(|| ui::settings::Section::Library),
        book_page: use_signal(String::new),
        next_id: use_signal(|| 0),
        state_file: use_signal(|| None),
    };
    use_context_provider(|| ctx);
    use_context_provider(|| ctx.ui);
    use_hook(|| ctx.ui.set_host(Rc::new(WebHost(ctx))));

    // Start: read what this browser keeps, then listen for what it says.
    use_future(move || async move {
        let boot: Result<bridge::Boot, String> = bridge::call("boot", json!({})).await;
        match boot {
            Ok(b) => {
                let mut library = ctx.library;
                let mut caps = ctx.caps;
                let mut settings = ctx.choices;
                let mut boot_state = ctx.boot;
                {
                    let mut lib = library.write();
                    lib.books = b.books;
                    lib.states = b.states.into_iter().map(|s| (s.fingerprint.clone(), s)).collect();
                }
                if let Some(s) = b.meta.get("settings") {
                    if let Ok(s) = serde_json::from_value::<Settings>(s.clone()) {
                        settings.set(s);
                    }
                }
                {
                    let mut readers = ctx.ui.settings;
                    let saved = b.meta.get("readers").and_then(|v| serde_json::from_value::<library::Settings>(v.clone()).ok());
                    let r = saved.unwrap_or_else(first_visit_readers);
                    readers.set(r);
                }
                if let Some(p) = b.meta.get("presets") {
                    if let Ok(p) = serde_json::from_value::<library::PresetsDb>(p.clone()) {
                        let mut presets = ctx.presets;
                        presets.set(p);
                    }
                }
                if let Some(l) = b.meta.get("last_exports") {
                    if let Ok(l) = serde_json::from_value(l.clone()) {
                        let mut last = ctx.last_exports;
                        last.set(l);
                    }
                }
                caps.set(b.caps);
                if let Some(e) = b.error {
                    ctx.error(e);
                }
                boot_state.set(Boot::Ready);
                // Which screen the address bar names (a book that was being read, Export, ...).
                bridge::send("route_ready", json!({}));
            }
            Err(e) => {
                let mut boot_state = ctx.boot;
                boot_state.set(Boot::Failed(e));
            }
        }
        loop {
            for ev in bridge::next_events().await {
                handle_event(ctx, ev);
            }
        }
    });

    let boot = ctx.boot.read().clone();
    rsx! {
        {match boot {
            Boot::Starting => rsx! {
                div { class: "splash",
                    art::walker::WalkingCat {}
                    h1 { "MangaPoint" }
                    p { class: "muted", "Opening your library\u{2026}" }
                }
            },
            Boot::Failed(message) => rsx! {
                div { class: "splash",
                    h1 { "Something went wrong" }
                    div { class: "banner bad", "{message}" }
                    button { class: "btn primary", onclick: move |_| { let _ = web_sys_reload(); }, "Try again" }
                }
            },
            Boot::Ready => rsx! { BrowserShell {} },
        }}
        WebConfirmation {}
        UnsavedHost {}
        ui::import::BrowserImportHost {}
        ToastHost {}
        DropOverlay {}
    }
}

fn web_sys_reload() -> Result<(), String> {
    bridge::send("reload", json!({}));
    Ok(())
}

fn handle_event(ctx: Ctx, ev: Event) {
    match ev {
        Event::Analyzing { status } => {
            let mut imp = ctx.import;
            let mut i = imp.write();
            if i.stage != ImportStage::Review {
                i.stage = ImportStage::Analyzing(status);
            }
        }
        Event::Import { drafts, failures, into } => ui::import::analysis_done(ctx, drafts, failures, into),
        Event::CommitProgress { done, total } => {
            let mut imp = ctx.import;
            let mut guard = imp.write();
            if let ImportStage::Keeping { done: d, total: t, .. } = &mut guard.stage {
                *d = done;
                *t = total;
            }
        }
        Event::StateFile { name, text } => ui::settings::state_file_loaded(ctx, name, text),
        Event::Route { path, hash } => apply_route(ctx, &path, &hash),
        Event::Key { .. } | Event::Tap { .. } | Event::Zoom { .. } | Event::Wheel { .. } | Event::Pos { .. } | Event::Scrolled { .. } | Event::EdgeBack => {
            ui::reader::handle_reader_event(ctx, ev)
        }
        Event::ExportStatus { .. }
        | Event::ExportProgress { .. }
        | Event::ExportDone { .. }
        | Event::ExportError { .. }
        | Event::ExportCancelled => ui::export::handle_export_event(ctx, ev),
        Event::UpdateReady => {
            let mut s = ctx.update_ready;
            s.set(true);
        }
        Event::OfflineReady => {
            let mut s = ctx.offline_ready;
            s.set(true);
        }
        Event::Editor { current, .. } => ui::export::scrolled(ctx, current),
        Event::Narrow { narrow } => {
            let mut s = ctx.narrow;
            s.set(narrow);
        }
        Event::Online { online } => {
            let mut s = ctx.online;
            s.set(online);
        }
        Event::CanInstall => {
            let mut s = ctx.can_install;
            s.set(true);
        }
        Event::Installed => {
            let mut s = ctx.can_install;
            s.set(false);
            ctx.notify("MangaPoint is installed.");
        }
        Event::Other => {}
    }
}

// ---- the shell ----------------------------------------------------------------------------------

/// A press on the dock or the corner bar: the page fades out first, then the other comes in.
fn pick_with_exit(ctx: Ctx, id: &'static str, leaving: Signal<bool>) {
    let mut leaving = leaving;
    let current = *ctx.page.peek();
    let same = matches!((current, id), (Page::Library | Page::Book, "library") | (Page::Export, "export") | (Page::Settings, "settings"));
    if leaving() || same || current == Page::Export || id == "export" {
        return pick(ctx, id);
    }
    leaving.set(true);
    spawn(async move {
        bridge::sleep(340).await;
        pick(ctx, id);
        leaving.set(false);
    });
}

fn pick(ctx: Ctx, id: &str) {
    ctx.go(match id {
        "library" => Page::Library,
        "export" => Page::Export,
        _ => Page::Settings,
    });
}

#[component]
fn BrowserShell() -> Element {
    let ctx = use_context::<Ctx>();
    let page = *ctx.page.read();
    let reading = ctx.reading.read().is_some();
    let online = *ctx.online.read();
    let update = *ctx.update_ready.read();
    let mut later = use_signal(|| false);
    // The page being left fades before the next one comes in.
    let leaving = use_signal(|| false);
    // A book's overview belongs to the Library.
    let current = match page {
        Page::Book => Page::Library,
        p => p,
    };
    // The address bar follows the screen: /, /export, /settings#reader, /read/<book>/<chapter>.
    use_effect(move || {
        if !*ctx.routed.read() {
            return;
        }
        let (path, hash) = route_of(ctx);
        bridge::send("route", json!({ "path": path, "hash": hash }));
    });
    rsx! {
        div { class: "app", "data-motion": "clean", "data-reading": "{reading}",
            if !reading {
            mangapoint_ui::shell::Dock {
                items: vec![
                    BarItem::new("library", "Library", "nav-library", current == Page::Library),
                    BarItem::new("export", "Export", "nav-export", current == Page::Export),
                ],
                on_pick: move |id| pick_with_exit(ctx, id, leaving),
            }
            mangapoint_ui::shell::CornerBar {
                items: vec![BarItem::new("settings", "Settings", "gear", current == Page::Settings)],
                on_pick: move |id| pick_with_exit(ctx, id, leaving),
            }
            }
            main { class: "content", id: "content",
                div { class: if leaving() { "page leaving" } else { "page" },
                    if !online {
                        div { class: "banner small offline-note",
                            Icon { name: "info", size: 16 }
                            div { "You are offline. Your library still works." }
                        }
                    }
                    {match page {
                        Page::Library => rsx! { ui::library::BrowserLibraryPage {} },
                        Page::Book => rsx! { ui::book::BookPage {} },
                        Page::Export => rsx! { ui::export::BrowserExportPage {} },
                        Page::Settings => rsx! { ui::settings::BrowserSettingsPage {} },
                    }}
                }
            }
            if reading {
                ui::reader::ReaderHost {}
            }
            if update && !*later.read() {
                div { class: "overlay", id: "update-dialog", onclick: move |_| later.set(true),
                    div { class: "modal update-modal", onclick: move |e| e.stop_propagation(),
                        div { class: "upd-top", crate::art::update_cat::UpdateCat {} }
                        h2 { "A new version of MangaPoint is ready" }
                        p { class: "muted small", style: "margin: 0 0 10px", "Reload to use it. Your library and settings are not touched." }
                        div { class: "actions",
                            button { class: "btn", id: "update-later", onclick: move |_| later.set(true), "Later" }
                            button { class: "btn primary", id: "update-now", onclick: move |_| bridge::send("apply_update", json!({})),
                                Icon { name: "refresh-cw", size: 15 }
                                "Update and reload"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WebConfirmation() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(modal) = ctx.modal.read().clone() else { return rsx! {} };
    rsx! {
        mangapoint_ui::dialogs::Confirmation {
            title: modal.title, body: modal.body, confirm: modal.confirm, cancel: modal.cancel,
            danger: modal.danger, dismiss_outside: true,
            on_cancel: move |_| { let mut m = ctx.modal; m.set(None); },
            on_confirm: move |_| {
                let mut m = ctx.modal;
                m.set(None);
                run_action(ctx, modal.action.clone());
            },
        }
    }
}

fn run_action(ctx: Ctx, action: ModalAction) {
    dioxus::core::spawn_forever(async move {
        match action {
            ModalAction::RemoveDownloads(ids) => {
                // The files go; the books stay, waiting for their file.
                let mut removed = Vec::new();
                for id in ids {
                    let Some(book) = ctx.book(&id) else { continue };
                    if ctx.reading.peek().as_ref().is_some_and(|r| r.book_id == id) {
                        ui::reader::close_reader(ctx);
                    }
                    if let Err(e) = bridge::call_ok("remove_book", json!({ "id": id })).await {
                        ctx.error(format!("Could not remove the download of {}: {e}", book.title));
                        continue;
                    }
                    let mut lib = ctx.library;
                    lib.write().keep_place(&book);
                    ctx.save_state_of(&id);
                    lib.write().remove_book(&id, false);
                    removed.push(book.title);
                }
                match removed.as_slice() {
                    [] => {}
                    [title] => ctx.notify(format!("Removed the download of {title}.")),
                    many => ctx.notify(format!("Removed the downloads of {}. They stay in your library.", plural(many.len(), "book"))),
                }
            }
            ModalAction::Forget(ids) => {
                // Books here and places waiting for their file alike: gone, with their reading state.
                let mut forgotten = Vec::new();
                for id in ids {
                    let title = ctx.book(&id).map(|b| b.title).or_else(|| ctx.library.peek().state_of(&id).map(|s| s.title.clone())).unwrap_or_default();
                    if ctx.book(&id).is_some() {
                        if ctx.reading.peek().as_ref().is_some_and(|r| r.book_id == id) {
                            ui::reader::close_reader(ctx);
                        }
                        if let Err(e) = bridge::call_ok("remove_book", json!({ "id": id })).await {
                            ctx.error(format!("Could not forget {title}: {e}"));
                            continue;
                        }
                        let mut lib = ctx.library;
                        lib.write().remove_book(&id, true);
                    } else {
                        let mut lib = ctx.library;
                        lib.write().forget_state(&id);
                    }
                    let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
                    forgotten.push(title);
                }
                match forgotten.as_slice() {
                    [] => {}
                    [title] => ctx.notify(format!("Forgot {title}.")),
                    many => ctx.notify(format!("Forgot {}.", plural(many.len(), "book"))),
                }
            }
            ModalAction::DeletePreset(id) => {
                let mut presets = ctx.presets;
                let result = presets.write().delete(&id);
                match result {
                    Ok(library::Deleted::Removed) => {
                        ctx.save_presets();
                        ctx.notify("Preset deleted.");
                    }
                    Ok(library::Deleted::Reset) => {
                        ctx.save_presets();
                        ctx.notify("Back to the original settings.");
                    }
                    Err(e) => ctx.error(e.to_string()),
                }
                ui::export::forget_preset(ctx, &id);
            }
            ModalAction::RemoveReader { id } => {
                let mut readers = ctx.ui.settings;
                {
                    let mut st = readers.write();
                    st.custom_devices.retain(|d| d.id != id);
                    st.device_presets.remove(&id);
                    st.reader_setups.remove(&id);
                    st.device = st.custom_devices.first().map(|d| d.id.clone()).unwrap_or_default();
                }
                ctx.save_readers();
                ui::export::reader_changed(ctx);
            }
            ModalAction::ClearAll => {
                ui::reader::close_reader(ctx);
                match bridge::call_ok("clear_all", json!({})).await {
                    Ok(()) => {
                        let mut lib = ctx.library;
                        *lib.write() = Library::default();
                        let mut settings = ctx.choices;
                        settings.set(Settings::default());
                        let mut readers = ctx.ui.settings;
                        readers.set(first_visit_readers());
                        let mut sf = ctx.state_file;
                        sf.set(None);
                        ctx.notify("Everything MangaPoint kept on this device was deleted.");
                    }
                    Err(e) => ctx.error(format!("Could not delete everything: {e}")),
                }
            }
        }
    });
}

/// Shown while files are dragged over the page.
#[component]
fn DropOverlay() -> Element {
    rsx! {
        div { class: "drop-overlay", "aria-hidden": "true",
            div { class: "drop-card",
                Icon { name: "file-up", size: 32 }
                strong { "Drop to import" }
                span { class: "muted", "Comics, books, folders and pictures stay on this device." }
            }
        }
    }
}

/// A first visit has no readers: the visitor adds their own. Until then pages are made for a
/// phone or tablet, as they are.
fn first_visit_readers() -> library::Settings {
    library::Settings { device: device_profiles::PHONE_ORIGINAL_ID.to_string(), ..Default::default() }
}

/// The address of the screen being shown, without the leading slash: `` (the library), `export`,
/// `settings` (with the part of Settings after `#`) or `read?book=<id>&chapter=<n>`.
fn route_of(ctx: Ctx) -> (String, String) {
    if let Some(r) = ctx.reading.read().as_ref() {
        return (format!("read?book={}&chapter={}", r.book_id, r.chapter + 1), String::new());
    }
    match *ctx.page.read() {
        Page::Library => (String::new(), String::new()),
        Page::Book => (format!("book?id={}", ctx.book_page.read()), String::new()),
        Page::Export => ("export".into(), String::new()),
        Page::Settings => ("settings".into(), ctx.settings_section.read().slug().into()),
    }
}

/// Show the screen an address names (the page was opened at it, or Back or Forward went to it).
fn apply_route(ctx: Ctx, path: &str, hash: &str) {
    let mut routed = ctx.routed;
    routed.set(true);
    let (head, query) = path.split_once('?').unwrap_or((path, ""));
    let param = |name: &str| query.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=')).unwrap_or("");
    match head {
        "export" => {
            ui::reader::close_reader(ctx);
            ctx.go(Page::Export);
        }
        "settings" => {
            ui::reader::close_reader(ctx);
            let mut section = ctx.settings_section;
            section.set(ui::settings::Section::from_slug(hash).unwrap_or(ui::settings::Section::Library));
            ctx.go(Page::Settings);
        }
        "book" => {
            ui::reader::close_reader(ctx);
            let id = param("id");
            if ctx.book(id).is_some() {
                ui::book::open_overview(ctx, id);
            } else {
                ctx.go(Page::Library);
                bridge::send("route_fix", json!({ "path": "", "hash": "" }));
            }
        }
        "read" => {
            let id = param("book");
            let chapter = param("chapter").parse::<usize>().ok().map(|n| n.saturating_sub(1));
            let usable = ctx.book(id).is_some_and(|b| !b.needs_file && !b.chapters.is_empty());
            if !usable {
                ui::reader::close_reader(ctx);
                ctx.go(Page::Library);
                // This book is not here (removed, or the address is from another browser).
                bridge::send("route_fix", json!({ "path": "", "hash": "" }));
                return;
            }
            let open_now = ctx.reading.peek().as_ref().map(|r| (r.book_id.clone(), r.chapter));
            match open_now {
                Some((book, c)) if book == id => {
                    if let Some(want) = chapter.filter(|w| *w != c) {
                        ui::reader::open_chapter(ctx, want);
                    }
                }
                _ => {
                    ctx.go(Page::Library);
                    ui::reader::open_reader(ctx, id, chapter);
                }
            }
        }
        _ => {
            ui::reader::close_reader(ctx);
            ctx.go(Page::Library);
        }
    }
}
