//! MangaPoint conversion engine: pixels in, XTC/XTCH bytes out.
//!
//! This crate does no I/O, networking or UI, so the same code runs in the
//! desktop app and (compiled to WASM) in the website.

#[cfg(feature = "decode")]
pub mod decode;
pub mod dither;
pub mod gray;
pub mod plan;
pub mod process;
pub mod testpattern;
pub mod xtc;

/// The version of what the converter makes from a picture. A cached chapter
/// is only reused while this is the same, so **bump it whenever a change to
/// this crate alters the pixels or bytes it produces** for the same input and
/// options (resizing, trimming, planning, tones, dithering, XTC/XTH packing).
/// A change that leaves every output byte as it was (speed, cancelling, memory)
/// must not bump it.
pub const CONVERTER_REV: u32 = 1;

pub use dither::{quantize, Dither};
pub use gray::{ColorImage, Depth, GrayImage, ImageError, LevelImage};
pub use plan::{ChapterPlan, PlannedPage, Prepared, Source};
pub use process::{
    crop_bounds, process_page, CropMode, Layout, Margins, ProcessOptions, SpreadMode, StripCut,
};

#[cfg(test)]
mod reference;
#[cfg(test)]
mod color_tests;
