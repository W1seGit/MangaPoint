//! Everything MangaPoint remembers between runs, stored as small JSON files.
//!
//! * [`paths`]: where files live
//! * [`json`]: crash-safe load/save of one JSON document
//! * [`settings`]: user preferences (device, output, conversion defaults)
//! * [`sources`]: first-run choices and added extension stores
//! * [`health`]: source test results and real-use success/latency, which
//!   feed the search ranking

pub mod downloads;
pub mod exports;
pub mod health;
pub mod json;
pub mod paths;
pub mod presets;
pub mod reading;
pub mod settings;
pub mod sources;
pub mod speeds;

pub use downloads::{
    chapter_key, remove_inside, series_id, status_label, DownloadedChapter, DownloadedSeries,
    LibraryDb, LibraryFiles, SavedSource, SeriesInfo, SAMPLE_TITLE,
};
pub use exports::{delete_exported_file, ExportRecord, ExportedFileRecord, ExportsDb};
pub use health::{HealthDb, StepTiming, TestRecord, Verdict};
pub use json::{JsonError, JsonFile};
pub use paths::AppPaths;
pub use presets::{
    builtin_presets, ConversionPreset, Deleted, PresetEntry, PresetError, PresetsDb,
};
pub use settings::{
    Animations, ConversionField, ConversionSettings, DownloadLimits, DownloadSettings,
    DownloadSpeed, ExportMethod, ExportSettings, Orientation, OutputFormat, PageQuality, ReaderSetup,
    Settings,
    SetupStep,
};
pub use reading::{ChapterRead, PageDirection, ReadFit, ReadMode, ReadProgress, ReaderPrefs};
pub use sources::{Choice, SourceState};
pub use speeds::{SourceSpeed, SpeedDb};
pub mod crops;
pub use crops::{CropOverrides, CropRect, CropScope};
