//! Everything the screens share.

use crate::logic::{JobProgress, SearchSort, SortKey, TargetChoice};
use crate::queue::DownloadQueue;
use catalog::SuwayomiClient;
use dioxus::prelude::*;
use library::{
    AppPaths, ExportsDb, HealthDb, JsonFile, LibraryDb, LibraryFiles, PresetsDb, Settings,
    SourceState, SpeedDb,
};
use pipeline::{CancelFlag, SourceChoice};
use search::{MergedChapter, RankedSeries};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Setup,
    Search,
    /// A series the user opened: chapters, downloads, preview, export.
    Series,
    Library,
    /// Choosing, editing and exporting series, full window.
    Export,
    /// Downloads and exports, running and finished.
    Tasks,
    /// Choosing what to download from one or more library series.
    Plan,
    Sources,
    Settings,
}

/// Where the user was going when unsaved reader settings stopped them: asked
/// about, then carried on to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    Page(Page),
    Section(SettingsSection),
    /// Another reader, by id.
    Reader(String),
}

/// Writes the reader form's unsaved changes; true if they were saved. Present
/// only while the form has changes that are not saved.
#[derive(Clone)]
pub struct UnsavedReader(pub std::rc::Rc<dyn Fn() -> bool>);

/// The parts of the Settings page, shown in its own sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    #[default]
    Reader,
    Downloads,
    Files,
    Preferences,
    About,
}

/// The steps within an open series.
#[derive(Clone, PartialEq, Debug)]
pub enum BootState {
    Starting {
        message: String,
        percent: Option<u8>,
    },
    Ready,
    /// The first run: the catalog engine is not on this computer, and nothing is
    /// downloaded until the user says so.
    NeedsEngine,
    Failed(String),
}

/// The running catalog server.
#[derive(Clone)]
pub struct Services {
    pub client: Arc<SuwayomiClient>,
    pub port: u16,
    pub data_dir: PathBuf,
}

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

#[derive(Clone, Default)]
pub struct SearchView {
    pub query: String,
    pub ranked: Vec<RankedSeries>,
    /// Sources are still being asked.
    pub searching: bool,
    /// Every source has answered; the chapter lists of the best-looking series
    /// are being looked at, to pick the best source for each. The results are
    /// all there and can be used meanwhile.
    pub enriching: bool,
    pub answered: usize,
    pub total: usize,
    /// Sources that did not answer, by name.
    pub failed: Vec<String>,
    pub no_sources: bool,
    /// Every installed source is turned off for searching.
    pub none_selected: bool,
    /// Which search these results belong to. A newer search bumps it, and an
    /// older one still running then stops publishing.
    pub generation: u64,
}

/// What the sources say about a series opened from a search that is not in
/// the library yet: shown on its overview.
#[derive(Clone)]
pub struct OpenDetails {
    pub series_id: String,
    pub info: library::SeriesInfo,
    /// Where the cover comes from (see `covers`).
    pub cover: Option<Rc<str>>,
}

/// The series the user is working on.
#[derive(Clone)]
pub struct Workspace {
    /// The library id (from the title), under which downloads are kept.
    pub series_id: String,
    pub title: String,
    /// Present when opened from a search; needed to find more chapters.
    pub ranked: Option<RankedSeries>,
    /// Where the back button goes.
    pub origin: Page,
}

/// Chapter lists from the sources, merged.
pub struct LoadedChapters {
    pub merged: Vec<MergedChapter>,
    /// Source id of each member, best first; chapter options point into this.
    pub member_sources: Vec<String>,
    pub member_names: Vec<String>,
    /// The same members as they are kept in the library.
    pub saved: Vec<library::SavedSource>,
    pub unreachable: usize,
}

/// What a download dialog will start once the user confirms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PendingDownload {
    /// The chapters ticked on the open series.
    Chapters,
    /// Everything chosen in the download planner.
    Plan,
}

/// The dialog that asks where downloaded files go, before they start.
#[derive(Clone, PartialEq, Debug)]
pub struct DownloadDialog {
    pub pending: PendingDownload,
    /// What is about to download, in words.
    pub summary: String,
}

