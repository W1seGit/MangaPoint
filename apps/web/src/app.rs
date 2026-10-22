//! The page: its state, the loop that listens to the browser, and the shell
//! (navigation, dialogs, toasts) around the screens.

use crate::art;
use crate::bridge::{self, Caps, Event};
use crate::format::plural;
use crate::model::{Book, Library};
use crate::review::{Failure, Review};
use crate::ui::{self, widgets::Icon};
use dioxus::prelude::*;
use serde_json::{json, Value};
use web_engine::state::{BookState, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Library,
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
}

impl Default for ImportState {
    fn default() -> Self {
        Self { stage: ImportStage::Idle, reviews: Vec::new(), failures: Vec::new() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub kind: ToastKind,
    pub leaving: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModalAction {
    RemoveBook { id: String, forget: bool },
    ForgetWaiting(String),
    /// Books and places, by id, taken out of the library together, with their reading state.
    ForgetMany(Vec<String>),
    /// The files of these books deleted; the books stay in the library, waiting for their file.
    RemoveDownloads(Vec<String>),
    ClearAll,
    DeletePreset(String),
    RemoveReader(String),
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
    /// Chapters drawn in the strip (scroll mode), in order.
    pub strip: Vec<usize>,
    pub bar: bool,
    /// Counts up so the screen starts fresh for every open.
    pub opened: u64,
    /// Ask the browser to scroll to this page of the book once it is drawn.
    pub scroll_to: Option<(usize, f32)>,
}

/// Everything the screens share.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub boot: Signal<Boot>,
    pub library: Signal<Library>,
    pub settings: Signal<Settings>,
    pub caps: Signal<Caps>,
    pub page: Signal<Page>,
    pub reading: Signal<Option<Reading>>,
    pub import: Signal<ImportState>,
    pub toasts: Signal<Vec<Toast>>,
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
    /// The user's readers (and the one in use), as the app keeps them in its settings: the
    /// list, each one's size, colour and file types. Kept in this browser.
    pub readers: Signal<library::Settings>,
    /// The address bar has been read: from now on it follows the screen.
    pub routed: Signal<bool>,
    /// Which part of Settings is open.
    pub settings_section: Signal<ui::settings::Section>,
    /// Writes the reader form's unsaved changes, while it has any (see [`UnsavedReader`]).
    pub unsaved_reader: Signal<Option<UnsavedReader>>,
    /// Going somewhere was held back by unsaved reader settings; the user is asked.
    pub leave_prompt: Signal<Option<Leave>>,
    pub next_id: Signal<u64>,
    pub state_file: Signal<Option<String>>,
}

/// Where the user was going when unsaved reader settings stopped them: asked
/// about, then carried on to. (The desktop app's own, `state/navigation.rs`.)
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    Page(Page),
    Section(ui::settings::Section),
    /// Another reader, by id.
    Reader(String),
}

/// Writes the reader form's unsaved changes; true if they were saved. Present
/// only while the form has changes that are not saved.
#[derive(Clone)]
pub struct UnsavedReader(pub std::rc::Rc<dyn Fn() -> bool>);

impl Ctx {
    pub fn id(mut self) -> u64 {
        let n = *self.next_id.peek() + 1;
        self.next_id.set(n);
        n
    }

    /// Go to a page. Unsaved reader settings are asked about first.
    pub fn go(self, page: Page) {
        self.leave(Leave::Page(page));
    }

    /// Go somewhere that would lose the reader form's unsaved changes, asking
    /// what to do with them first; anywhere else, just go.
    pub fn leave(self, to: Leave) {
        let staying = match &to {
            Leave::Page(page) => *page == Page::Settings,
            Leave::Section(section) => *section == ui::settings::Section::Readers,
            Leave::Reader(_) => false,
        };
        if !staying && self.unsaved_reader.peek().is_some() {
            let mut prompt = self.leave_prompt;
            prompt.set(Some(to));
            return;
        }
        self.leave_now(to);
    }

    /// Carry on to where the user was going, the changes dealt with.
    pub fn leave_now(self, to: Leave) {
        match to {
            Leave::Page(page) => {
                let mut p = self.page;
                p.set(page);
            }
            Leave::Section(section) => {
                let mut current = self.settings_section;
                current.set(section);
            }
            Leave::Reader(id) => ui::export::use_reader(self, &id),
        }
    }

    pub fn toast(self, kind: ToastKind, text: impl Into<String>) {
        let id = self.id();
        let mut toasts = self.toasts;
        toasts.write().push(Toast { id, text: text.into(), kind, leaving: false });
        let secs = if kind == ToastKind::Error { 9000 } else { 5000 };
        dioxus::core::spawn_forever(async move {
            bridge::sleep(secs).await;
            self.dismiss_toast(id);
        });
    }

