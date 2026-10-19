//! Where the app's files are, and saving the databases kept in them.

use super::AppState;
use crate::queue::DownloadQueue;
use dioxus::prelude::*;
use library::{AppPaths, JsonFile, LibraryFiles, PresetsDb, Settings, SpeedDb};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Where the app keeps its files. Set `MANGAPOINT_HOME` to use another folder
/// (handy for trying the app without touching real settings).
pub fn paths() -> &'static AppPaths {
    static PATHS: OnceLock<AppPaths> = OnceLock::new();
    PATHS.get_or_init(|| {
        AppPaths::new(
            std::env::var_os("MANGAPOINT_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(AppPaths::default_root),
        )
    })
}

/// The folders downloads are kept in: where new ones go, and the others.
/// Process-wide because background tasks build `LibraryFiles` from anywhere,
/// without an `AppState` to hand; `apply_library_dirs` keeps it in step with the
/// settings.
static LIBRARY_DIRS: std::sync::RwLock<Option<(PathBuf, Vec<PathBuf>)>> =
    std::sync::RwLock::new(None);

/// Use the download folders from the settings. Call at start and whenever the
/// choice changes.
pub fn apply_library_dirs(settings: &Settings) {
    let app_folder = paths().library_dir();
    let active = settings.library_dir.clone().unwrap_or(app_folder.clone());
    let mut others: Vec<PathBuf> = std::iter::once(app_folder)
        .chain(settings.library_dirs.iter().cloned())
        .filter(|p| *p != active)
        .collect();
    others.dedup();
    if let Ok(mut dirs) = LIBRARY_DIRS.write() {
        *dirs = Some((active, others));
    }
}

/// The downloaded chapters on disk.
pub fn library_files() -> LibraryFiles {
    match LIBRARY_DIRS.read().ok().and_then(|d| d.clone()) {
        Some((active, others)) => LibraryFiles::new(active).with_other_roots(others),
        None => LibraryFiles::new(paths().library_dir()),
    }
}

impl AppState {
    pub fn save_settings(&self) {
        let _ = JsonFile::new(paths().settings_file()).save(&*self.settings.read());
    }

    pub fn save_sources(&self) {
        let _ = JsonFile::new(paths().sources_file()).save(&*self.sources.read());
    }

    pub fn save_health(&self) {
        let _ = JsonFile::new(paths().health_file()).save(&*self.health.read());
    }

    /// Change the presets. (A signal can only be written through a mutable
    /// binding, which `&self` does not give.)
    pub fn edit_presets<R>(&self, edit: impl FnOnce(&mut PresetsDb) -> R) -> R {
        let mut presets = self.presets;
        let mut guard = presets.write();
        edit(&mut guard)
    }

    /// Change the download queue.
    pub fn edit_queue<R>(&self, edit: impl FnOnce(&mut DownloadQueue) -> R) -> R {
        let mut queue = self.queue;
        let mut guard = queue.write();
        let result = edit(&mut guard);
        drop(guard);
        // Kept on disk, so what is waiting and what has finished survive a
        // restart; the saver writes it soon, and when the window closes.
        self.mark_queue_changed();
        result
    }

    /// Change the download queue without saving it. For how far a download
    /// has got, which is not kept on disk: saving the whole queue (hundreds of
    /// KB) several times a second held up the window for nothing.
    pub fn edit_queue_quiet<R>(&self, edit: impl FnOnce(&mut DownloadQueue) -> R) -> R {
        let mut queue = self.queue;
        let mut guard = queue.write();
        edit(&mut guard)
    }

    /// Change the saved source speeds.
    pub fn edit_speeds<R>(&self, edit: impl FnOnce(&mut SpeedDb) -> R) -> R {
        let mut speeds = self.speeds;
        let mut guard = speeds.write();
        edit(&mut guard)
    }

    pub fn save_speeds(&self) {
        let _ = JsonFile::new(paths().speeds_file()).save(&*self.speeds.read());
    }

    /// Save the presets, bring every series that follows a preset up to date
    /// with it, and redraw the preview.
    pub fn save_presets(&self) {
        let _ = JsonFile::new(paths().presets_file()).save(&*self.presets.read());
        let mut library = self.library;
        let changed = library.write().follow_presets(&self.presets.read());
        if changed {
            self.save_library();
        }
        if self.workspace.peek().is_some() {
            crate::ui::work::schedule_preview(*self, 0);
        }
    }

    /// The conversion settings and preset a new series starts with: the
    /// preset chosen for the current reader if there is one, else the user's
    /// defaults.
    pub fn new_series_defaults(&self) -> (library::ConversionSettings, Option<String>) {
        let settings = self.settings.peek();
        let for_device = settings
            .device_presets
            .get(&settings.device)
            .and_then(|id| self.presets.peek().find(id));
        match for_device {
            Some(entry) => (entry.preset.settings, Some(entry.preset.id)),
            None => (
                settings.conversion.clone(),
                settings.default_preset_id.clone(),
            ),
        }
    }

    pub fn save_exports(&self) {
        let _ = JsonFile::new(paths().exports_file()).save(&*self.exports.read());
    }
}
