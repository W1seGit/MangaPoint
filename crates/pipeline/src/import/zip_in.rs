//! CBZ and ZIP: comic archives. (An EPUB is a ZIP too, and is passed on.)

use super::archive::{build_book, Entry};
use super::names::natural_cmp;
use super::{comicinfo, epub_in, fail, BookSource, Format, ImportBook, MAX_ENTRIES, MAX_PAGE_BYTES};
use crate::error::{PipelineError, Result};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use zip::ZipArchive;

pub(crate) type Archive = ZipArchive<BufReader<File>>;

pub(crate) fn open(path: &Path) -> Result<Archive> {
    let file = File::open(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    let archive = ZipArchive::new(BufReader::new(file)).map_err(|e| {
        fail(format!("{} is not a readable ZIP or CBZ ({e})", name(path)))
    })?;
    if archive.len() > MAX_ENTRIES {
        return Err(fail(format!("{} holds too many files", name(path))));
    }
    Ok(archive)
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Read one entry by name, never more than a page may be.
pub(crate) fn read_entry(archive: &mut Archive, entry: &str) -> Result<Vec<u8>> {
    let file = archive
        .by_name(entry)
        .map_err(|e| fail(format!("{entry}: {e}")))?;
    let mut bytes = Vec::new();
    file.take(MAX_PAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail(format!("{entry}: {e}")))?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(fail(format!("{entry} is too large to be a page")));
    }
    Ok(bytes)
}

/// Every file in an archive, with whether any is locked by a password.
pub(crate) fn list(archive: &mut Archive) -> (Vec<Entry>, bool) {
    let mut entries = Vec::new();
    let mut locked = false;
    for i in 0..archive.len() {
        let Ok(file) = archive.by_index_raw(i) else {
            continue;
        };
        if file.is_dir() {
            continue;
        }
        if file.encrypted() {
            locked = true;
        }
        entries.push(Entry {
            name: file
                .name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(file.name_raw()).into_owned()),
            size: file.size(),
        });
    }
    (entries, locked)
}

pub(crate) fn scan(path: &Path) -> Result<ImportBook> {
    let label = name(path);
    let mut archive = open(path)?;
    let (entries, locked) = list(&mut archive);
    if entries.iter().any(|e| e.name.eq_ignore_ascii_case("META-INF/container.xml")) {
        return epub_in::scan(path, &mut archive, entries);
    }
    if locked {
        return Err(fail(format!(
            "{label} is protected by a password, which MangaPoint cannot use."
        )));
    }
    let comic = entries
        .iter()
        .filter(|e| {
            let lower = e.name.to_lowercase();
            lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml")
        })
        .min_by(|a, b| natural_cmp(&a.name, &b.name))
        .and_then(|e| read_entry(&mut archive, &e.name).ok())
        .and_then(|bytes| comicinfo::parse(&bytes));
    let format = if super::extension(path) == "cbz" { Format::Cbz } else { Format::Zip };
    build_book(
        &label,
        format,
        BookSource::Zip(path.to_path_buf()),
        &entries,
        comic,
        &mut |entry| read_entry(&mut archive, entry).ok(),
    )
}

pub(crate) fn visit(
    path: &Path,
    keys: &[String],
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let mut archive = open(path)?;
    for key in keys {
        match read_entry(&mut archive, key) {
            Ok(bytes) => each(key, bytes)?,
            Err(_) => each(key, Vec::new())?,
        }
    }
    Ok(())
}
