//! How the shared screens reach the app they are in.
//!
//! - [`UiState`] holds the signals both apps keep: the settings (with the readers), the reader
//!   form's unsaved changes and the question about them, the toasts and the open groups of the
//!   Edit controls. The desktop app's `AppState` and the website's `Ctx` each own one and provide
//!   it as context, so a shared screen asks for `UiState`, never for an app's own state.
//! - [`Host`] is the short list of things the two apps do differently: saving the settings,
//!   picking a reader, asking for confirmation, waiting, running a script on the page. Each app
//!   implements it once (`DesktopHost`, `WebHost`).

use dioxus::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// A task that runs on the page's own thread.
pub type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// What a toast says: it worked, something to know, or it failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Success,
    Warning,
    Error,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    /// On its way out: it slides off and is then taken away.
    pub leaving: bool,
}

/// Writes the reader form's unsaved changes; true if they were saved. Present
/// only while the form has changes that are not saved.
#[derive(Clone)]
pub struct UnsavedReader(pub Rc<dyn Fn() -> bool>);

/// Where the user was going when unsaved reader settings stopped them: asked
/// about, then carried on to.
#[derive(Clone)]
pub enum Leave {
    /// Another reader, by id.
    Reader(String),
    /// Somewhere in the app (a page, a part of Settings): carrying on goes there.
    Elsewhere(Rc<dyn Fn()>),
}

/// What a confirmation asked by a shared screen does when it is accepted. Each app turns it into
/// its own action.
#[derive(Clone, PartialEq, Debug)]
pub enum SharedAction {
    /// Take a reader off the list (books and series made for it keep their settings).
    RemoveReader { id: String },
}

/// A confirmation dialog asked for by a shared screen.
#[derive(Clone, PartialEq, Debug)]
pub struct Ask {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
    pub danger: bool,
    pub action: SharedAction,
}

/// What the app can do that the other cannot, for the screens that show more where it can.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Caps {
    /// Files can be sent straight to a reader (over USB or Wi-Fi): the desktop app.
    pub send_to_reader: bool,
}

/// The things the two apps do differently. See the module's notes.
pub trait Host {
    /// Wait this long, in milliseconds.
    fn sleep(&self, ms: u64) -> LocalFuture<()>;
    /// Write a message to the app's log (the desktop app's log file; nothing on the website).
    fn log(&self, _kind: ToastKind, _text: &str) {}
    /// Keep the settings (the desktop app's settings file; the website's store in this browser).
    fn save_settings(&self);
    /// Make this reader the one in use, and let the screens that make pages for it follow.
    fn use_reader(&self, id: &str);
    /// The reader in use was edited and saved: the screens that make pages for it follow.
    fn reader_saved(&self) {}
    /// Open Settings at the list of readers.
    fn open_readers(&self);
    /// Ask for a confirmation.
    fn ask(&self, ask: Ask);
    /// Run one of the page scripts by name, with its arguments as JSON.
    fn run_script(&self, name: &str, args: String);
    /// Check that a reader answers at this Wi-Fi address (only where `caps().send_to_reader`).
    fn check_wifi(&self, _url: String) -> LocalFuture<Result<(), String>> {
        Box::pin(async { Err("Sending to a reader over Wi-Fi is not possible here.".to_string()) })
    }
    /// What this app can do.
    fn caps(&self) -> Caps {
        Caps::default()
    }
}

/// The signals both apps keep, and the host they run in.
#[derive(Clone, Copy)]
pub struct UiState {
    host: Signal<Option<Rc<dyn Host>>>,
    /// The app's settings: the readers (and the one in use), the conversion defaults, and on the
    /// desktop everything else the app remembers.
    pub settings: Signal<library::Settings>,
    /// Writes the reader form's unsaved changes, while it has any.
    pub unsaved_reader: Signal<Option<UnsavedReader>>,
    /// Going somewhere was held back by unsaved reader settings; the user is asked.
    pub leave_prompt: Signal<Option<Leave>>,
    pub toasts: Signal<Vec<Toast>>,
    /// The pointer is over the toasts: they stay while it is.
    pub toasts_hovered: Signal<bool>,
    /// The groups of the Edit controls that are open. They all start folded,
    /// and stay as they were left when the screen is visited again.
    pub open_groups: Signal<Vec<&'static str>>,
}

