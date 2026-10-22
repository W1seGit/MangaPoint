//! What the desktop app (`apps/desktop`) and the website (`apps/web`) both show or decide, kept once
//! so a change lands in both. An app folder keeps only what the other app cannot have; see
//! `docs/architecture.md`.
//!
//! - `art`: the cat, the scenes and the icons.
//! - `host`: how a shared screen reaches the app it is in (`Host`, `UiState`).
//! - `widgets`: the small pieces used by several screens.
//! - `chapter_table`: the chapter table with its filter, range and sorting.
//! - `conversion`: the controls for how pages are converted.
//! - `export`: the Export screen's folding panels and the bars between them.
//! - `dialogs`: the question about unsaved reader settings, and the toasts.
//! - `reader`: the reader's bars, settings, zoom menu and end-of-chapter card.
//! - `readers`: the list of readers, adding one, and the form for the chosen one.
//! - `scripts`: the page scripts both apps run (`crates/ui/js`).
//! - `library`: the Library's cards and the bar for the ticked ones.
//! - `logic`: the decisions behind the screens, plain data in and out, tested without a window.

pub mod art;
pub mod host;
pub mod chapter_table;
pub mod conversion;
pub mod dialogs;
pub mod export;
pub mod library;
pub mod logic;
pub mod reader;
pub mod readers;
pub mod scripts;
pub mod widgets;
