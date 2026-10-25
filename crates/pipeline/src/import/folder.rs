//! Folders and loose pictures: a folder of pictures is a chapter, a folder of
//! folders of pictures is a series of chapters, and a folder of archives is a
//! series with one chapter in each.

use super::names::{chapter_name, chapter_number, clean_name, mangapoint_chapter, mangapoint_title, natural_cmp};
use super::{
    extension, fail, scan_file, tiff_in, BookSource, Format, ImportBook, ImportChapter, PageRef,
    MAX_ENTRIES, MAX_PAGE_BYTES, PAGE_EXTENSIONS,
};
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use std::io::Read;
use std::path::{Path, PathBuf};

const BOOK_EXTENSIONS: [&str; 8] = ["cbz", "zip", "cbr", "rar", "cb7", "7z", "pdf", "epub"];
const MAX_DEPTH: usize = 8;

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_cover(name: &str) -> bool {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_lowercase();
    matches!(stem.as_str(), "cover" | "folder" | "front" | "frontcover")
}

/// What a folder holds, below it.
#[derive(Default)]
struct Tree {
    /// (folder relative to the root with `/`, picture file names in it)
    pictures: Vec<(String, Vec<String>)>,
    books: Vec<PathBuf>,
    count: usize,
}

fn walk(root: &Path, rel: &str, depth: usize, tree: &mut Tree, cancel: &CancelFlag) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Err(fail(format!("{} cannot be read", dir.display())));
    };
    let mut pictures = Vec::new();
    let mut subdirs = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        tree.count += 1;
        if tree.count > MAX_ENTRIES {
            return Err(fail("the folder holds too many files"));
        }
        let path = entry.path();
        let kind = entry.file_type().ok();
        if kind.is_some_and(|k| k.is_dir()) {
            subdirs.push(name);
        } else if kind.is_some_and(|k| k.is_file()) {
            let ext = extension(&path);
            // (`.img` is a page as MangaPoint keeps it in its own library folder.)
            if PAGE_EXTENSIONS.contains(&ext.as_str()) || ext == "img" {
                pictures.push(name);
            } else if BOOK_EXTENSIONS.contains(&ext.as_str()) {
                tree.books.push(path);
            }
        }
    }
    if !pictures.is_empty() {
        pictures.sort_by(|a, b| natural_cmp(a, b));
        tree.pictures.push((rel.to_string(), pictures));
    }
    if depth < MAX_DEPTH {
        for sub in subdirs {
            let child = if rel.is_empty() { sub } else { format!("{rel}/{sub}") };
            walk(root, &child, depth + 1, tree, cancel)?;
        }
    }
    Ok(())
}

/// The pages of a picture file: one, or one for each picture of a TIFF that
/// holds several.
fn pages_of(root: &Path, rel: &str, warnings: &mut Vec<String>) -> Vec<PageRef> {
    let path = root.join(rel);
    // (A TIFF is counted by opening it; the cancel flag is checked between files.)
    if matches!(extension(&path).as_str(), "tif" | "tiff") {
        return match tiff_in::count_pages(&path) {
            Ok(1) => vec![PageRef { source: 0, key: rel.to_string() }],
            Ok(n) => (0..n)
                .map(|i| PageRef { source: 0, key: format!("{rel}#{i}") })
                .collect(),
            Err(e) => {
                warnings.push(format!("{rel}: {e}"));
                Vec::new()
            }
        };
    }
    vec![PageRef { source: 0, key: rel.to_string() }]
}

fn read_capped(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    let mut bytes = Vec::new();
    file.take(MAX_PAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(fail(format!("{} is too large to be a page", path.display())));
    }
    Ok(bytes)
}

/// A TIFF that is open, with the path it was opened from.
type OpenTiff = Option<(PathBuf, tiff_in::Reader)>;

