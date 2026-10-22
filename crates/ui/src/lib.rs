//! What the desktop app (`apps/desktop`) and the website (`apps/web`) both show or decide, kept once
//! so a change lands in both. An app folder keeps only what the other app cannot have; see
//! `docs/architecture.md`.
//!
//! - `art`: the cat, the scenes and the icons.
//! - `host`: how a shared screen reaches the app it is in (`Host`, `UiState`).
//! - `widgets`: the small pieces used by several screens.
//! - `chapter_table`: the chapter table with its filter, range and sorting.
//! - `dialogs`: the question about unsaved reader settings, and the toasts.
//! - `readers`: the list of readers, adding one, and the form for the chosen one.
//! - `logic`: the decisions behind the screens, plain data in and out, tested without a window.

pub mod art;
pub mod host;
pub mod chapter_table;
pub mod dialogs;
pub mod logic;
pub mod readers;
pub mod widgets;
