//! Chapters and titles from the names of the pictures in a book.
//!
//! The website never unpacks a book to find its chapters: the page lists the
//! entries of a CBZ/ZIP/EPUB (or the files of a chosen folder) and this module
//! decides, with **exactly the desktop importer's rules**, which of them are
//! pages, in which order, where each chapter starts, what the chapters and the
//! book are called, and which picture is the cover.
//!
//! The name and ComicInfo rules come from `formats`, shared with the desktop importer.
//! Archive and folder IO stays in the pipeline; the tests
//! feed the same books to the real importer and compare the answers.
//!
//! ## JavaScript names
//!
//! `guess_chapters_json(input)` takes either a bare list of entries, or an object:
//!
//! ```json
//! { "label": "One Piece v01.cbz",       // the file's or folder's name
//!   "kind": "archive",                   // "archive" (default; also EPUB), "folder" or "images"
//!   "comicinfo": "<ComicInfo>...",       // text of the book's ComicInfo.xml, if it has one
//!   "entries": [ { "path": "ch1/001.jpg", "size": 123456 } ] }
//! ```
//!
//! and answers
//!
//! ```json
//! { "title": "One Piece",
//!   "chapters": [ { "name": "Chapter 1", "number": 1, "first": 0, "count": 20 } ],
//!   "pages": [ 3, 0, 1 ],      // entry indices of the pages, in reading order
//!   "cover": 3,                 // entry index of the cover picture, or null
//!   "comicinfo": 7,             // entry index of the ComicInfo.xml to read, or null
//!   "warnings": [], "meta": { "series": "", "author": "", ... } }
//! ```
//!
//! `first` and `count` count positions in `pages`, so chapter `i` is
//! `pages[first .. first + count]`. `number` is `null` when a chapter has none.
//! `kind: "archive"` is also what to use for an EPUB's pictures; the desktop
//! importer reads an EPUB's own table of contents, which this module cannot see.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use formats::{comicinfo, names};

pub use comicinfo::{ComicInfo, InfoPage};
pub use names::{chapter_name, chapter_number, clean_name, mangapoint_chapter, mangapoint_title, natural_cmp, plain, series_title};

/// The pictures a page can be (the importer's list).
pub const PAGE_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "gif", "tif", "tiff"];
/// The largest cover picture that is considered (the importer's limit).
const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;
/// The most entries one list may have.
pub const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 8;

/// One file of a book: its path inside the archive or folder, and its size.
#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub path: String,
    #[serde(default = "one")]
    pub size: f64,
}

fn one() -> f64 {
    1.0
}

/// What kind of thing the entries come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A CBZ, ZIP or an EPUB's pictures.
    #[default]
    Archive,
    /// A chosen folder; paths are relative to it.
    Folder,
    /// Pictures picked one by one from a folder.
    Images,
}

/// What to guess chapters for.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Input {
    /// The file's or folder's name.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: Kind,
    /// The text of the book's `ComicInfo.xml`.
    #[serde(default)]
    pub comicinfo: Option<String>,
    pub entries: Vec<Entry>,
}

/// One chapter of the guess.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chapter {
    pub name: String,
    pub number: Option<f64>,
    /// Position in [`Guess::pages`] of the chapter's first page.
    pub first: usize,
    pub count: usize,
}

/// What the book says about itself (from its `ComicInfo.xml`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub series: String,
    pub author: String,
    pub artist: String,
    pub summary: String,
    pub genres: Vec<String>,
    pub language: String,
    pub right_to_left: Option<bool>,
}

/// The answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Guess {
    pub title: String,
    pub chapters: Vec<Chapter>,
    /// Entry indices of the pages, in reading order.
    pub pages: Vec<usize>,
    pub cover: Option<usize>,
    /// The entry to read as `ComicInfo.xml` (and pass back as `comicinfo`).
    pub comicinfo: Option<usize>,
    pub warnings: Vec<String>,
    pub meta: Meta,
}

fn fail(message: impl std::fmt::Display) -> String {
    message.to_string()
}

/// Is this name a page picture? (Not an archiver's litter such as `__MACOSX`.)
pub fn is_page_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    if base.starts_with("._") || base == ".ds_store" || base == "thumbs.db" {
        return false;
    }
    if lower.split(['/', '\\']).any(|part| part == "__macosx") {
        return false;
    }
    let ext = base.rsplit('.').next().unwrap_or("");
    base.contains('.') && PAGE_EXTENSIONS.contains(&ext)
}

