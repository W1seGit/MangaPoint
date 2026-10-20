//! The engine behind the MangaPoint website (see `apps/web`).
//!
//! Everything here is plain Rust that is unit-tested natively; the
//! `wasm-bindgen` glue lives in `glue.rs` and is compiled only for
//! `wasm32`. The website runs this in a Web Worker so the page never freezes,
//! and keeps NOTHING on a server: every book stays on the visitor's device.
//!
//! The crate has two cargo features. `core` is what the page itself links
//! (`apps/web` depends on this crate as a normal Rust library and calls the
//! pure functions directly, no JavaScript in between). `convert` adds the
//! converter and the file writers; it is built as its own `.wasm` that only the
//! Web Worker loads, so the page's bundle stays small.
//!
//! ## The Rust API
//!
//! | module | feature | what |
//! |---|---|---|
//! | [`state`] | core | the reading-state file: [`state::parse`], [`state::merge`], [`state::to_json_pretty`], [`state::from_records`] |
//! | [`fingerprint`] | core | [`fingerprint::Fingerprinter`], [`fingerprint::book_fingerprint_head_tail`] |
//! | [`chapters`] | core | [`chapters::guess`] / [`chapters::guess_chapters_json`], [`chapters::parse_comicinfo_json`] |
//! | [`devices`] | core | [`devices::devices`] / [`devices::devices_json`] |
//! | [`options`] | core | [`options::ExportOptions`], [`options::default_options_json`] |
//! | [`export`] | convert | [`export::Exporter`] |
//!
//! ## The contract with the page (JavaScript names, for the worker build)
//!
//! All structured values cross the boundary as JSON strings, bytes as
//! `Uint8Array`. Errors are thrown as `Error` with a plain sentence.
//!
//! * `engine_version() -> string`
//! * `devices_json() -> string` — the readers the app knows (`id`, `name`,
//!   `maker`, `width`, `height`, `ppi`, `grays`, `formats`, `color`,
//!   `crosspoint`, `note`, `original`), including the "phone or tablet" entry.
//! * `default_options_json(device_id, preset) -> string` — export options
//!   (`preset` is `"manga" | "manhwa" | "manhua"`); the options are described in
//!   [`options`].
//! * `class Exporter(options_json)`:
//!   * `add_chapter(name: string, number: number /* NaN = none */, pages: Uint8Array[]) -> number`
//!     converts one chapter and returns how many pages it made. Pages are the
//!     stored bytes of the chapter's pictures, in reading order. Cancelling is
//!     done by dropping the worker.
//!   * `finish()` closes the book(s) (one file per series, per chapter or every
//!     N, as the options say); then `file_count() -> number` and
//!     `take_file(i) -> { name: string, bytes: Uint8Array }` (each file can be
//!     taken once, so its memory is freed as soon as the page has saved it).
//! * `class Fingerprinter`: `new()`, `update(bytes)`, `finish() -> string`
//!   (hex SHA-256 over what was fed; the page feeds it slices of a big file).
//! * `book_fingerprint_head_tail(size, head: Uint8Array, tail: Uint8Array) -> string`
//!   — the quick identity of a file: its size, first and last 256 KiB.
//! * `guess_chapters_json(entries_json) -> string` — from the names of the
//!   pictures in an archive or folder (`[{"path": "...", "size": n}]`, or an
//!   object with `label`/`kind`/`comicinfo`, see [`chapters`]), the book's
//!   chapters in reading order, the page order, the cover and a title guess,
//!   with the importer's rules (natural order, chapter folders, number words).
//! * `parse_comicinfo_json(xml) -> string`.
//! * `state_parse_json(bytes_or_str) -> string`, `state_merge_json(a, b) -> string`,
//!   `state_to_json(records_json) -> string`: see [`state`] (the reading-state
//!   file format and its merge).
//!
//! The output of a conversion equals what the desktop app makes for the same
//! pages and options (the tests compare the bytes natively, and the Node smoke
//! test compares the WebAssembly build with the native one).

#[cfg(feature = "core")]
pub mod chapters;
#[cfg(feature = "core")]
pub mod devices;
#[cfg(feature = "core")]
pub mod fingerprint;
#[cfg(feature = "core")]
pub mod options;
#[cfg(feature = "core")]
pub mod state;

#[cfg(feature = "convert")]
pub mod export;
#[cfg(feature = "convert")]
pub mod sample;

#[cfg(target_arch = "wasm32")]
mod glue;

/// The version of this engine (the crate's version).
pub fn engine_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
