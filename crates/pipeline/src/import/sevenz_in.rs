//! 7Z and CB7 archives. A solid archive can only be read from the front, so
//! the pages wanted are taken in one pass.

use super::archive::{build_book, scan_wanted, Entry};
use super::{
    comicinfo, fail, is_page_name, BookSource, Format, ImportBook, MAX_ENTRIES, MAX_PAGE_BYTES,
    MAX_SCAN_ENTRY_BYTES, MAX_SCAN_TOTAL_BYTES,
};
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use sevenz_rust::{Password, SevenZReader};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

fn label_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn open(path: &Path) -> Result<SevenZReader<File>> {
    SevenZReader::open(path, Password::empty()).map_err(|e| {
        let text = e.to_string();
        if text.to_lowercase().contains("password") || text.to_lowercase().contains("aes") {
            fail(format!("{} is protected by a password, which MangaPoint cannot use.", label_of(path)))
        } else {
            fail(format!("{} is not a readable 7Z archive ({text})", label_of(path)))
        }
    })
}

/// Read the named entries in one pass, in the order they are stored. `each`
/// gets every one found; an entry that cannot be read is given empty.
fn read_named(
    path: &Path,
    wanted: &HashSet<String>,
    limit: u64,
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let mut reader = open(path)?;
    let limit = limit.min(MAX_PAGE_BYTES);
    // The pass ends when every wanted name has been given once. An archive can
    // hold a name twice: the first is the one used, the later ones are passed by.
    let mut remaining = wanted.len();
    let mut given: HashSet<String> = HashSet::new();
    let mut failure: Option<PipelineError> = None;
    let outcome = reader.for_each_entries(&mut |entry: &sevenz_rust::SevenZArchiveEntry, data: &mut dyn Read| {
        if cancel.is_cancelled() {
            failure = Some(PipelineError::Cancelled);
            return Ok(false);
        }
        if entry.is_directory() || !wanted.contains(entry.name()) || given.contains(entry.name()) {
            return Ok(true);
        }
        given.insert(entry.name().to_string());
        let mut bytes = Vec::new();
        let read = data.take(limit + 1).read_to_end(&mut bytes);
        let usable = read.is_ok() && bytes.len() as u64 <= limit;
        if let Err(e) = each(entry.name(), if usable { bytes } else { Vec::new() }) {
            failure = Some(e);
            return Ok(false);
        }
        remaining -= 1;
        Ok(remaining > 0)
    });
    if let Some(e) = failure {
        return Err(e);
    }
    if let Err(e) = outcome {
        let text = e.to_string();
        if text.to_lowercase().contains("password") {
            return Err(fail(format!("{} is protected by a password, which MangaPoint cannot use.", label_of(path))));
        }
        // A broken archive gives what it could.
    }
    Ok(())
}

pub(crate) fn scan(path: &Path, cancel: &CancelFlag) -> Result<ImportBook> {
    let label = label_of(path);
    let reader = open(path)?;
    let entries: Vec<Entry> = reader
        .archive()
        .files
        .iter()
        .filter(|f| !f.is_directory() && f.has_stream())
        .map(|f| Entry { name: f.name().to_string(), size: f.size() })
        .collect();
    drop(reader);
    if entries.len() > MAX_ENTRIES {
        return Err(fail(format!("{label} holds too many files")));
    }
    // One pass for the details and the cover.
    let mut pages: Vec<&Entry> = entries.iter().filter(|e| is_page_name(&e.name)).collect();
    pages.sort_by(|a, b| super::natural_cmp(&a.name, &b.name));
    let wanted = scan_wanted(&entries);
    let mut got: HashMap<String, Vec<u8>> = HashMap::new();
    let mut kept = 0u64;
    read_named(path, &wanted, MAX_SCAN_ENTRY_BYTES, cancel, &mut |name, bytes| {
        // What is kept in memory for the details stays small.
        kept += bytes.len() as u64;
        if kept <= MAX_SCAN_TOTAL_BYTES {
            got.insert(name.to_string(), bytes);
        }
        Ok(())
    })?;
    if let Some(first) = pages.first() {
        if got.get(&first.name).is_none_or(|b| b.is_empty()) && entries.iter().any(|e| e.size > 0) {
            // Nothing could be read from the front of it.
            return Err(fail(format!("{label} cannot be read (it may be damaged or protected by a password).")));
        }
    }
    let comic = got
        .iter()
        .filter(|(n, _)| n.to_lowercase().ends_with("comicinfo.xml"))
        .min_by_key(|(n, _)| n.len())
        .and_then(|(_, b)| comicinfo::parse(b));
    build_book(
        &label,
        Format::SevenZ,
        BookSource::SevenZ(path.to_path_buf()),
        &entries,
        comic,
        &mut |name| got.get(name).filter(|b| !b.is_empty()).cloned(),
    )
}

pub(crate) fn visit(
    path: &Path,
    keys: &[String],
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let wanted: HashSet<String> = keys.iter().cloned().collect();
    let mut seen: HashSet<String> = HashSet::new();
    read_named(path, &wanted, MAX_PAGE_BYTES, cancel, &mut |name, bytes| {
        seen.insert(name.to_string());
        each(name, bytes)
    })?;
    // Entries the archive did not have count as pages that cannot be read.
    for key in keys.iter().filter(|k| !seen.contains(*k)) {
        each(key, Vec::new())?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 7z of the given files, made with the library's own compressor.
    pub(crate) fn make_7z(path: &Path, files: &[(&str, Vec<u8>)]) {
        let dir = path.with_extension("src");
        let _ = std::fs::remove_dir_all(&dir);
        for (name, bytes) in files {
            let target = dir.join(name);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, bytes).unwrap();
        }
        sevenz_rust::compress_to_path(&dir, path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
