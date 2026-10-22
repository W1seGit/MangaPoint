//! What the desktop app (`apps/desktop`) and the website (`apps/web`) both show or decide, kept once
//! so a change lands in both. An app folder keeps only what the other app cannot have; see
//! `docs/architecture.md`.
//!
//! - `art`: the cat, the scenes and the icons.
//! - `widgets`: the small pieces used by several screens.
//! - `chapter_table`: the chapter table with its filter, range and sorting.
//! - `logic`: the decisions behind the screens, plain data in and out, tested without a window.

pub mod art;
pub mod chapter_table;
pub mod logic;
pub mod widgets;
