//! Everything the screens share.
//!
//! `AppState` holds one signal for each thing the screens share; the types they
//! hold and the methods that change them live in one file per topic, and are
//! all re-exported here.

mod boot;
mod download;
mod export;
mod import;
mod job;
mod navigation;
mod notices;
mod preview;
mod reader;
mod saver;
mod search_view;
mod storage;
mod workspace;

pub use boot::*;
pub use download::*;
pub use export::*;
pub use import::*;
pub use job::*;
pub use navigation::*;
pub use notices::*;
pub use preview::*;
pub use reader::*;
pub use search_view::*;
pub use storage::*;
pub use workspace::*;

use crate::logic::SearchSort;
use crate::queue::DownloadQueue;
use catalog::SuwayomiClient;
use dioxus::prelude::*;
use library::{ExportsDb, HealthDb, JsonFile, LibraryDb, PresetsDb, Settings, SourceState, SpeedDb};
use mangapoint_ui::host::UiState;
use pipeline::SourceChoice;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct AppState {
    /// What the desktop app shares with the website: the settings, the toasts, the reader
    /// form's unsaved changes (see `mangapoint_ui::host`).
    pub ui: UiState,
    pub boot: Signal<BootState>,
    /// The catalog engine was already installed when the app started, so the
    /// app can be used (offline) while it starts. False on the very first run,
    /// which has to download it.
    pub engine_installed: bool,
    pub services: Signal<Option<Services>>,
    pub sources: Signal<SourceState>,
    pub health: Signal<HealthDb>,
    pub page: Signal<Page>,
    pub search: Signal<SearchView>,
    /// How the search results are ordered.
    pub search_sort: Signal<SearchSort>,
    /// What the search results are narrowed to.
    pub search_filters: Signal<crate::logic::SearchFilters>,
    /// The genres each search result's source lists, once looked up.
    pub search_tags: Signal<HashMap<(String, i64), Vec<String>>>,
    pub settings_section: Signal<SettingsSection>,
    /// Details of the series opened from a search, once the sources have answered.
    pub open_details: Signal<Option<OpenDetails>>,
    /// Cover images as data URLs, keyed by `source:manga`.
    pub covers: Signal<HashMap<String, std::rc::Rc<str>>>,
    pub modal: Signal<Option<Modal>>,
    /// Downloaded chapters (their pages are on disk).
    pub library: Signal<LibraryDb>,
    /// Files exported so far.
    pub exports: Signal<ExportsDb>,
    /// The user's conversion presets (and changes to the built-in ones).
    pub presets: Signal<PresetsDb>,
    /// How fast each source has been, from short speed tests.
    pub speeds: Signal<SpeedDb>,
    /// Where the next download takes its chapters from.
    pub download_source: Signal<SourceChoice>,
    /// Chapters waiting to download, the batch downloading now, and what finished.
    pub queue: Signal<DownloadQueue>,
    /// The series being planned for download, with their ticked chapters.
    pub plan: Signal<Vec<PlanEntry>>,
    /// Asking where a download should go, if it is open.
    pub download_dialog: Signal<Option<DownloadDialog>>,
    /// A newer version being offered, if the dialog for it is open.
    pub update_prompt: Signal<Option<crate::updates::Update>>,
    /// Where getting that version has got to.
    pub update_phase: Signal<UpdatePhase>,
    /// Series whose details are being fetched right now.
    pub info_busy: Signal<HashSet<String>>,
    /// The series open in the reader, if it is open.
    pub reader: Signal<Option<ReaderOpen>>,
    pub workspace: Signal<Option<Workspace>>,
    /// The series that was open when the Export screen took the workspace over,
    /// put back when the screen is left.
    pub parked_workspace: Signal<Option<Workspace>>,
    pub chapters: Signal<ChaptersState>,
    /// Chapters ticked in the table, by position in the merged list.
    pub selected: Signal<HashSet<usize>>,
    pub table: Signal<TableView>,
    pub preview: Signal<PreviewState>,
    pub export_opts: Signal<ExportOpts>,
    /// The series on the Export screen.
    pub export_plan: Signal<ExportPlan>,
    /// The manual crop being dragged, for the page on screen.
    pub crop_draft: Signal<library::CropRect>,
    /// The import dialog, when one is open.
    pub import: Signal<Option<ImportSession>>,
    /// The running (or just finished) download or export.
    pub job: Signal<Option<JobView>>,
    /// The exports lined up behind the running one, in the order they go.
    pub export_waiting: Signal<Vec<WaitingExport>>,
    /// Bumped when installed sources or test results change.
    pub sources_version: Signal<u32>,
    /// Source ids being tested right now.
    pub testing: Signal<HashSet<String>>,
    /// True while "install all recommended" is running.
    pub installing: Signal<bool>,
}

