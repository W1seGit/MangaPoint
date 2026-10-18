//! Running and talking to a local Suwayomi server.
//!
//! * [`release`]: the pinned Suwayomi version and its checksums
//! * [`provision`]: download, verify and unpack it
//! * [`server`]: start and stop the process (loopback only, password protected)
//! * [`client`]: typed GraphQL calls (extensions, sources, chapters, downloads)
//! * [`browser`]: the optional Chromium "browser support" download
//!
//! Nothing here knows about the UI or the converter.

pub mod api;
pub mod browser;
pub mod client;
pub mod error;
#[cfg(feature = "fake")]
pub mod fake;
pub mod provision;
pub mod release;
pub mod server;

pub use api::Catalog;
pub use client::{Limits, 
    Browse, BrowsePage, Chapter, Extension, MangaDetails, MangaSummary, Source, SuwayomiClient,
};
pub use error::{CatalogError, Result};
pub use provision::{provision, Install, Progress};
pub use server::{RunningServer, ServerConfig};