/// A name with `/` separators and `.` and `..` resolved.
fn norm(name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn base(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn dir_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Is this file named as a cover is ("cover.jpg", "Folder.png")?
fn named_cover(name: &str) -> bool {
    let stem = base(name)
        .rsplit_once('.')
        .map_or(base(name), |(s, _)| s)
        .to_lowercase();
    matches!(stem.as_str(), "cover" | "folder" | "front" | "frontcover")
}

fn size_of(e: &Entry) -> u64 {
    if e.size.is_finite() && e.size > 0.0 {
        e.size as u64
    } else {
        0
    }
}

/// Which entry is the book's `ComicInfo.xml`: the importer's choice.
pub fn comicinfo_entry(entries: &[Entry]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let lower = e.path.to_lowercase();
            lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml")
        })
        .filter(|(_, e)| size_of(e) <= MAX_COVER_BYTES)
        .min_by(|(_, a), (_, b)| natural_cmp(&a.path, &b.path))
        .map(|(i, _)| i)
}

/// Guess the chapters of a book. Errors are sentences for the visitor.
pub fn guess(input: &Input) -> Result<Guess, String> {
    if input.entries.len() > MAX_ENTRIES {
        return Err(fail("This book holds too many files."));
    }
    let comic = input
        .comicinfo
        .as_deref()
        .and_then(|xml| comicinfo::parse(xml.as_bytes()));
    let mut guess = match input.kind {
        Kind::Archive => archive(&input.label, &input.entries, comic)?,
        Kind::Folder => folder(&input.label, &input.entries)?,
        Kind::Images => images(&input.label, &input.entries)?,
    };
    if input.kind == Kind::Archive {
        guess.comicinfo = comicinfo_entry(&input.entries);
    }
    Ok(guess)
}

/// `guess_chapters_json`: the whole thing as JSON text in and out.
pub fn guess_chapters_json(input_json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(input_json).map_err(|_| fail("The list of files is not valid JSON."))?;
    let input: Input = if value.is_array() {
        Input {
            entries: serde_json::from_value(value).map_err(|_| fail("The list of files is not valid."))?,
            ..Input::default()
        }
    } else {
        serde_json::from_value(value).map_err(|_| fail("The list of files is not valid."))?
    };
    let guess = guess(&input)?;
    serde_json::to_string(&guess).map_err(fail)
}

