//! Turning chosen chapters into files on the e-reader, in three stages:
//!
//! 1. [`download`]: save chapters into the library (resumable, with
//!    fallback to another source)
//! 2. [`preview`]: show how a saved page will look on the reader, using the
//!    very conversion the export uses
//! 3. [`compose`]: build reader files from the saved chapters, with no
//!    network, then send them with [`export`] (folder, SD card, Wi-Fi)

pub mod balance;
pub mod cbz;
pub mod compose;
mod conversion_cache;
pub mod download;
pub mod epub;
pub mod error;
pub mod export;
pub mod fetch;
pub mod limiter;
pub mod options;
pub mod plan;
pub mod preview;
pub mod probe;
pub mod types;

pub use compose::{export_series, ExportJob, ExportOutcome, Grouping};
pub use download::{download_chapters, DownloadOutcome};
pub use error::PipelineError;
pub use export::{ExportTarget, ExportedFile};
pub use fetch::RetryPolicy;
pub use limiter::Limiter;
pub use options::{process_options, process_options_for};
pub use plan::{
    apply_choice, coverage, rank_sources, top_tier, Chosen, RankedSource, SourceChoice,
};
pub use preview::{
    crop_page_key, levels_to_thumbnail, render_planned_with_crop, render_source_preview,
};
pub use preview::{
    decode_page, levels_to_previews, prepare_chapter, render_gray, render_levels, render_original,
    render_page, render_planned, render_strip_original, sample_page, ChapterPreview,
    OriginalPreview, PreviewImage, RenderedPage,
};
pub use probe::{probe_source, probe_sources, PROBE_PAGES};
pub use types::{
    requests_from_merged, CancelFlag, ChapterRef, ChapterRequest, JobEvent, SkippedChapter,
    SourceUse,
};
