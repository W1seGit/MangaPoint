//! RAR and CBR archives, read with the vendored UnRAR source (its licence is
//! in THIRD-PARTY-LICENSES.md). Like a solid 7Z, a RAR is read from the front in
//! one pass.

use super::archive::{build_book, scan_wanted, Entry};
use super::{
    comicinfo, fail, BookSource, Format, ImportBook, MAX_ENTRIES, MAX_PAGE_BYTES,
    MAX_SCAN_ENTRY_BYTES, MAX_SCAN_TOTAL_BYTES,
};
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use unrar::Archive;

fn label_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn normal(name: &std::path::Path) -> String {
    name.to_string_lossy().replace('\\', "/")
}

fn explain(path: &Path, e: &unrar::error::UnrarError) -> crate::error::PipelineError {
    use unrar::error::Code;
    let label = label_of(path);
    match e.code {
        Code::MissingPassword | Code::BadPassword => {
            fail(format!("{label} is protected by a password, which MangaPoint cannot use."))
        }
        Code::EOpen | Code::EReference => fail(format!(
            "{label} is a part of a multi-part RAR, and the other parts are missing. Put all the parts in one folder and import the first."
        )),
        _ => fail(format!("{label} is not a readable RAR archive ({e})")),
    }
}

/// The files in the archive, and whether any is locked.
fn list(path: &Path) -> Result<(Vec<Entry>, bool)> {
    let archive = Archive::new(path)
        .open_for_listing()
        .map_err(|e| explain(path, &e))?;
    let mut entries = Vec::new();
    let mut locked = false;
    for header in archive {
        let header = header.map_err(|e| explain(path, &e))?;
        if header.is_directory() {
            continue;
        }
        if header.is_encrypted() {
            locked = true;
        }
        entries.push(Entry { name: normal(&header.filename), size: header.unpacked_size });
        if entries.len() > MAX_ENTRIES {
            return Err(fail(format!("{} holds too many files", label_of(path))));
        }
    }
    Ok((entries, locked))
}

/// Read the named entries in one pass; each is given to `each`. Ones that
/// cannot be read are given empty.
fn read_named(
    path: &Path,
    wanted: &HashSet<String>,
    limit: u64,
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let mut archive = Archive::new(path)
        .open_for_processing()
        .map_err(|e| explain(path, &e))?;
    // The pass ends when every wanted name has been given once. An archive can
    // hold a name twice: the first is the one used, the later ones are passed by.
    let mut remaining = wanted.len();
    let mut given: HashSet<String> = HashSet::new();
    while remaining > 0 {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let Some(header) = archive.read_header().map_err(|e| explain(path, &e))? else {
            break;
        };
        let name = normal(&header.entry().filename);
        if header.entry().is_file() && wanted.contains(&name) && !given.contains(&name) {
            given.insert(name.clone());
            remaining -= 1;
            if header.entry().unpacked_size > limit.min(MAX_PAGE_BYTES) {
                // Too big to be taken: it counts as a picture that cannot be read.
                each(&name, Vec::new())?;
                archive = header.skip().map_err(|e| explain(path, &e))?;
                continue;
            }
            match header.read() {
                Ok((bytes, next)) => {
                    each(&name, bytes)?;
                    archive = next;
                }
                Err(e) => {
                    // Cannot go on in a solid archive after a bad entry.
                    each(&name, Vec::new())?;
                    return Err(explain(path, &e));
                }
            }
        } else {
            archive = header.skip().map_err(|e| explain(path, &e))?;
        }
    }
    Ok(())
}

pub(crate) fn scan(path: &Path, cancel: &CancelFlag) -> Result<ImportBook> {
    let label = label_of(path);
    let (entries, locked) = list(path)?;
    if entries.is_empty() {
        return Err(fail(format!("{label} is not a readable RAR archive: it is empty or damaged.")));
    }
    if locked {
        return Err(fail(format!("{label} is protected by a password, which MangaPoint cannot use.")));
    }
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
    let comic = got
        .iter()
        .filter(|(n, _)| n.to_lowercase().ends_with("comicinfo.xml"))
        .min_by_key(|(n, _)| n.len())
        .and_then(|(_, b)| comicinfo::parse(b));
    build_book(
        &label,
        Format::Rar,
        BookSource::Rar(path.to_path_buf()),
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
    let outcome = read_named(path, &wanted, MAX_PAGE_BYTES, cancel, &mut |name, bytes| {
        seen.insert(name.to_string());
        each(name, bytes)
    });
    if matches!(outcome, Err(PipelineError::Cancelled)) {
        return outcome;
    }
    // Entries not reached count as pages that cannot be read.
    for key in keys.iter().filter(|k| !seen.contains(*k)) {
        each(key, Vec::new())?;
    }
    outcome
}
