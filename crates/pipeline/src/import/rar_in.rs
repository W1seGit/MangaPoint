//! RAR and CBR archives, read with the vendored UnRAR source (its licence is
//! in THIRD-PARTY-LICENSES.md). Like a solid 7Z, a RAR is read from the front in
//! one pass.

use super::archive::{build_book, Entry};
use super::{comicinfo, fail, is_page_name, BookSource, Format, ImportBook, MAX_ENTRIES, MAX_PAGE_BYTES};
use crate::error::Result;
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
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let mut archive = Archive::new(path)
        .open_for_processing()
        .map_err(|e| explain(path, &e))?;
    let mut remaining = wanted.len();
    while remaining > 0 {
        let Some(header) = archive.read_header().map_err(|e| explain(path, &e))? else {
            break;
        };
        let name = normal(&header.entry().filename);
        if header.entry().is_file() && wanted.contains(&name) && header.entry().unpacked_size <= MAX_PAGE_BYTES {
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
            remaining -= 1;
        } else {
            archive = header.skip().map_err(|e| explain(path, &e))?;
        }
    }
    Ok(())
}

pub(crate) fn scan(path: &Path) -> Result<ImportBook> {
    let label = label_of(path);
    let (entries, locked) = list(path)?;
    if entries.is_empty() {
        return Err(fail(format!("{label} is not a readable RAR archive: it is empty or damaged.")));
    }
    if locked {
        return Err(fail(format!("{label} is protected by a password, which MangaPoint cannot use.")));
    }
    let mut pages: Vec<&Entry> = entries.iter().filter(|e| is_page_name(&e.name)).collect();
    pages.sort_by(|a, b| super::natural_cmp(&a.name, &b.name));
    let mut wanted: HashSet<String> = HashSet::new();
    for e in &entries {
        let lower = e.name.to_lowercase();
        if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            wanted.insert(e.name.clone());
        }
    }
    if let Some(first) = pages.first() {
        wanted.insert(first.name.clone());
    }
    for e in &pages {
        let stem = e.name.rsplit('/').next().unwrap_or("").rsplit_once('.').map_or("", |(s, _)| s).to_lowercase();
        if matches!(stem.as_str(), "cover" | "folder" | "front" | "frontcover") {
            wanted.insert(e.name.clone());
        }
    }
    let mut got: HashMap<String, Vec<u8>> = HashMap::new();
    read_named(path, &wanted, &mut |name, bytes| {
        got.insert(name.to_string(), bytes);
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
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let wanted: HashSet<String> = keys.iter().cloned().collect();
    let mut seen: HashSet<String> = HashSet::new();
    let outcome = read_named(path, &wanted, &mut |name, bytes| {
        seen.insert(name.to_string());
        each(name, bytes)
    });
    // Entries not reached count as pages that cannot be read.
    for key in keys.iter().filter(|k| !seen.contains(*k)) {
        each(key, Vec::new())?;
    }
    outcome
}
