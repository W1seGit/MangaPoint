//! Toasts and confirmation dialogs.

use super::{spawn_forever, AppState};
use dioxus::prelude::*;

#[derive(Clone, PartialEq, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    /// On its way out: it slides off and is then taken away.
    pub leaving: bool,
}

/// What a toast says: it worked, something to know, or it failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Success,
    Warning,
    Error,
}

/// What a confirmation dialog does when accepted.
#[derive(Clone, PartialEq, Debug)]
pub enum ModalAction {
    /// Turn on browser support, then install the built-in source with this id (if any).
    EnableBrowser {
        then_install: Option<String>,
    },
    DisableBrowser,
    RemoveExtension(String),
    /// Take series out of the library, deleting what was downloaded for them
    /// (exported files are kept).
    RemoveSeries {
        series_ids: Vec<String>,
    },
    /// Delete the downloaded pages of some chapters; the series stays.
    DeleteChapters {
        series_id: String,
        keys: Vec<String>,
    },
    /// Delete every downloaded chapter of these series; they stay in the library.
    DeleteDownloads {
        series_ids: Vec<String>,
    },
    /// Delete one of the user's presets, or put a changed built-in back.
    DeletePreset {
        id: String,
    },
    /// Give a series a preset's settings (replacing its own).
    LoadPreset {
        series_id: String,
        preset_id: Option<String>,
        settings: library::ConversionSettings,
    },
    /// Save a series' settings over one of the user's presets.
    SavePreset {
        id: String,
        settings: library::ConversionSettings,
    },
    /// Take a reader off the list (series made for it keep their settings).
    RemoveReader { id: String },
    /// Clear the list of finished downloads.
    ClearFinishedDownloads,
    /// Stop every download, the one running and those waiting.
    CancelAllDownloads,
    /// Delete an exported file from disk.
    /// Every file of one export.
    DeleteExport { record_id: String },
    DeleteExportedFile {
        record_id: String,
        path: String,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Modal {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
    pub danger: bool,
    pub action: ModalAction,
}

impl AppState {
    /// Something worked.
    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::info!("{text}");
        self.show_toast(ToastKind::Success, text);
    }

    /// Something to know, that is not a failure.
    #[allow(dead_code)]
    pub fn warn(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.show_toast(ToastKind::Warning, text);
    }

    /// Something failed.
    pub fn error(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.show_toast(ToastKind::Error, text);
    }

    /// Put a toast on the stack, and take it away again after a while (longer
    /// for the ones that went wrong; never while the pointer is over them).
    fn show_toast(&self, kind: ToastKind, text: String) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut toasts = self.toasts;
        {
            let mut stack = toasts.write();
            // The same message again replaces the one already there.
            stack.retain(|t| !(t.text == text && t.kind == kind && !t.leaving));
            stack.push(Toast {
                id,
                kind,
                text,
                leaving: false,
            });
            // Only the newest few are kept.
            let extra = stack.len().saturating_sub(8);
            stack.drain(..extra);
        }
        let seconds = match kind {
            ToastKind::Success => 4,
            ToastKind::Warning => 7,
            ToastKind::Error => 10,
        };
        let this = *self;
        spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
            while *this.toasts_hovered.peek() {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
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
        let this = *self;
        spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_millis(340)).await;
            let mut toasts = this.toasts;
            toasts.write().retain(|t| t.id != id);
        });
    }

    pub fn ask(&self, modal: Modal) {
        let mut m = self.modal;
        m.set(Some(modal));
    }
}