/// A CBZ, ZIP or EPUB. This is `build_book` of the importer.
fn archive(label: &str, entries: &[Entry], comic: Option<ComicInfo>) -> Result<Guess, String> {
    let mut pages: Vec<(usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| is_page_name(&e.path) && size_of(e) > 0)
        .collect();
    // Two files of one name cannot be told apart by name: the first one is the
    // page, and the person is told.
    let mut seen: HashSet<&str> = HashSet::new();
    let before = pages.len();
    pages.retain(|(_, e)| seen.insert(e.path.as_str()));
    let duplicates = before - pages.len();
    pages.sort_by(|a, b| natural_cmp(&norm(&a.1.path), &norm(&b.1.path)));
    if pages.is_empty() {
        return Err(fail(format!(
            "{label} has no page pictures in it (JPG, PNG, WEBP, GIF or TIFF)."
        )));
    }
    let mut out = Guess::default();
    if duplicates > 0 {
        out.warnings.push(format!(
            "{duplicates} page(s) have the same name as an earlier one in the archive, and are left out."
        ));
    }
    let info = comic.unwrap_or_default();

    // A cover that sits alone in the top folder, beside folders of chapters,
    // is the cover and not a page.
    let has_bookmarks = info.pages.iter().any(|p| p.bookmark.is_some());
    let in_folders = pages.iter().any(|(_, e)| !dir_of(&norm(&e.path)).is_empty());
    let mut apart: Option<(usize, &Entry)> = None;
    if !has_bookmarks && in_folders {
        let covers: Vec<(usize, &Entry)> = pages
            .iter()
            .copied()
            .filter(|(_, e)| dir_of(&norm(&e.path)).is_empty() && named_cover(&e.path))
            .collect();
        if covers.len() <= 2 && covers.len() < pages.len() {
            apart = covers.first().copied();
            pages.retain(|(_, e)| !(dir_of(&norm(&e.path)).is_empty() && named_cover(&e.path)));
        }
    }

    // Where the chapters begin, as (first page, name) in page order.
    let page_dirs: Vec<String> = pages
        .iter()
        .map(|(_, e)| dir_of(&norm(&e.path)).to_string())
        .collect();
    let mut starts: Vec<(usize, String)> = Vec::new();
    let bookmarks: Vec<(usize, String)> = info
        .pages
        .iter()
        .filter(|p| p.image < pages.len())
        .filter_map(|p| p.bookmark.clone().map(|b| (p.image, b)))
        .collect();
    if !bookmarks.is_empty() {
        starts = bookmarks;
        starts.sort_by_key(|(i, _)| *i);
        starts.dedup_by_key(|(i, _)| *i);
        if starts[0].0 > 0 {
            starts.insert(0, (0, String::new()));
        }
    } else {
        let mut dirs: Vec<&str> = Vec::new();
        for d in &page_dirs {
            if !dirs.contains(&d.as_str()) {
                dirs.push(d);
            }
        }
        if dirs.len() > 1 {
            let mut last: Option<&str> = None;
            for (i, d) in page_dirs.iter().enumerate() {
                if last != Some(d.as_str()) {
                    starts.push((i, d.rsplit('/').next().unwrap_or(d).to_string()));
                    last = Some(d.as_str());
                }
            }
        }
    }

    out.pages = pages.iter().map(|(i, _)| *i).collect();
    if starts.len() <= 1 {
        let number = info
            .number
            .or_else(|| chapter_number(label))
            .or_else(|| starts.first().and_then(|(_, n)| chapter_number(n)));
        let name = if !info.title.is_empty() && info.title != info.series {
            info.title.clone()
        } else {
            match (number, starts.first()) {
                (_, Some((_, n))) if !n.is_empty() => chapter_name(n, number),
                (Some(n), _) => chapter_name("", Some(n)),
                _ => clean_name(label),
            }
        };
        out.chapters.push(Chapter { name, number, first: 0, count: pages.len() });
    } else {
        for (n, (from, name)) in starts.iter().enumerate() {
            let end = starts.get(n + 1).map_or(pages.len(), |(i, _)| *i);
            if end <= *from {
                continue;
            }
            let number = chapter_number(name);
            out.chapters.push(Chapter {
                name: if name.is_empty() { chapter_name("", number) } else { chapter_name(name, number) },
                number,
                first: *from,
                count: end - *from,
            });
        }
    }

    // What it says about itself.
    out.title = if !info.series.is_empty() {
        info.series.clone()
    } else if !info.title.is_empty() && out.chapters.len() > 1 {
        info.title.clone()
    } else {
        series_title(label)
    };
    out.meta = Meta {
        series: info.series.clone(),
        author: info.writer.clone(),
        artist: info.artist.clone(),
        summary: info.summary.clone(),
        genres: info.genres.clone(),
        language: info.language.clone(),
        right_to_left: info.right_to_left,
    };

    // The cover: the one the book marks, one named for it, or the first page.
    let front = info
        .pages
        .iter()
        .find(|p| p.kind.eq_ignore_ascii_case("FrontCover"))
        .and_then(|p| pages.get(p.image));
    let cover = front
        .copied()
        .or(apart)
        .or_else(|| {
            entries
                .iter()
                .enumerate()
                .find(|(_, e)| is_page_name(&e.path) && named_cover(&e.path) && size_of(e) > 0)
        })
        .unwrap_or(pages[0]);
    // A cover too big to be one is not used (a page is still a page).
    out.cover = (size_of(cover.1) <= MAX_COVER_BYTES).then_some(cover.0);
    Ok(out)
}

fn is_cover_file(name: &str) -> bool {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_lowercase();
    matches!(stem.as_str(), "cover" | "folder" | "front" | "frontcover")
}

fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