    pub fn notify(self, text: impl Into<String>) {
        self.toast(ToastKind::Success, text);
    }

    pub fn warn(self, text: impl Into<String>) {
        self.toast(ToastKind::Warning, text);
    }

    pub fn error(self, text: impl Into<String>) {
        self.toast(ToastKind::Error, text);
    }

    pub fn dismiss_toast(self, id: u64) {
        let mut toasts = self.toasts;
        if let Some(t) = toasts.write().iter_mut().find(|t| t.id == id) {
            t.leaving = true;
        }
        dioxus::core::spawn_forever(async move {
            bridge::sleep(360).await;
            let mut toasts = self.toasts;
            toasts.write().retain(|t| t.id != id);
        });
    }

    pub fn ask(mut self, modal: Modal) {
        self.modal.set(Some(modal));
    }

    // ---- keeping records ----

    pub fn save_book(self, book: &Book) {
        bridge::put_soon("books", &book.id, book);
    }

    /// Write the reading state of a book (the browser merges quick repeats).
    pub fn save_state_of(self, id: &str) {
        if let Some(s) = self.library.peek().state_of(id) {
            bridge::put_soon("state", id, s);
        }
    }

    pub fn save_readers(self) {
        bridge::put_soon("meta", "readers", &*self.readers.peek());
    }

    pub fn save_presets(self) {
        bridge::put_soon("meta", "presets", &*self.presets.peek());
    }

    pub fn save_last_exports(self) {
        bridge::put_soon("meta", "last_exports", &*self.last_exports.peek());
    }

    pub fn save_settings(self) {
        bridge::put_soon("meta", "settings", &*self.settings.peek());
    }

    pub fn book(self, id: &str) -> Option<Book> {
        if id == ui::export::SAMPLE {
            return Some(ui::export::sample_book());
        }
        self.library.peek().find(id).cloned()
    }