/// What the download planner knows about one series.
#[derive(Clone)]
pub enum PlanChapters {
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// One series in the download planner, with the chapters ticked for it.
#[derive(Clone)]
pub struct PlanEntry {
    pub series_id: String,
    pub title: String,
    pub chapters: PlanChapters,
    /// Positions in the merged chapter list.
    pub picked: HashSet<usize>,
    pub open: bool,
    /// The words typed in this series' filter box.
    pub filter: String,
    pub sort: (SortKey, bool),
}

#[derive(Clone)]
pub enum ChaptersState {
    /// Nothing to load (opened from the library).
    Idle,
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// How the chapter table is filtered and sorted.
#[derive(Clone)]
pub struct TableView {
    pub filter: String,
    /// Column, and whether descending.
    pub sort: (SortKey, bool),
}

impl Default for TableView {
    fn default() -> Self {
        Self {
            filter: String::new(),
            sort: (SortKey::Number, true),
        }
    }
}

/// A rendered page as a data URL.
#[derive(Clone, PartialEq)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// How big the pages are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Zoom {
    /// As large as fits without scrolling.
    #[default]
    Fit,
    /// As wide as the space allows; scroll down for the rest.
    Width,
    /// A percentage of the device's own size: 100 is one screen pixel for
    /// each pixel of the reader.
    Custom(u16),
}

impl Zoom {
    pub const MIN_PERCENT: u16 = 25;
    pub const MAX_PERCENT: u16 = 400;
}

/// Preview display choices that are not conversion settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PreviewView {
    pub zoom: Zoom,
    /// Hide the settings panel to give the pages the whole width.
    pub focus: bool,
    /// Show the downloaded page beside the converted one.
    pub show_original: bool,
    /// Draw the cropped-away area on the original.
    pub show_crop: bool,
    pub editing_crop: bool,
    pub source_focus: bool,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: Zoom::Fit,
            focus: false,
            show_original: true,
            show_crop: true,
            editing_crop: false,
            source_focus: false,
        }
    }
}

/// One page of the chapter's plan, as the Preview tab shows it.
#[derive(Clone, Default, PartialEq)]
pub struct PreviewPage {
    /// The page as it will appear on the reader; `None` until it is drawn.
    pub shot: Option<Shot>,
    pub thumbnail: Option<Shot>,
    /// The picture the page comes from, whole and as downloaded: a double
    /// page is one wide picture whatever the spread setting. Joined strip rows
    /// for a strip chapter.
    pub original: Option<Shot>,
    /// Where on `original` this page lies; `None` for a turned page and for
    /// strip rows.
    pub region: Option<converter::plan::SourceRegion>,
    /// Only while cropping: the part of `original` that is cropped (a half, a
    /// turned page, a piece), as the crop is measured on it.
    pub piece: Option<Shot>,
    /// The part of the original the conversion keeps (left, top, width,
    /// height as fractions); paged chapters only.
    pub kept: Option<[f32; 4]>,
    /// The page's image, for a paged chapter. A split double page is two pages
    /// from one image.
    pub image: Option<usize>,
    pub crop_key: String,
    pub source: Option<converter::PlannedPage>,
    pub generation: u64,
    /// The settings (see `PreviewState::signature`) this page was drawn with.
    /// A page drawn with the current ones needs no drawing again.
    pub rendered_for: u64,
}

/// What the Preview tab is showing: every page of one chapter.
#[derive(Clone, Default)]
pub struct PreviewState {
    /// Downloaded chapter being previewed.
    pub chapter_key: Option<String>,
    /// Every page the chapter becomes, in order. After a settings change the
    /// old drawing of a page stays until the new one replaces it.
    pub pages: Vec<Rc<PreviewPage>>,
    /// The chapter is one stitched strip (a webtoon), not one page per image.
    pub strip: bool,
    /// How many downloaded images the chapter has.
    pub images: usize,
    /// Pages drawn with the current settings so far.
    pub fresh: usize,
    pub view: PreviewView,
    pub busy: bool,
    pub error: Option<String>,
    /// Bumped on every change so an older render never overwrites a newer one.
    pub generation: u64,
    /// What the pages should look like, as one number: the chapter, the
    /// conversion settings, the reader, and the crops. Scrolling does not
    /// change it; anything that changes a page does.
    pub signature: u64,
    pub selected: usize,
    pub thumb_start: usize,
    /// The first and last page on or near the screen in the scrolling list;
    /// those are drawn full size.
    pub window: (usize, usize),
    pub overview: bool,
    /// An unsaved crop projected only on the selected source section.
    pub draft_crop: Option<(String, library::CropRect)>,
    pub shading_samples: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupingChoice {
    Single,
    PerChapter,
    EveryN,
}

/// Choices on the Export screen that cover every series being exported.
#[derive(Clone)]
pub struct ExportOpts {
    pub target: TargetChoice,
    pub grouping: GroupingChoice,
    /// The "N" of "every N chapters", as typed.
    pub every_n: String,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            target: TargetChoice::Folder,
            grouping: GroupingChoice::Single,
            every_n: "10".into(),
        }
    }
}

/// One series in the Export screen's list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportItem {
    pub series_id: String,
    /// Downloaded chapters left out of the export.
    pub excluded: HashSet<String>,
    /// Switched off, a series stays in the list but is not exported.
    pub off: bool,
    /// The chapter being previewed, kept for when the screen is opened again.
    pub chapter: Option<String>,
}

/// The series being edited and exported together, and the one on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportPlan {
    pub items: Vec<ExportItem>,
    pub active: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobKind {
    Export,
}

/// A series whose export is lined up behind the one running.
#[derive(Clone, Debug, PartialEq)]
pub struct WaitingExport {
    pub series_id: String,
    pub title: String,
    /// The chapters to export, by key.
    pub keys: Vec<String>,
}