/// A chosen folder. This is `scan_dir` of the importer, for the pictures in it.
fn folder(label: &str, entries: &[Entry]) -> Result<Guess, String> {
    // (folder relative to the root with `/`, [(picture file name, entry index)])
    let mut tree: Vec<(String, Vec<(String, usize)>)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let path = norm(&e.path);
        let (dir, file) = match path.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), path.clone()),
        };
        // The importer skips anything hidden and does not go deeper than MAX_DEPTH.
        if path.split('/').any(|part| part.starts_with('.')) {
            continue;
        }
        if !dir.is_empty() && dir.split('/').count() > MAX_DEPTH {
            continue;
        }
        // (`.img` is a page as MangaPoint keeps it in its own library folder.)
        let ext = extension(&file);
        if size_of(e) == 0 || !(PAGE_EXTENSIONS.contains(&ext.as_str()) || ext == "img") {
            continue;
        }
        match tree.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, files)) => files.push((file, i)),
            None => tree.push((dir, vec![(file, i)])),
        }
    }
    for (_, files) in &mut tree {
        files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    }
    tree.sort_by(|a, b| natural_cmp(&a.0, &b.0));

    let mut out = Guess {
        title: clean_name(label),
        ..Guess::default()
    };
    // A series folder of MangaPoint's own library: `c12/00001.img` and a `cover.img`.
    let own_library = tree.iter().any(|(rel, _)| !rel.is_empty())
        && tree
            .iter()
            .all(|(rel, _)| rel.is_empty() || (!rel.contains('/') && names::mangapoint_chapter(rel).is_some()));
    if own_library {
        if let Some(title) = names::mangapoint_title(label) {
            out.title = title;
        }
    }
    let only_root = tree.len() == 1 && tree[0].0.is_empty();
    let mut cover: Option<usize> = None;
    let apart_ok = tree.len() > 1;
    for (rel, files) in &tree {
        let in_root = rel.is_empty();
        let apart = in_root && apart_ok;
        let mut pages = Vec::new();
        for (name, index) in files {
            if apart && is_cover_file(name) {
                cover = cover.or(Some(*index));
                continue;
            }
            pages.push(*index);
        }
        if pages.is_empty() {
            continue;
        }
        let last = rel.rsplit('/').next().unwrap_or(rel);
        let own = if own_library && !in_root { names::mangapoint_chapter(last) } else { None };
        let number = match &own {
            Some((n, _)) => *n,
            None => chapter_number(last),
        };
        let name = if let Some((_, name)) = own {
            name
        } else if only_root {
            chapter_name(label, chapter_number(label))
        } else if in_root {
            "Loose pages".to_string()
        } else {
            chapter_name(last, number)
        };
        let number = if only_root {
            chapter_number(label)
        } else if in_root {
            None
        } else {
            number
        };
        out.chapters.push(Chapter { name, number, first: out.pages.len(), count: pages.len() });
        out.pages.extend(pages);
    }
    if out.chapters.is_empty() {
        return Err(fail(format!("{label} has no page pictures or books in it.")));
    }
    out.cover = cover.or_else(|| out.pages.first().copied());
    Ok(out)
}

/// Pictures picked one by one: one chapter, named for the folder they are in.
fn images(label: &str, entries: &[Entry]) -> Result<Guess, String> {
    let label = if label.is_empty() { "Pictures" } else { label };
    let mut files: Vec<(&str, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| size_of(e) > 0 && PAGE_EXTENSIONS.contains(&extension(&e.path).as_str()))
        .map(|(i, e)| (base(e.path.as_str()), i))
        .collect();
    files.sort_by(|a, b| natural_cmp(a.0, b.0));
    if files.is_empty() {
        return Err(fail("none of the pictures could be read."));
    }
    let number = chapter_number(label);
    let cover = files.iter().find(|(n, _)| is_cover_file(n)).unwrap_or(&files[0]).1;
    Ok(Guess {
        title: clean_name(label),
        chapters: vec![Chapter {
            name: chapter_name(label, number),
            number,
            first: 0,
            count: files.len(),
        }],
        pages: files.iter().map(|(_, i)| *i).collect(),
        cover: Some(cover),
        ..Guess::default()
    })
}