impl AppState {
    /// Create the state once, inside the app's root component, loading saved
    /// files.
    ///
    /// The signals are owned by Dioxus's root scope rather than by the
    /// component that creates them. Background tasks (downloads, searches,
    /// retests) run in the root scope and write to this state, and a signal
    /// may only be used from its owner or the owner's descendants.
    pub fn init() -> Self {
        use_hook(Self::build)
    }

    fn build() -> Self {
        fn sig<T: 'static>(value: T) -> Signal<T> {
            Signal::new_in_scope(value, ScopeId::ROOT)
        }
        let sources: SourceState = JsonFile::new(paths().sources_file()).load();
        // The first run is walked through before the app shows; see `onboarding`.
        let first_page = Page::Search;
        let settings = {
            let mut settings = JsonFile::<Settings>::new(paths().settings_file()).load();
            settings.list_old_readers();
            // Dev flag: `cargo run -p mangapoint -- --onboarding` (or
            // MANGAPOINT_ONBOARDING=1) walks through the first-run setup again.
            if std::env::args().any(|a| a == "--onboarding")
                || std::env::var_os("MANGAPOINT_ONBOARDING").is_some()
            {
                settings.setup_step = library::SetupStep::Engine;
            }
            apply_library_dirs(&settings);
            settings
        };
        Self {
            ui: UiState::new(settings),
            engine_installed: catalog::provision::locate_install(
                &paths()
                    .suwayomi_dir()
                    .join(format!("suwayomi-{}", catalog::release::VERSION_TAG)),
            )
            .is_ok(),
            boot: sig(BootState::Starting {
                message: "Starting…".into(),
                percent: None,
            }),
            services: sig(None),
            sources: sig(sources),
            health: sig(JsonFile::<HealthDb>::new(paths().health_file()).load()),
            page: sig(first_page),
            search: sig(SearchView::default()),
            search_sort: sig(SearchSort::default()),
            search_filters: sig(crate::logic::SearchFilters::default()),
            search_tags: sig(HashMap::new()),
            settings_section: sig(SettingsSection::default()),
            open_details: sig(None),
            covers: sig(HashMap::new()),
            modal: sig(None),
            library: sig(JsonFile::<LibraryDb>::new(paths().library_file()).load()),
            exports: sig(JsonFile::<ExportsDb>::new(paths().exports_file()).load()),
            presets: sig(JsonFile::<PresetsDb>::new(paths().presets_file()).load()),
            speeds: sig(JsonFile::<SpeedDb>::new(paths().speeds_file()).load()),
            download_source: sig(SourceChoice::Adaptive),
            queue: sig(DownloadQueue::from_saved(
                JsonFile::<crate::queue::SavedQueue>::new(paths().queue_file()).load(),
            )),
            plan: sig(Vec::new()),
            download_dialog: sig(None),
            update_prompt: sig(None),
            update_phase: sig(UpdatePhase::Idle),
            info_busy: sig(HashSet::new()),
            reader: sig(None),
            workspace: sig(None),
            parked_workspace: sig(None),
            chapters: sig(ChaptersState::Idle),
            selected: sig(HashSet::new()),
            table: sig(TableView::default()),
            preview: sig(PreviewState::default()),
            export_opts: sig(ExportOpts::default()),
            export_plan: sig(ExportPlan::default()),
            crop_draft: sig(library::CropRect::default()),
            import: sig(None),
            job: sig(None),
            export_waiting: sig(Vec::new()),
            sources_version: sig(0),
            testing: sig(HashSet::new()),
            installing: sig(false),
        }
    }

    pub fn client(&self) -> Option<Arc<SuwayomiClient>> {
        self.services.read().as_ref().map(|s| s.client.clone())
    }

    pub fn bump_sources(&self) {
        let mut v = self.sources_version;
        let next = *v.peek() + 1;
        v.set(next);
    }
}

/// Run a task for as long as the app lives, even after the screen that
/// started it is closed. (Ordinary `spawn` tasks stop with their screen.)
pub fn spawn_forever(future: impl std::future::Future<Output = ()> + 'static) {
    dioxus::core::spawn_forever(future);
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
