//! The JavaScript side of the engine (compiled for `wasm32` only).
//!
//! Every function here only converts between JavaScript values and the plain
//! Rust functions of the other modules, which hold the logic and the tests. See
//! the contract in the crate documentation. Errors are thrown as `Error`.

#[cfg(feature = "convert")]
use js_sys::{Array, Object, Reflect};
#[cfg(feature = "core")]
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

fn error(message: impl std::fmt::Display) -> JsError {
    JsError::new(&message.to_string())
}

/// The version of this engine.
#[wasm_bindgen]
pub fn engine_version() -> String {
    crate::engine_version().to_string()
}

/// The readers the site knows, as JSON.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn devices_json() -> String {
    crate::devices::devices_json()
}

/// The export options a device and a preset start from, as JSON.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn default_options_json(device_id: &str, preset: &str) -> Result<String, JsError> {
    crate::options::default_options_json(device_id, preset).map_err(error)
}

/// The chapters, page order, cover and title of a book from its entry names.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn guess_chapters_json(entries_json: &str) -> Result<String, JsError> {
    crate::chapters::guess_chapters_json(entries_json).map_err(error)
}

/// What a `ComicInfo.xml` says, as JSON.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn parse_comicinfo_json(xml: &str) -> Result<String, JsError> {
    crate::chapters::parse_comicinfo_json(xml).map_err(error)
}

/// Check a reading-state file (text or bytes) and give it back cleaned.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn state_parse_json(input: JsValue) -> Result<String, JsError> {
    if let Some(text) = input.as_string() {
        return crate::state::parse_str(&text)
            .map(|s| crate::state::to_json_pretty(&s))
            .map_err(error);
    }
    let bytes = Uint8Array::new(&input).to_vec();
    crate::state::parse_json(&bytes).map_err(error)
}

/// Merge two reading-state files (as text).
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn state_merge_json(a: &str, b: &str) -> Result<String, JsError> {
    crate::state::merge_json(a, b).map_err(error)
}

/// Make the file to save from the page's records.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn state_to_json(records_json: &str) -> Result<String, JsError> {
    crate::state::to_json(records_json).map_err(error)
}

/// The quick identity of a file: its size, first and last 256 KiB.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub fn book_fingerprint_head_tail(size: f64, head: &[u8], tail: &[u8]) -> String {
    let size = if size.is_finite() && size > 0.0 { size as u64 } else { 0 };
    crate::fingerprint::book_fingerprint_head_tail(size, head, tail)
}

/// An incremental SHA-256 of what is fed to it.
#[cfg(feature = "core")]
#[wasm_bindgen]
pub struct Fingerprinter {
    inner: Option<crate::fingerprint::Fingerprinter>,
}

#[cfg(feature = "core")]
#[wasm_bindgen]
impl Fingerprinter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Fingerprinter {
        Fingerprinter { inner: Some(crate::fingerprint::Fingerprinter::new()) }
    }

    /// Feed the next bytes.
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| error("This fingerprint is already finished."))?
            .update(bytes);
        Ok(())
    }

    /// The hash of everything fed (64 hex digits). The fingerprinter is spent after this.
    pub fn finish(&mut self) -> Result<String, JsError> {
        self.inner
            .take()
            .map(crate::fingerprint::Fingerprinter::finish)
            .ok_or_else(|| error("This fingerprint is already finished."))
    }
}

#[cfg(feature = "core")]
impl Default for Fingerprinter {
    fn default() -> Self {
        Self::new()
    }
}

/// An export in progress (worker build only).
#[cfg(feature = "convert")]
#[wasm_bindgen]
pub struct Exporter {
    inner: crate::export::Exporter,
}

#[cfg(feature = "convert")]
#[wasm_bindgen]
impl Exporter {
    #[wasm_bindgen(constructor)]
    pub fn new(options_json: &str) -> Result<Exporter, JsError> {
        Ok(Exporter { inner: crate::export::Exporter::new(options_json).map_err(error)? })
    }

    /// Convert one chapter: `pages` is an array of `Uint8Array`, the stored
    /// pictures in reading order; `number` is `NaN` for none. Returns how many
    /// pages the chapter made.
    pub fn add_chapter(&mut self, name: &str, number: f64, pages: Array) -> Result<u32, JsError> {
        let mut bytes: Vec<Vec<u8>> = Vec::with_capacity(pages.length() as usize);
        for page in pages.iter() {
            bytes.push(Uint8Array::new(&page).to_vec());
        }
        let number = Some(number).filter(|n| n.is_finite());
        let made = self.inner.add_chapter(name, number, &bytes).map_err(error)?;
        Ok(made as u32)
    }

    /// Close the books; then use `file_count` and `take_file`.
    pub fn finish(&mut self) -> Result<(), JsError> {
        self.inner.finish().map_err(error)
    }

    pub fn file_count(&self) -> u32 {
        self.inner.file_count() as u32
    }

    /// File `i` as `{ name, bytes }`. Each file can be taken once.
    pub fn take_file(&mut self, i: u32) -> Result<JsValue, JsError> {
        let file = self.inner.take_file(i as usize).map_err(error)?;
        let out = Object::new();
        Reflect::set(&out, &"name".into(), &file.name.into()).map_err(|_| error("could not build the file"))?;
        let bytes = Uint8Array::from(file.bytes.as_slice());
        drop(file.bytes);
        Reflect::set(&out, &"bytes".into(), &bytes.into()).map_err(|_| error("could not build the file"))?;
        Ok(out.into())
    }
}
