//! The chapter guesser gives the desktop importer's answers.
//!
//! Each case builds a real ZIP, folder or set of loose pictures on disk, lets the
//! importer (`pipeline::import::scan`) read it, runs the same names through
//! `web_engine::chapters`, and compares titles, chapter names and numbers, page
//! order and the cover.
#![cfg(feature = "convert")]

use pipeline::import::{scan, ImportBook};
use pipeline::CancelFlag;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use web_engine::chapters::{guess, Entry, Guess, Input, Kind};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("web-engine-chapters-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The content of a file is its own name, so the cover can be told by its bytes.
fn body(name: &str) -> Vec<u8> {
    format!("picture:{name}").into_bytes()
}

struct Item {
    path: String,
    /// Written empty (size 0).
    empty: bool,
}

fn items(paths: &[&str]) -> Vec<Item> {
    paths.iter().map(|p| Item { path: (*p).into(), empty: false }).collect()
}

fn entries(items: &[Item]) -> Vec<Entry> {
    items
        .iter()
        .map(|i| Entry { path: i.path.clone(), size: if i.empty { 0.0 } else { body(&i.path).len() as f64 } })
        .collect()
}

/// What the importer made of it, reduced to what is compared.
#[derive(Debug, PartialEq)]
struct Summary {
    title: String,
    chapters: Vec<(String, Option<f64>, Vec<String>)>,
    cover: Option<String>,
}

fn summarize(book: &ImportBook) -> Summary {
    Summary {
        title: book.meta.title.clone(),
        chapters: book
            .chapters
            .iter()
            .map(|c| (c.name.clone(), c.number, c.pages.iter().map(|p| p.key.clone()).collect()))
            .collect(),
        cover: book.cover.as_ref().map(|b| String::from_utf8(b.clone()).unwrap().trim_start_matches("picture:").to_string()),
    }
}

fn mine(g: &Guess, entries: &[Entry]) -> Summary {
    Summary {
        title: g.title.clone(),
        chapters: g
            .chapters
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.number,
                    g.pages[c.first..c.first + c.count].iter().map(|&i| entries[i].path.clone()).collect(),
                )
            })
            .collect(),
        cover: g.cover.map(|i| entries[i].path.clone()),
    }
}

fn importer_scan(paths: &[PathBuf]) -> (Vec<ImportBook>, Vec<String>) {
    let result = scan(paths, &CancelFlag::new());
    (result.books, result.failures.into_iter().map(|f| f.message).collect())
}

