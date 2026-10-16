//! MangaPoint conversion engine: pixels in, XTC/XTCH bytes out.
//!
//! This crate does no I/O, networking or UI, so the same code runs in the
//! desktop app and (compiled to WASM) in the website.

#[cfg(feature = "decode")]
pub mod decode;
pub mod dither;
pub mod gray;
pub mod panels;
pub mod plan;
pub mod process;
pub mod testpattern;
pub mod xtc;

pub use dither::{quantize, Dither};
pub use gray::{Depth, GrayImage, ImageError, LevelImage};
pub use panels::{
    find_panels, panel_model_ready, panels_for, panels_from_frames, set_panel_model, PanelModel,
    Rect,
};
pub use plan::{ChapterPlan, PlannedPage, Prepared, Source};
pub use process::{
    crop_bounds, process_page, CropMode, Layout, Margins, PanelFinder, PanelView, ProcessOptions,
    SpreadMode, StripCut,
};