/// The bytes of a picture file as a page: a TIFF becomes a PNG. `tiff` keeps
/// the TIFF open between calls, so the pictures of one file are read from a
/// single opening.
fn read_page(root: &Path, key: &str, tiff: &mut OpenTiff) -> Result<Vec<u8>> {
    if key.split('/').any(|part| part == "..") {
        return Err(fail("a page name leaves its folder"));
    }
    let (rel, index) = match key.rsplit_once('#') {
        Some((rel, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            (rel, n.parse().unwrap_or(0))
        }
        _ => (key, 0),
    };
    let path = root.join(rel);
    if matches!(extension(&path).as_str(), "tif" | "tiff") {
        if tiff.as_ref().is_none_or(|(open, _)| *open != path) {
            *tiff = Some((path.clone(), tiff_in::Reader::open(&path)?));
        }
        match tiff.as_mut() {
            Some((_, reader)) => reader.read_page(index),
            None => Err(fail("the TIFF is not open")),
        }
    } else {
        read_capped(&path)
    }
}

pub(crate) fn visit(
    root: &Path,
    keys: &[String],
    cancel: &CancelFlag,
    each: &mut dyn FnMut(&str, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let mut tiff: OpenTiff = None;
    for key in keys {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        match read_page(root, key, &mut tiff) {
            Ok(bytes) => each(key, bytes)?,
            // A page that cannot be read is left out, and counted by the writer.
            Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
            Err(_) => each(key, Vec::new())?,
        }
    }
    Ok(())
}

/// A folder as a book.
pub(crate) fn scan_dir(path: &Path, cancel: &CancelFlag) -> Result<ImportBook> {
    let label = name_of(path);
    let mut tree = Tree::default();
    walk(path, "", 0, &mut tree, cancel)?;
    tree.pictures
        .sort_by(|a, b| natural_cmp(&a.0, &b.0));
    tree.books.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    let mut book = ImportBook::new(label.clone(), Format::Folder, BookSource::Dir(path.to_path_buf()));
    book.meta.title = clean_name(&label);
    // A series folder of MangaPoint's own library: `c12/00001.img` and a `cover.img`. Its chapters
    // are numbered by their folders, and the folder's name gives the title.
    let own_library = tree.pictures.iter().any(|(rel, _)| !rel.is_empty())
        && tree
            .pictures
            .iter()
            .all(|(rel, _)| rel.is_empty() || (!rel.contains('/') && mangapoint_chapter(rel).is_some()));
    if own_library {
        if let Some(title) = mangapoint_title(&label) {
            book.meta.title = title;
        }
    }

    let only_root = tree.pictures.len() == 1 && tree.pictures[0].0.is_empty();
    let mut cover_rel: Option<String> = None;
    for (rel, names) in &tree.pictures {
        // A cover next to folders of chapters is the cover, not a page.
        let in_root = rel.is_empty();
        let apart = in_root && tree.pictures.len() > 1;
        let mut pages = Vec::new();
        for name in names {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let key = if in_root { name.clone() } else { format!("{rel}/{name}") };
            if apart && is_cover(name) {
                cover_rel = cover_rel.or(Some(key));
                continue;
            }
            pages.extend(pages_of(path, &key, &mut book.warnings));
        }
        if pages.is_empty() {
            continue;
        }
        let last = rel.rsplit('/').next().unwrap_or(rel);
        let own = if own_library && !in_root { mangapoint_chapter(last) } else { None };
        let number = match &own {
            Some((n, _)) => *n,
            None => chapter_number(last),
        };
        let name = if let Some((_, name)) = own {
            name
        } else if only_root {
            chapter_name(&label, chapter_number(&label))
        } else if in_root {
            "Loose pages".to_string()
        } else {
            chapter_name(last, number)
        };
        let number = if only_root { chapter_number(&label) } else if in_root { None } else { number };
        book.chapters.push(ImportChapter::new(number, name, pages));
    }

    // Archives in the folder: one chapter each (or the chapters it holds).
    if !tree.books.is_empty() {
        let mut parts = Vec::new();
        for file in &tree.books {
            match scan_file(file, cancel) {
                Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
                Ok(mut part) => {
                    if part.chapters.len() == 1 {
                        let n = name_of(file);
                        let chapter = &mut part.chapters[0];
                        chapter.number = chapter_number(&n).or(chapter.number);
                        chapter.name = chapter_name(&n, chapter.number);
                    }
                    parts.push(part);
                }
                Err(e) => book
                    .warnings
                    .push(format!("{}: {}", name_of(file), e.to_string().trim_start_matches("import: "))),
            }
        }
        if let Some(mut rest) = ImportBook::merge(parts) {
            if book.chapters.is_empty() {
                rest.meta.title = clean_name(&label);
                rest.label = label.clone();
                rest.format = Format::Folder;
                rest.warnings.splice(0..0, book.warnings.drain(..));
                return finish(rest);
            }
            // Pictures and archives together: the archives follow the pictures.
            let offset = book.sources.len();
            book.sources.extend(rest.sources);
            for mut chapter in rest.chapters {
                for page in &mut chapter.pages {
                    page.source += offset;
                }
                book.chapters.push(chapter);
            }
            book.warnings.extend(rest.warnings);
            book.cover = book.cover.take().or(rest.cover);
        }
    }
    if book.chapters.is_empty() {
        return Err(fail(format!("{label} has no page pictures or books in it.")));
    }
    // The cover: one named for it, else the first page.
    if book.cover.is_none() {
        let key = cover_rel.or_else(|| {
            book.chapters
                .first()
                .and_then(|c| c.pages.first())
                .filter(|p| p.source == 0)
                .map(|p| p.key.clone())
        });
        book.cover = key.and_then(|k| read_page(path, &k, &mut None).ok());
    }
    finish(book)
}

fn finish(mut book: ImportBook) -> Result<ImportBook> {
    if book.chapters.is_empty() {
        return Err(fail(format!("{} has nothing to import.", book.label)));
    }
    book.chapters.retain(|c| !c.pages.is_empty());
    Ok(book)
}

/// Pictures picked one by one: one chapter, named for the folder they are in.
pub(crate) fn scan_files(parent: &Path, files: Vec<PathBuf>, cancel: &CancelFlag) -> Result<ImportBook> {
    let label = {
        let n = name_of(parent);
        if n.is_empty() { "Pictures".to_string() } else { n }
    };
    let mut names: Vec<String> = files.iter().map(|f| name_of(f)).collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    let mut book = ImportBook::new(label.clone(), Format::Images, BookSource::Dir(parent.to_path_buf()));
    book.meta.title = clean_name(&label);
    let mut pages = Vec::new();
    for name in &names {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        pages.extend(pages_of(parent, name, &mut book.warnings));
    }
    if pages.is_empty() {
        return Err(fail("none of the pictures could be read."));
    }
    let number = chapter_number(&label);
    book.chapters.push(ImportChapter::new(number, chapter_name(&label, number), pages));
    let cover_name = names.iter().find(|n| is_cover(n)).unwrap_or(&names[0]);
    book.cover = read_page(parent, cover_name, &mut None).ok();
    Ok(book)
}
