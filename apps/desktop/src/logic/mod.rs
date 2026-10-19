//! The app's decisions that do not depend on the UI, so they can be tested.
//!
//! One file per topic; everything is re-exported here, so the rest of the app
//! says `logic::name` whichever file the name lives in.

mod backoff;
mod busy;
mod chapters;
mod drive_identity;
mod export;
mod format;
mod generation;
mod import_run;
mod library_view;
mod progress;
mod reader;
mod search_results;
mod series_info;

pub use backoff::*;
pub use busy::*;
pub use chapters::*;
pub use drive_identity::*;
pub use export::*;
pub use format::*;
pub use generation::*;
pub use import_run::*;
pub use library_view::*;
pub use progress::*;
pub use reader::*;
pub use search_results::*;
pub use series_info::*;