fn write_zip(path: &Path, items: &[Item], comicinfo: Option<&str>) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for item in items {
        zip.start_file(item.path.as_str(), options).unwrap();
        if !item.empty {
            zip.write_all(&body(&item.path)).unwrap();
        }
    }
    if let Some(xml) = comicinfo {
        zip.start_file("ComicInfo.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

/// An archive, with or without a ComicInfo.xml.
fn check_archive(tag: &str, label: &str, items: &[Item], comicinfo: Option<&str>) -> Option<Summary> {
    let dir = scratch(tag);
    let path = dir.join(label);
    write_zip(&path, items, comicinfo);
    let (books, failures) = importer_scan(&[path]);
    let mut list = entries(items);
    if let Some(xml) = comicinfo {
        list.push(Entry { path: "ComicInfo.xml".into(), size: xml.len() as f64 });
    }
    let input = Input { label: label.into(), kind: Kind::Archive, comicinfo: comicinfo.map(String::from), entries: list.clone() };
    let summary = match (books.first(), guess(&input)) {
        (Some(book), Ok(g)) => {
            assert_eq!(mine(&g, &list), summarize(book), "{tag}");
            Some(summarize(book))
        }
        (None, Err(e)) => {
            assert_eq!(&e, failures.first().expect("the importer says why"), "{tag}");
            None
        }
        (book, mine) => panic!("{tag}: the importer gave {:?}, this gave {:?}", book.map(summarize), mine.map(|g| g.title)),
    };
    let _ = fs::remove_dir_all(dir);
    summary
}

fn check_folder(tag: &str, label: &str, items: &[Item]) {
    let dir = scratch(tag);
    let root = dir.join(label);
    fs::create_dir_all(&root).unwrap();
    for item in items {
        let path = root.join(&item.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, if item.empty { Vec::new() } else { body(&item.path) }).unwrap();
    }
    let (books, failures) = importer_scan(&[root]);
    let list = entries(items);
    let input = Input { label: label.into(), kind: Kind::Folder, comicinfo: None, entries: list.clone() };
    match (books.first(), guess(&input)) {
        (Some(book), Ok(g)) => assert_eq!(mine(&g, &list), summarize(book), "{tag}"),
        (None, Err(e)) => assert_eq!(&e, failures.first().expect("the importer says why"), "{tag}"),
        (book, mine) => panic!("{tag}: the importer gave {:?}, this gave {:?}", book.map(summarize), mine.map(|g| g.title)),
    }
    let _ = fs::remove_dir_all(dir);
}

fn check_images(tag: &str, label: &str, names: &[&str]) {
    let dir = scratch(tag);
    let root = dir.join(label);
    fs::create_dir_all(&root).unwrap();
    let mut paths = Vec::new();
    for n in names {
        let p = root.join(n);
        fs::write(&p, body(n)).unwrap();
        paths.push(p);
    }
    let (books, failures) = importer_scan(&paths);
    let list: Vec<Entry> = names.iter().map(|n| Entry { path: (*n).into(), size: body(n).len() as f64 }).collect();
    let input = Input { label: label.into(), kind: Kind::Images, comicinfo: None, entries: list.clone() };
    match (books.first(), guess(&input)) {
        (Some(book), Ok(g)) => assert_eq!(mine(&g, &list), summarize(book), "{tag}"),
        (None, Err(e)) => assert_eq!(&e, failures.first().expect("the importer says why"), "{tag}"),
        (book, mine) => panic!("{tag}: the importer gave {:?}, this gave {:?}", book.map(summarize), mine.map(|g| g.title)),
    }
    let _ = fs::remove_dir_all(dir);
}

// ---- Archives ------------------------------------------------------------------

#[test]
fn a_flat_archive_is_one_chapter_in_natural_order_whatever_its_name() {
    for (i, label) in [
        "Vol 2 Ch 10.cbz",
        "One Piece - c1050 [Group] (2020).cbz",
        "Berserk v03.cbz",
        "Berserk 2019.cbz",
        "Naruto #7.cbz",
        "Chapter 10.5.cbz",
        "012.cbz",
        "12.5.zip",
        "Ep. 3 - The Beginning.cbz",
        "My Hero Academia 400.cbz",
        "Solo Leveling Chapter 179 (Final).cbz",
        "[Group] Title_With_Underscores_ch.3.cbz",
        "noextension",
        "進撃の巨人 第3巻.cbz",
        "第12話.cbz",
        "Ünïcödé Séries 7.cbz",
        ".cbz",
    ]
    .iter()
    .enumerate()
    {
        check_archive(
            &format!("flat{i}"),
            label,
            &items(&["10.jpg", "2.jpg", "1.jpg", "02.jpg", "A.JPG", "notes.txt", "__MACOSX/1.jpg", "Thumbs.db", ".DS_Store", "._1.jpg", "z.webp", "q.gif", "t.tiff"]),
            None,
        );
    }
}

#[test]
fn folders_inside_an_archive_are_chapters_with_extras_and_a_cover() {
    let chapter_folders = items(&[
        "cover.jpg",
        "Chapter 10/01.jpg",
        "Chapter 10/02.jpg",
        "Chapter 10.5/01.jpg",
        "Chapter 2/01.jpg",
        "Chapter 2/02.jpg",
        "Chapter 2/03.jpg",
        "Extra/01.jpg",
        "Omake/1.png",
        "Oneshot/1.png",
    ]);
    let book = check_archive("folders", "One Piece v01.cbz", &chapter_folders, None).expect("a book");
    let names: Vec<&str> = book.chapters.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(names, ["Chapter 2", "Chapter 10.5", "Chapter 10", "Extra", "Omake", "Oneshot"]);
    assert_eq!((book.title.as_str(), book.cover.as_deref()), ("One Piece", Some("cover.jpg")));
    check_archive("folders-no-label", "x.cbz", &chapter_folders, None);
    check_archive(
        "numeric-folders",
        "Series.cbz",
        &items(&["010/1.jpg", "001/1.jpg", "002/2.jpg", "002/1.jpg", "Folder.png"]),
        None,
    );
    check_archive(
        "nested",
        "Series.cbz",
        &items(&["Series/Vol 1/Ch 1/001.jpg", "Series/Vol 1/Ch 2/001.jpg", "Series/Vol 2/Ch 10/001.jpg", "__MACOSX/Series/._001.jpg"]),
        None,
    );
    check_archive(
        "non-ascii",
        "進撃の巨人.cbz",
        &items(&["第2話/01.jpg", "第12話/01.jpg", "第12話/02.jpg", "第1話/01.jpg", "Ünï/1.jpg"]),
        None,
    );
    check_archive(
        "two-covers",
        "A.cbz",
        &items(&["cover.jpg", "front.png", "ch1/1.jpg", "ch2/1.jpg"]),
        None,
    );
    check_archive("cover-only", "A.cbz", &items(&["cover.jpg", "1.jpg"]), None);
    check_archive("same-names-in-folders", "A.cbz", &items(&["a/1.jpg", "b/1.jpg", "c/1.jpg"]), None);
    check_archive("a-cover-named-page", "A.cbz", &items(&["01.jpg", "02.jpg", "Cover.JPG"]), None);
}

#[test]
fn empty_and_pictureless_archives_fail_with_the_importers_sentence() {
    assert!(check_archive("no-pictures", "Notes.cbz", &items(&["readme.txt", "ComicInfo2.xml"]), None).is_none());
    let mut with_empty = items(&["1.jpg", "2.jpg"]);
    with_empty[0].empty = true;
    check_archive("one-empty-page", "A.cbz", &with_empty, None);
    let mut all_empty = items(&["1.jpg"]);
    all_empty[0].empty = true;
    check_archive("only-empty-pages", "A.cbz", &all_empty, None);
}

#[test]
fn comicinfo_gives_bookmarks_details_and_the_cover() {
    let pages = items(&["001.jpg", "002.jpg", "003.jpg", "004.jpg", "005.jpg"]);
    let with_bookmarks = r#"<?xml version="1.0"?><ComicInfo><Series>My Series</Series><Title>Vol 1</Title><Writer>A</Writer>
        <Manga>YesAndRightToLeft</Manga><Pages>
        <Page Image="0" Type="FrontCover"/><Page Image="1" Bookmark="Prologue"/><Page Image="3" Bookmark="Chapter 7.5"/></Pages></ComicInfo>"#;
    check_archive("bookmarks", "x.cbz", &pages, Some(with_bookmarks));
    let single = r#"<ComicInfo><Series>Solo</Series><Title>The Hunt</Title><Number>12</Number></ComicInfo>"#;
    check_archive("single-chapter", "x.cbz", &pages, Some(single));
    let same_title = r#"<ComicInfo><Series>Solo</Series><Title>Solo</Title></ComicInfo>"#;
    check_archive("same-title", "Solo 3.cbz", &pages, Some(same_title));
    let front = r#"<ComicInfo><Pages><Page Image="2" Type="FrontCover"/></Pages></ComicInfo>"#;
    check_archive("front-cover", "x.cbz", &pages, Some(front));
    check_archive("not-xml", "x.cbz", &pages, Some("this is not xml"));
    let in_folders = items(&["a/1.jpg", "a/2.jpg", "b/1.jpg", "cover.jpg"]);
    let marks = r#"<ComicInfo><Pages><Page Image="0" Bookmark="One"/><Page Image="2" Bookmark="Two"/></Pages></ComicInfo>"#;
    check_archive("bookmarks-with-folders", "x.cbz", &in_folders, Some(marks));
    let out_of_range = r#"<ComicInfo><Pages><Page Image="99" Bookmark="Late"/></Pages></ComicInfo>"#;
    check_archive("bookmark-out-of-range", "x.cbz", &pages, Some(out_of_range));
}

// ---- Folders and loose pictures ------------------------------------------------------

#[test]
fn a_folder_is_a_book_of_chapters_or_one_chapter() {
    check_folder("folder-flat", "Chapter 7", &items(&["02.jpg", "01.jpg", "10.jpg"]));
    check_folder("folder-flat-plain", "Holiday Photos", &items(&["b.png", "a.png"]));
    check_folder(
        "folder-chapters",
        "Berserk",
        &items(&["Ch 2/1.jpg", "Ch 2/2.jpg", "Ch 10/1.jpg", "Ch 1/1.jpg", "Ch 1/notes.txt", "cover.jpg"]),
    );
    check_folder(
        "folder-mixed",
        "Series",
        &items(&["intro.jpg", "Chapter 3/1.jpg", "Chapter 3/2.jpg", "Chapter 4/1.jpg"]),
    );
    check_folder(
        "folder-hidden-and-deep",
        "Series",
        &items(&[".hidden/1.jpg", "a/b/c/d/e/f/g/h/i/1.jpg", "a/b/c/d/e/f/g/h/1.jpg", "ok/.secret.jpg", "ok/1.jpg", "ok/2.jpg"]),
    );
    check_folder("folder-non-ascii", "月刊 Ünï 4", &items(&["第1話/1.jpg", "第10話/1.jpg", "第2話/1.jpg"]));
    check_folder("folder-nothing", "Empty", &items(&["readme.txt"]));
    check_folder("folder-only-cover", "Pics", &items(&["cover.jpg", "1.jpg"]));
}

#[test]
fn loose_pictures_are_one_chapter_named_for_their_folder() {
    check_images("loose", "Vol 3 Chapter 21", &["10.png", "2.png", "1.png", "cover.png"]);
    check_images("loose-plain", "Trip", &["b.jpg", "a.jpg", "A2.jpg"]);
    check_images("loose-one", "Single", &["only.webp"]);
}
