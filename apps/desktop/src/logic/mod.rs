//! The app's decisions that do not depend on the UI, so they can be tested.
//!
//! One file per topic; everything is re-exported here, so the rest of the app
//! says `logic::name` whichever file the name lives in.

mod chapters;
mod export;
mod format;
mod library_view;
mod progress;
mod search_results;
mod series_info;

pub use chapters::*;
pub use export::*;
pub use format::*;
pub use library_view::*;
pub use progress::*;
pub use search_results::*;
pub use series_info::*;
