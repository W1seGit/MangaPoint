//! The reading-state file (`.mpstate`), the same file the website saves: where you are in each
//! book you imported, and which chapters you read. Moving it between the app and the website (or
//! another computer) carries your places along. How the file maps to the library is in
//! `web_engine::state`; here it is read, written and kept.

use crate::state::{now_secs, paths, AppState};
use dioxus::prelude::*;
use library::JsonFile;
use std::path::Path;
use web_engine::state::{self, Applied, State};

/// What earlier files held about books this library does not have, so that they go back out when
/// a file is saved again (a file of the website carries every book of the website).
fn other() -> JsonFile<State> {
    JsonFile::new(paths().root().join("reading-state-other.json"))
}

/// The name a saved file is offered under.
pub const FILE_NAME: &str = "mangapoint-reading.mpstate";

/// Write the places of the library, and the books of earlier files, to `path`. Gives how many books
/// the file holds.
pub fn export_to(app: AppState, path: &Path) -> Result<usize, String> {
    let now_ms = now_secs().saturating_mul(1000);
    let file = {
        let db = app.library.peek();
        state::export_from_library(&db, &other().load(), &state::iso_of_ms(now_ms))
    };
    let count = file.books.len();
    std::fs::write(path, state::to_json_pretty(&file)).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(count)
}

/// Take the places of the file at `path` for the books of the library it belongs to.
pub fn import_from(app: AppState, path: &Path) -> Result<Applied, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let file = state::parse(&bytes)?;
    let store = other();
    let (applied, rest) = {
        let mut library = app.library;
        let mut db = library.write();
        state::apply_to_library(&mut db, &file, &store.load())
    };
    store.save(&rest).map_err(|e| format!("Could not keep the other books of the file: {e}"))?;
    app.save_library();
    Ok(applied)
}

/// The sentence for what an import did.
pub fn describe(a: Applied) -> String {
    let mut parts = vec![format!("{} updated", a.updated)];
    if a.kept > 0 {
        parts.push(format!("{} already further along here", a.kept));
    }
    if a.other > 0 {
        parts.push(format!("{} not in this library (kept for the next file you save)", a.other));
    }
    format!("Reading state loaded: {}.", parts.join(", "))
}
