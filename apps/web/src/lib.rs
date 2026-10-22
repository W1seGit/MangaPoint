//! The MangaPoint website: read, import and export your own books in the
//! browser. Nothing is uploaded; see `docs/web.md`.
//!
//! The pure parts (the library's bookkeeping, the import review, the reader's
//! decisions) compile and are tested natively. The screens and the bridge to
//! the browser exist only for `wasm32`.


pub mod art;
/// The desktop app's number and text helpers (one source of truth).
#[path = "../../desktop/src/logic/format.rs"]
pub mod format;
pub mod logic;
pub mod model;
pub mod ranges;
#[cfg(target_arch = "wasm32")]
mod update_cat;
pub mod reader_logic;
pub mod review;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod bridge;
#[cfg(target_arch = "wasm32")]
mod ui;

/// The quick identity of a file (see the engine): the page's JavaScript calls
/// this while it reads the head and tail of a picked file.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn mp_fingerprint(size: f64, head: &[u8], tail: &[u8]) -> String {
    web_engine::fingerprint::book_fingerprint_head_tail(size as u64, head, tail)
}

/// Chapters, page order and a title from the names inside a book (the engine).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn mp_guess_chapters(input_json: &str) -> Result<String, wasm_bindgen::JsError> {
    web_engine::chapters::guess_chapters_json(input_json).map_err(|e| wasm_bindgen::JsError::new(&e))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    dioxus::launch(app::App);
}