#[derive(Clone)]
pub struct JobView {
    pub kind: JobKind,
    pub title: String,
    /// The series being exported, for its cover.
    pub series_id: String,
    /// Stopped for now; it goes on from where it was when resumed (even after
    /// the app is closed and opened again).
    pub paused: bool,
    pub progress: JobProgress,
    /// `None` while running, then a one-line summary or the error.
    pub result: Option<Result<String, String>>,
    pub cancel: CancelFlag,
}

/// Where getting a newer version has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdatePhase {
    /// The offer is showing and nothing has been started.
    Idle,
    /// Downloading and checking the file (0.0 to 1.0).
    Downloading(f32),
    /// The installer is running and this program is about to close.
    Replacing,
    /// The file was opened or shown; what to do next.
    Handed { path: String, command: Option<String> },
    /// It did not work; this says why.
    Failed(String),
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub boot: Signal<BootState>,
    /// The catalog engine was already installed when the app started, so the
    /// app can be used (offline) while it starts. False on the very first run,
    /// which has to download it.
    pub engine_installed: bool,
    pub services: Signal<Option<Services>>,
    pub settings: Signal<Settings>,
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
    /// The reader form has changes that are not saved yet.
    pub unsaved_reader: Signal<Option<UnsavedReader>>,
    /// Going somewhere was held back by unsaved reader settings; the user is asked.
    pub leave_prompt: Signal<Option<Leave>>,
    /// Cover images as data URLs, keyed by `source:manga`.
    pub covers: Signal<HashMap<String, std::rc::Rc<str>>>,
    pub toasts: Signal<Vec<Toast>>,
    /// The pointer is over the toasts: they stay while it is.
    pub toasts_hovered: Signal<bool>,
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
        Self {
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
            settings: sig({
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
            }),
            sources: sig(sources),
            health: sig(JsonFile::<HealthDb>::new(paths().health_file()).load()),
            page: sig(first_page),
            search: sig(SearchView::default()),
            search_sort: sig(SearchSort::default()),
            search_filters: sig(crate::logic::SearchFilters::default()),
            search_tags: sig(HashMap::new()),
            settings_section: sig(SettingsSection::default()),
            open_details: sig(None),
            unsaved_reader: sig(None),
            leave_prompt: sig(None),
            covers: sig(HashMap::new()),
            toasts: sig(Vec::new()),
            toasts_hovered: sig(false),
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
            workspace: sig(None),
            parked_workspace: sig(None),
            chapters: sig(ChaptersState::Idle),
            selected: sig(HashSet::new()),
            table: sig(TableView::default()),
            preview: sig(PreviewState::default()),
            export_opts: sig(ExportOpts::default()),
            export_plan: sig(ExportPlan::default()),
            crop_draft: sig(library::CropRect::default()),
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

    /// Go to a page. Unsaved reader settings are asked about first.
    pub fn go(&self, page: Page) {
        self.leave(Leave::Page(page));
    }

    /// Go somewhere that would lose the reader form's unsaved changes, asking
    /// what to do with them first; anywhere else, just go.
    pub fn leave(&self, to: Leave) {
        let staying = match &to {
            Leave::Page(page) => *page == Page::Settings,
            Leave::Section(section) => *section == SettingsSection::Reader,
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
    pub fn leave_now(&self, to: Leave) {
        match to {
            Leave::Page(page) => self.go_now(page),
            Leave::Section(section) => {
                let mut current = self.settings_section;
                current.set(section);
            }
            Leave::Reader(id) => {
                let mut settings = self.settings;
                settings.write().device = id;
                self.save_settings();
            }
        }
    }

    fn go_now(&self, page: Page) {
        let mut p = self.page;
        // The Export screen borrows the workspace for its own series; the
        // series that was open before it comes back when it is left.
        if *p.peek() == Page::Export && page != Page::Export {
            let mut parked = self.parked_workspace;
            let back = parked.write().take();
            let mut workspace = self.workspace;
            workspace.set(back);
        }
        p.set(page);
    }

    /// Leave the open series for the page it was opened from.
    pub fn close_series(&self, back: Page) {
        let mut workspace = self.workspace;
        workspace.set(None);
        self.go(back);
    }

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

    pub fn save_settings(&self) {
        let _ = JsonFile::new(paths().settings_file()).save(&*self.settings.read());
    }

    pub fn save_sources(&self) {
        let _ = JsonFile::new(paths().sources_file()).save(&*self.sources.read());
    }

    pub fn save_health(&self) {
        let _ = JsonFile::new(paths().health_file()).save(&*self.health.read());
    }

    pub fn save_library(&self) {
        let _ = JsonFile::new(paths().library_file()).save(&*self.library.read());
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
        // Kept on disk, so what is waiting and what has finished survive a restart.
        let _ = JsonFile::new(paths().queue_file()).save(&guard.to_saved());
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
