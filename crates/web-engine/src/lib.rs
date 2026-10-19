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
//! ## The contract with the page (JavaScript names, for the worker build)
//!
//! All structured values cross the boundary as JSON strings, bytes as
//! `Uint8Array`. Errors are thrown as `Error` with a plain sentence.
//!
//! * `engine_version() -> string`
//! * `devices_json() -> string` — the readers the app knows (id, name,
//!   width, height, ppi, gray levels, formats it opens, `color`), including the
//!   "phone or tablet" entry.
//! * `default_options_json(device_id, preset) -> string` — export options
//!   (`preset` is `"manga" | "manhwa" | "manhua"`).
//! * `class Exporter(options_json)`:
//!   * `add_chapter(name: string, number: number /* NaN = none */, pages: Uint8Array[]) -> number`
//!     converts one chapter and returns how many pages it made. Pages are the
//!     stored bytes of the chapter's pictures, in reading order. Cancelling is
//!     done by dropping the worker.
//!   * `file_count() -> number`, `take_file(i) -> { name: string, bytes: Uint8Array }`
//!     after `finish()`; `finish()` closes the book(s) (one file per series,
//!     per chapter or every N, as the options say).
//! * `class Fingerprinter`: `new()`, `update(bytes)`, `finish() -> string`
//!   (hex SHA-256 over what was fed; the page feeds it slices of a big file).
//! * `book_fingerprint_head_tail(size, head: Uint8Array, tail: Uint8Array) -> string`
//!   — the quick identity of a file: its size, first and last 256 KiB.
//! * `guess_chapters_json(entries_json) -> string` — from the names of the
//!   pictures in an archive or folder (`[{"path": "...", "size": n}]`), the
//!   book's chapters: `[{"name","number","first","count"}]` in reading order,
//!   with the importer's rules (natural order, chapter folders, number
//!   words), and a series title guess.
//! * `parse_comicinfo_json(xml) -> string`.
//! * `state_*`: see `state.rs` (the reading-state file format and its merge).
//!
//! The first line of a conversion's output must equal what the desktop app
//! makes for the same pages and options (tests compare bytes natively).