    pub fn is_online(self) -> bool {
        *self.online.peek()
    }
}

pub fn current_iso() -> String {
    crate::model::iso_of_ms(bridge::now_ms())
}

#[component]
pub fn App() -> Element {
    let ctx = Ctx {
        boot: use_signal(|| Boot::Starting),
        library: use_signal(Library::default),
        settings: use_signal(Settings::default),
        caps: use_signal(Caps::default),
        page: use_signal(|| Page::Library),
        reading: use_signal(|| None),
        import: use_signal(ImportState::default),
        toasts: use_signal(Vec::new),
        modal: use_signal(|| None),
        online: use_signal(|| true),
        update_ready: use_signal(|| false),
        offline_ready: use_signal(|| false),
        can_install: use_signal(|| false),
        export: use_signal(ui::export::ExportUi::default),
        presets: use_signal(library::PresetsDb::default),
        last_exports: use_signal(Default::default),
        narrow: use_signal(|| false),
        readers: use_signal(library::Settings::default),
        routed: use_signal(|| false),
        settings_section: use_signal(|| ui::settings::Section::Library),
        unsaved_reader: use_signal(|| None),
        leave_prompt: use_signal(|| None),
        next_id: use_signal(|| 0),
        state_file: use_signal(|| None),
    };
    use_context_provider(|| ctx);

    // Start: read what this browser keeps, then listen for what it says.
    use_future(move || async move {
        let boot: Result<bridge::Boot, String> = bridge::call("boot", json!({})).await;
        match boot {
            Ok(b) => {
                let mut library = ctx.library;
                let mut caps = ctx.caps;
                let mut settings = ctx.settings;
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
                    let mut readers = ctx.readers;
                    let saved = b.meta.get("readers").and_then(|v| serde_json::from_value::<library::Settings>(v.clone()).ok());
                    let r = saved.unwrap_or_else(|| {
                        // A first visit starts with the three Xteink readers, as the app does.
                        let mut r = library::Settings { readers_listed: false, ..Default::default() };
                        r.list_old_readers();
                        let chosen = settings.peek().device.clone();
                        if r.devices().iter().any(|d| d.id == chosen) {
                            r.device = chosen;
                        }
                        r
                    });
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
            Boot::Ready => rsx! { Shell {} },
        }}
        ModalHost {}
        UnsavedHost {}
        ui::import::ImportHost {}
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
        Event::Import { drafts, failures } => ui::import::analysis_done(ctx, drafts, failures),
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
        Event::Key { .. } | Event::Tap { .. } | Event::Swipe { .. } | Event::Zoom { .. } | Event::Wheel { .. } | Event::Pos { .. } | Event::Scrolled { .. } | Event::EdgeBack => {
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

#[component]
fn NavItem(page: Page, label: &'static str, icon: &'static str) -> Element {
    let ctx = use_context::<Ctx>();
    let on = *ctx.page.read() == page;
    rsx! {
        button {
            class: if on { "nav-item active" } else { "nav-item" },
            "aria-current": if on { "page" } else { "false" },
            id: "nav-{label}",
            onclick: move |_| ctx.go(page),
            Icon { name: icon, size: 18 }
            span { class: "nav-label", "{label}" }
        }
    }
}

#[component]
fn Shell() -> Element {
    let ctx = use_context::<Ctx>();
    let page = *ctx.page.read();
    let reading = ctx.reading.read().is_some();
    let online = *ctx.online.read();
    let update = *ctx.update_ready.read();
    let mut later = use_signal(|| false);
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
            nav { class: "nav",
                div { class: "brand",
                    art::cat::Cat {}
                    span { "MangaPoint" }
                }
                NavItem { page: Page::Library, label: "Library", icon: "book-open" }
                NavItem { page: Page::Export, label: "Export", icon: "file-down" }
                div { class: "nav-spacer" }
                NavItem { page: Page::Settings, label: "Settings", icon: "settings" }
            }
            main { class: "content", id: "content",
                div { class: "page",
                    if !online {
                        div { class: "banner small offline-note",
                            Icon { name: "info", size: 16 }
                            div { "You are offline. Your library still works." }
                        }
                    }
                    {match page {
                        Page::Library => rsx! { ui::library::LibraryPage {} },
                        Page::Export => rsx! { ui::export::ExportPage {} },
                        Page::Settings => rsx! { ui::settings::SettingsPage {} },
                    }}
                }
            }
            if reading {
                ui::reader::ReaderHost {}
            }
            if update && !*later.read() {
                div { class: "overlay", id: "update-dialog", onclick: move |_| later.set(true),
                    div { class: "modal update-modal", onclick: move |e| e.stop_propagation(),
                        div { class: "upd-top", crate::update_cat::UpdateCat {} }
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

/// Asked when the user leaves the reader form with changes that are not saved.
#[component]
fn UnsavedHost() -> Element {
    let ctx = use_context::<Ctx>();
    let Some(to) = ctx.leave_prompt.read().clone() else { return rsx! {} };
    let (save_to, discard_to) = (to.clone(), to);
    rsx! {
        div { class: "overlay", id: "unsaved-prompt",
            div { class: "modal",
                h2 { "You have unsaved settings" }
                p { "Would you like to save or discard them?" }
                div { class: "actions",
                    button { class: "btn", id: "unsaved-keep",
                        onclick: move |_| { let mut p = ctx.leave_prompt; p.set(None); },
                        "Keep editing"
                    }
                    button { class: "btn danger", id: "unsaved-discard",
                        onclick: move |_| {
                            let mut p = ctx.leave_prompt;
                            p.set(None);
                            let mut unsaved = ctx.unsaved_reader;
                            unsaved.set(None);
                            ctx.leave_now(discard_to.clone());
                        },
                        "Discard"
                    }
                    button { class: "btn primary", id: "unsaved-save",
                        onclick: move |_| {
                            let saved = ctx.unsaved_reader.peek().clone().is_none_or(|u| (u.0)());
                            if saved {
                                let mut p = ctx.leave_prompt;
                                p.set(None);
                                let mut unsaved = ctx.unsaved_reader;
                                unsaved.set(None);
                                ctx.leave_now(save_to.clone());
                            }
                        },
                        "Save"
                    }
                }
            }
        }
    }
}

#[component]
fn ModalHost() -> Element {
    let ctx = use_context::<Ctx>();
    let modal = ctx.modal.read().clone();
    let Some(modal) = modal else { return rsx! {} };
    let confirm_class = if modal.danger { "btn danger" } else { "btn primary" };
    let action = modal.action.clone();
    rsx! {
        div { class: "overlay", onclick: move |_| { let mut m = ctx.modal; m.set(None); },
            div { class: "modal", role: "dialog", "aria-modal": "true", onclick: move |e| e.stop_propagation(),
                h2 { "{modal.title}" }
                p { "{modal.body}" }
                div { class: "actions",
                    button { class: "btn", id: "modal-cancel", onclick: move |_| { let mut m = ctx.modal; m.set(None); }, "{modal.cancel}" }
                    button { class: "{confirm_class}", id: "modal-confirm",
                        onclick: move |_| {
                            let mut m = ctx.modal;
                            m.set(None);
                            run_action(ctx, action.clone());
                        },
                        "{modal.confirm}"
                    }
                }
            }
        }
    }
}

fn run_action(ctx: Ctx, action: ModalAction) {
    dioxus::core::spawn_forever(async move {
        match action {
            ModalAction::RemoveBook { id, forget } => {
                if ctx.reading.peek().as_ref().is_some_and(|r| r.book_id == id) {
                    ui::reader::close_reader(ctx);
                }
                match bridge::call_ok("remove_book", json!({ "id": id })).await {
                    Ok(()) => {
                        let title = ctx.book(&id).map(|b| b.title.clone()).unwrap_or_default();
                        let mut lib = ctx.library;
                        // Only the download goes: the book stays, waiting for its file.
                        if !forget {
                            if let Some(book) = ctx.book(&id) {
                                lib.write().keep_place(&book);
                                ctx.save_state_of(&id);
                            }
                        }
                        lib.write().remove_book(&id, forget);
                        if forget {
                            let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
                        }
                        ctx.notify(if forget { format!("Forgot {title}.") } else { format!("Removed the download of {title}.") });
                    }
                    Err(e) => ctx.error(format!("Could not remove it: {e}")),
                }
            }
            ModalAction::ForgetWaiting(id) => {
                let mut lib = ctx.library;
                lib.write().forget_state(&id);
                let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
            }
            ModalAction::RemoveDownloads(ids) => {
                let mut removed = 0usize;
                for id in ids {
                    let Some(book) = ctx.book(&id) else { continue };
                    if ctx.reading.peek().as_ref().is_some_and(|r| r.book_id == id) {
                        ui::reader::close_reader(ctx);
                    }
                    if bridge::call_ok("remove_book", json!({ "id": id })).await.is_err() {
                        continue;
                    }
                    let mut lib = ctx.library;
                    lib.write().keep_place(&book);
                    ctx.save_state_of(&id);
                    lib.write().remove_book(&id, false);
                    removed += 1;
                }
                ctx.notify(format!("Removed the downloads of {}. They stay in your library.", plural(removed, "book")));
            }
            ModalAction::ForgetMany(ids) => {
                let mut forgotten = 0usize;
                for id in ids {
                    if ctx.book(&id).is_some() {
                        if ctx.reading.peek().as_ref().is_some_and(|r| r.book_id == id) {
                            ui::reader::close_reader(ctx);
                        }
                        if bridge::call_ok("remove_book", json!({ "id": id })).await.is_err() {
                            continue;
                        }
                        let mut lib = ctx.library;
                        lib.write().remove_book(&id, true);
                    } else {
                        let mut lib = ctx.library;
                        lib.write().forget_state(&id);
                    }
                    let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
                    forgotten += 1;
                }
                ctx.notify(format!("Forgot {}.", plural(forgotten, "book")));
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
            ModalAction::RemoveReader(id) => {
                let mut readers = ctx.readers;
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
                        let mut settings = ctx.settings;
                        settings.set(Settings::default());
                        let mut readers = ctx.readers;
                        let mut fresh = library::Settings { readers_listed: false, ..Default::default() };
                        fresh.list_old_readers();
                        readers.set(fresh);
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

#[component]
fn ToastHost() -> Element {
    let ctx = use_context::<Ctx>();
    let toasts = ctx.toasts.read().clone();
    if toasts.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "toasts",
            for (i, toast) in toasts.iter().rev().enumerate() {
                {
                    let id = toast.id;
                    let look = match toast.kind {
                        ToastKind::Success => "success",
                        ToastKind::Warning => "warning",
                        ToastKind::Error => "error",
                    };
                    rsx! {
                        div {
                            key: "{toast.id}",
                            class: if toast.leaving { "toast {look} leaving" } else { "toast {look}" },
                            style: "--i: {i}",
                            role: "status",
                            art::cat::ToastCat { look: look.to_string() }
                            div { class: "toast-text", "{toast.text}" }
                            button { class: "toast-x", title: "Dismiss", "aria-label": "Dismiss", onclick: move |_| ctx.dismiss_toast(id),
                                Icon { name: "x", size: 14 }
                            }
                        }
                    }
                }
            }
        }
    }
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

/// "3 books" for a count.
pub fn books(n: usize) -> String {
    plural(n, "book")
}

/// A book's state as the file would hold it, for the Settings counts.
pub fn state_count(lib: &Library) -> usize {
    lib.states.len()
}

pub fn _unused(_: &BookState, _: &Value) {}

/// The address of the screen being shown, without the leading slash: `` (the library), `export`,
/// `settings` (with the part of Settings after `#`) or `read?book=<id>&chapter=<n>`.
fn route_of(ctx: Ctx) -> (String, String) {
    if let Some(r) = ctx.reading.read().as_ref() {
        return (format!("read?book={}&chapter={}", r.book_id, r.chapter + 1), String::new());
    }
    match *ctx.page.read() {
        Page::Library => (String::new(), String::new()),
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
