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
    ClearAll,
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
    /// In paged mode the page of the chapter (the chapter's length is the end card).
    /// In scroll mode the page at the top of the strip.
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
    pub next_id: Signal<u64>,
    pub state_file: Signal<Option<String>>,
}

impl Ctx {
    pub fn id(mut self) -> u64 {
        let n = *self.next_id.peek() + 1;
        self.next_id.set(n);
        n
    }

    pub fn go(mut self, page: Page) {
        self.page.set(page);
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

    pub fn save_settings(self) {
        bridge::put_soon("meta", "settings", &*self.settings.peek());
    }

    pub fn book(self, id: &str) -> Option<Book> {
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
                caps.set(b.caps);
                if let Some(e) = b.error {
                    ctx.error(e);
                }
                boot_state.set(Boot::Ready);
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
                    if update {
                        div { class: "banner small update-note",
                            Icon { name: "download", size: 16 }
                            div { class: "grow", "A new version of MangaPoint is ready." }
                            button { class: "btn small primary", onclick: move |_| bridge::send("apply_update", json!({})), "Reload" }
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
                        let title = ctx.book(&id).map(|b| b.title).unwrap_or_default();
                        let mut lib = ctx.library;
                        lib.write().remove_book(&id, forget);
                        if forget {
                            let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
                        }
                        ctx.notify(format!("Removed {title}."));
                    }
                    Err(e) => ctx.error(format!("Could not remove it: {e}")),
                }
            }
            ModalAction::ForgetWaiting(id) => {
                let mut lib = ctx.library;
                lib.write().forget_state(&id);
                let _ = bridge::call_ok("db_delete", json!({ "store": "state", "key": id })).await;
            }
            ModalAction::ClearAll => {
                ui::reader::close_reader(ctx);
                match bridge::call_ok("clear_all", json!({})).await {
                    Ok(()) => {
                        let mut lib = ctx.library;
                        *lib.write() = Library::default();
                        let mut settings = ctx.settings;
                        settings.set(Settings::default());
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
