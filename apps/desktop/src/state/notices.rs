//! Confirmation dialogs. (The toasts are shared with the website: `mangapoint_ui::host`.)

use super::AppState;
use dioxus::prelude::*;

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
    /// Carry on with a paused export to a drive that is not the one it began on.
    ResumeExportAnyway,
    /// Close the app although a download or an export is running.
    QuitApp,
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
        self.ui.notify(text);
    }

    /// Something to know, that is not a failure.
    #[allow(dead_code)]
    pub fn warn(&self, text: impl Into<String>) {
        self.ui.warn(text);
    }

    /// Something failed.
    pub fn error(&self, text: impl Into<String>) {
        self.ui.error(text);
    }

    pub fn ask(&self, modal: Modal) {
        let mut m = self.modal;
        m.set(Some(modal));
    }
}