/// `parse_comicinfo_json`: the details a `ComicInfo.xml` holds, as JSON
/// `{ series, title, number, summary, writer, artist, genres, language,
/// rightToLeft, pages: [{ image, type, bookmark }] }`.
pub fn parse_comicinfo_json(xml: &str) -> Result<String, String> {
    let info = comicinfo::parse(xml.as_bytes())
        .ok_or_else(|| fail("This is not a ComicInfo.xml file."))?;
    let pages: Vec<serde_json::Value> = info
        .pages
        .iter()
        .map(|p| serde_json::json!({ "image": p.image, "type": p.kind, "bookmark": p.bookmark }))
        .collect();
    let value = serde_json::json!({
        "series": info.series,
        "title": info.title,
        "number": info.number,
        "summary": info.summary,
        "writer": info.writer,
        "artist": info.artist,
        "genres": info.genres,
        "language": info.language,
        "rightToLeft": info.right_to_left,
        "pages": pages,
    });
    serde_json::to_string(&value).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(paths: &[&str]) -> Vec<Entry> {
        paths.iter().map(|p| Entry { path: (*p).into(), size: 100.0 }).collect()
    }

    fn run(label: &str, kind: Kind, paths: &[&str]) -> Guess {
        guess(&Input { label: label.into(), kind, comicinfo: None, entries: entries(paths) }).unwrap()
    }

    #[test]
    fn pages_are_sorted_the_way_a_person_would() {
        let g = run("Book.cbz", Kind::Archive, &["10.jpg", "2.jpg", "1.jpg", "notes.txt", "__MACOSX/1.jpg"]);
        assert_eq!(g.pages, vec![2, 1, 0]);
        assert_eq!(g.chapters.len(), 1);
        assert_eq!((g.chapters[0].first, g.chapters[0].count), (0, 3));
    }

    #[test]
    fn folders_of_chapters_become_chapters_and_a_loose_cover_is_the_cover() {
        let g = run(
            "One Piece v01.cbz",
            Kind::Archive,
            &["cover.jpg", "Chapter 10/01.jpg", "Chapter 10/02.jpg", "Chapter 2/01.jpg", "Chapter 2/02.jpg", "Chapter 2/03.jpg"],
        );
        assert_eq!(g.cover, Some(0));
        let names: Vec<&str> = g.chapters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Chapter 2", "Chapter 10"]);
        assert_eq!(g.chapters[0].number, Some(2.0));
        assert_eq!((g.chapters[1].first, g.chapters[1].count), (3, 2));
        assert_eq!(g.pages.len(), 5);
        assert_eq!(g.title, "One Piece");
    }

    #[test]
    fn the_input_can_be_a_bare_list_and_errors_are_sentences() {
        let out = guess_chapters_json(r#"[{"path":"a/1.png","size":5},{"path":"b/1.png","size":5}]"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["chapters"].as_array().unwrap().len(), 2);
        assert_eq!(v["chapters"][0]["first"], 0);
        assert!(v["chapters"][0]["number"].is_null());
        assert!(guess_chapters_json("[]").unwrap_err().contains("no page pictures"));
        assert!(guess_chapters_json("nope").unwrap_err().contains("JSON"));
        assert!(guess_chapters_json(r#"{"entries": 5}"#).is_err());
        // Empty names and strange sizes do not panic.
        let _ = guess_chapters_json(r#"[{"path":""},{"path":"x.jpg","size":-5},{"path":"y.jpg","size":1e300}]"#);
    }

    #[test]
    fn a_comicinfo_with_bookmarks_gives_chapters_and_details() {
        let xml = r#"<ComicInfo><Series>My Series</Series><Writer>A. Writer</Writer><Manga>YesAndRightToLeft</Manga>
          <Pages><Page Image="0" Type="FrontCover" Bookmark="Prologue"/><Page Image="2" Bookmark="Chapter 1"/></Pages></ComicInfo>"#;
        let g = guess(&Input {
            label: "x.cbz".into(),
            kind: Kind::Archive,
            comicinfo: Some(xml.into()),
            entries: entries(&["001.jpg", "002.jpg", "003.jpg", "004.jpg", "ComicInfo.xml"]),
        })
        .unwrap();
        assert_eq!(g.title, "My Series");
        assert_eq!(g.chapters.len(), 2);
        assert_eq!((g.chapters[1].name.as_str(), g.chapters[1].number, g.chapters[1].first), ("Chapter 1", Some(1.0), 2));
        assert_eq!(g.meta.author, "A. Writer");
        assert_eq!(g.meta.right_to_left, Some(true));
        assert_eq!(g.comicinfo, Some(4));
        let json = parse_comicinfo_json(xml).unwrap();
        assert!(json.contains("\"rightToLeft\":true") && json.contains("Prologue"));
        assert!(parse_comicinfo_json("<html/>").is_err());
    }

    #[test]
    fn a_folder_is_a_series_of_chapters_and_loose_pictures_are_one_chapter() {
        let g = run("My Manga", Kind::Folder, &["cover.png", "Ch 2/1.jpg", "Ch 2/2.jpg", "Ch 1/1.jpg", ".hidden/1.jpg", "Ch 1/readme.txt"]);
        assert_eq!(g.chapters.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Ch 1", "Ch 2"]);
        assert_eq!(g.cover, Some(0));
        let only = run("Chapter 7", Kind::Folder, &["02.jpg", "01.jpg"]);
        assert_eq!((only.chapters[0].name.as_str(), only.chapters[0].number), ("Chapter 7", Some(7.0)));
        assert_eq!(only.pages, vec![1, 0]);
        let loose = run("Holiday", Kind::Images, &["/a/b/2.png", "/a/b/10.png", "/a/b/cover.png"]);
        assert_eq!(loose.title, "Holiday");
        assert_eq!(loose.cover, Some(2));
    }
}