impl UiState {
    /// The signals, owned by the root of the app so background tasks can use them. Call
    /// [`UiState::set_host`] before anything uses the host.
    pub fn new(settings: library::Settings) -> Self {
        fn sig<T: 'static>(value: T) -> Signal<T> {
            Signal::new_in_scope(value, ScopeId::ROOT)
        }
        Self {
            host: sig(None),
            settings: sig(settings),
            unsaved_reader: sig(None),
            leave_prompt: sig(None),
            toasts: sig(Vec::new()),
            toasts_hovered: sig(false),
            open_groups: sig(Vec::new()),
        }
    }

    pub fn set_host(self, host: Rc<dyn Host>) {
        let mut h = self.host;
        h.set(Some(host));
    }

    pub fn host(&self) -> Rc<dyn Host> {
        self.host.peek().clone().expect("the app sets its host when it starts")
    }

    // ---- messages ----

    /// Something worked.
    pub fn notify(&self, text: impl Into<String>) {
        self.toast(ToastKind::Success, text.into());
    }

    /// Something to know, that is not a failure.
    pub fn warn(&self, text: impl Into<String>) {
        self.toast(ToastKind::Warning, text.into());
    }

    /// Something failed.
    pub fn error(&self, text: impl Into<String>) {
        self.toast(ToastKind::Error, text.into());
    }

    /// Put a toast on the stack, and take it away again after a while (longer
    /// for the ones that went wrong; never while the pointer is over them).
    pub fn toast(&self, kind: ToastKind, text: String) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let host = self.host();
        host.log(kind, &text);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut toasts = self.toasts;
        {
            let mut stack = toasts.write();
            // The same message again replaces the one already there.
            stack.retain(|t| !(t.text == text && t.kind == kind && !t.leaving));
            stack.push(Toast { id, kind, text, leaving: false });
            // Only the newest few are kept.
            let extra = stack.len().saturating_sub(8);
            stack.drain(..extra);
        }
        let ms = match kind {
            ToastKind::Success => 4000,
            ToastKind::Warning => 7000,
            ToastKind::Error => 10_000,
        };
        let this = *self;
        dioxus::core::spawn_forever(async move {
            host.sleep(ms).await;
            while *this.toasts_hovered.peek() {
                host.sleep(800).await;
            }
            this.dismiss_toast(id);
        });
    }

    /// Slide a toast away, and remove it once it has gone.
    pub fn dismiss_toast(&self, id: u64) {
        let mut toasts = self.toasts;
        {
            let mut stack = toasts.write();
            match stack.iter_mut().find(|t| t.id == id) {
                Some(t) if !t.leaving => t.leaving = true,
                _ => return,
            }
        }
        let host = self.host();
        let this = *self;
        dioxus::core::spawn_forever(async move {
            host.sleep(340).await;
            let mut toasts = this.toasts;
            toasts.write().retain(|t| t.id != id);
        });
    }

    // ---- going somewhere ----

    /// Go somewhere that would lose the reader form's unsaved changes, asking what to do with
    /// them first. `keeps_form` is true for a place that keeps the form open (Settings, its
    /// readers), where nothing is lost.
    pub fn leave(&self, to: Leave, keeps_form: bool) {
        if !keeps_form && self.unsaved_reader.peek().is_some() {
            let mut prompt = self.leave_prompt;
            prompt.set(Some(to));
            return;
        }
        self.leave_now(to);
    }

    /// Carry on to where the user was going, the changes dealt with.
    pub fn leave_now(&self, to: Leave) {
        match to {
            Leave::Reader(id) => self.host().use_reader(&id),
            Leave::Elsewhere(go) => go(),
        }
    }

    // ---- the rest of the host ----

    pub fn save_settings(&self) {
        self.host().save_settings();
    }

    pub fn ask(&self, ask: Ask) {
        self.host().ask(ask);
    }
}

/// The shared state, from a component.
pub fn use_ui() -> UiState {
    use_context::<UiState>()
}
