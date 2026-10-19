//! Background work: opening a series, downloading, previewing, exporting,
//! and deleting. Everything here runs with `spawn_forever`, so leaving the
//! screen that started it does not stop it.
//!
//! Process-wide state lives in `preview` (the decoded chapter, the prepared
//! preview and the request counter that tells a newer preview from an older
//! one); everything else is held in `AppState`.

use crate::logic::{self, JobProgress};
use crate::queue::{Batch, BatchStatus};
use crate::state::{
    apply_library_dirs, library_files, now_secs, paths, spawn_forever, AppState, ChaptersState,
    DownloadDialog, ExportItem, ExportOpts, ExportPlan, GroupingChoice, JobKind, JobView,
    LoadedChapters, Page, PendingDownload, PlanChapters, PlanEntry, PreviewPage, PreviewState,
    Shot, TableView, WaitingExport, Workspace,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use futures_util::future::join;
use futures_util::stream::{self, StreamExt};
use library::{chapter_key, ExportRecord, ExportedFileRecord};
use pipeline::{
    download_chapters, export_series, requests_from_merged, CancelFlag, ExportJob, ExportTarget,
    Grouping, JobEvent, PipelineError, PreviewImage, RetryPolicy, SourceChoice,
};
use search::{merge_chapters, RankedSeries, ScoredMember};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

mod delete;
mod download;
mod export;
mod import;
mod open;
mod preview;

pub use delete::*;
pub use download::*;
pub use export::*;
pub use import::*;
pub use open::*;
pub use preview::*;
