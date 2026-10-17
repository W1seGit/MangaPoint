//! Turning the list of names inside an archive (ZIP, 7Z or RAR) into a book:
//! pages in reading order, chapters from `ComicInfo.xml` or from folders, the
//! details and the cover. The reading of the archives themselves is in the
//! modules for each kind.

use super::comicinfo::ComicInfo;
use super::names::{chapter_name, chapter_number, clean_name, natural_cmp, series_title};
use super::{fail, is_page_name, BookSource, Format, ImportBook, ImportChapter, PageRef};
use crate::error::Result;

/// One file inside an archive.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub name: String,
    pub size: u64,
}

/// A name with `/` separators and `.` and `..` resolved, so a name cannot
/// make a folder of its own by climbing out of another.
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
    let stem = base(name).rsplit_once('.').map_or(base(name), |(s, _)| s).to_lowercase();
    matches!(stem.as_str(), "cover" | "folder" | "front" | "frontcover")
}

/// Make the book an archive holds. `read` reads one file of it by name (for
/// the cover).
pub(crate) fn build_book(
    label: &str,
    format: Format,
    source: BookSource,
    entries: &[Entry],
    comic: Option<ComicInfo>,
    read: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<ImportBook> {
    let mut pages: Vec<&Entry> = entries
        .iter()
        .filter(|e| is_page_name(&e.name) && e.size > 0)
        .collect();
    pages.sort_by(|a, b| natural_cmp(&norm(&a.name), &norm(&b.name)));
    if pages.is_empty() {
        return Err(fail(format!(
            "{label} has no page pictures in it (JPG, PNG, WEBP, GIF or TIFF)."
        )));
    }
    let mut book = ImportBook::new(label, format, source);
    let info = comic.unwrap_or_default();

    // A cover that sits alone in the top folder, beside folders of chapters,
    // is the cover and not a page.
    let has_bookmarks = info.pages.iter().any(|p| p.bookmark.is_some());
    let in_folders = pages.iter().any(|e| !dir_of(&norm(&e.name)).is_empty());
    let mut apart: Option<&Entry> = None;
    if !has_bookmarks && in_folders {
        let covers: Vec<&Entry> = pages
            .iter()
            .copied()
            .filter(|e| dir_of(&norm(&e.name)).is_empty() && named_cover(&e.name))
            .collect();
        if covers.len() <= 2 && covers.len() < pages.len() {
            apart = covers.first().copied();
            pages.retain(|e| !(dir_of(&norm(&e.name)).is_empty() && named_cover(&e.name)));
        }
    }

    // Where the chapters begin, as (first page, name) in page order.
    let page_dirs: Vec<String> = pages.iter().map(|e| dir_of(&norm(&e.name)).to_string()).collect();
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

    let page_ref = |e: &Entry| PageRef { source: 0, key: e.name.clone() };
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
        book.chapters.push(ImportChapter::new(
            number,
            name,
            pages.iter().map(|e| page_ref(e)).collect(),
        ));
    } else {
        for (n, (from, name)) in starts.iter().enumerate() {
            let end = starts.get(n + 1).map_or(pages.len(), |(i, _)| *i);
            let range = &pages[*from..end];
            if range.is_empty() {
                continue;
            }
            let number = chapter_number(name);
            book.chapters.push(ImportChapter::new(
                number,
                if name.is_empty() { chapter_name("", number) } else { chapter_name(name, number) },
                range.iter().map(|e| page_ref(e)).collect(),
            ));
        }
    }

    // What it says about itself.
    let meta = &mut book.meta;
    meta.title = if !info.series.is_empty() {
        info.series.clone()
    } else if !info.title.is_empty() && book.chapters.len() > 1 {
        info.title.clone()
    } else {
        series_title(label)
    };
    meta.description = info.summary;
    meta.author = info.writer;
    meta.artist = info.artist;
    meta.genres = info.genres;
    meta.language = info.language;
    meta.right_to_left = info.right_to_left;

    // The cover: the one the book marks, one named for it, or the first page.
    let front = info
        .pages
        .iter()
        .find(|p| p.kind.eq_ignore_ascii_case("FrontCover"))
        .and_then(|p| pages.get(p.image));
    let cover = front
        .copied()
        .or(apart)
        .or_else(|| entries.iter().find(|e| is_page_name(&e.name) && named_cover(&e.name) && e.size > 0))
        .unwrap_or(pages[0]);
    book.cover = read(&cover.name);
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<Entry> {
        names.iter().map(|n| Entry { name: n.to_string(), size: 10 }).collect()
    }

    fn build(names: &[&str], comic: Option<ComicInfo>) -> ImportBook {
        build_book(
            "Series - c12.cbz",
            Format::Cbz,
            BookSource::Zip("x".into()),
            &entries(names),
            comic,
            &mut |_| Some(vec![1]),
        )
        .unwrap()
    }

    #[test]
    fn loose_pages_are_one_chapter_named_for_the_file() {
        let book = build(&["10.jpg", "2.jpg", "1.jpg", "__MACOSX/._1.jpg", "Thumbs.db", "note.txt"], None);
        assert_eq!(book.chapters.len(), 1);
        let keys: Vec<&str> = book.chapters[0].pages.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["1.jpg", "2.jpg", "10.jpg"]);
        assert_eq!(book.chapters[0].number, Some(12.0));
        assert_eq!(book.chapters[0].name, "Chapter 12");
        assert_eq!(book.meta.title, "Series");
    }

    #[test]
    fn folders_are_chapters_in_the_order_a_person_counts() {
        let book = build(
            &[
                "Series/Chapter 10/01.png",
                "Series/Chapter 2/02.png",
                "Series/Chapter 2/01.png",
                "Series/Chapter 1/01.png",
            ],
            None,
        );
        let chapters: Vec<_> = book
            .chapters
            .iter()
            .map(|c| (c.number, c.name.as_str(), c.pages.len()))
            .collect();
        assert_eq!(
            chapters,
            [(Some(1.0), "Chapter 1", 1), (Some(2.0), "Chapter 2", 2), (Some(10.0), "Chapter 10", 1)]
        );
    }

    #[test]
    fn a_wrapper_folder_around_one_chapter_is_not_a_chapter_list() {
        let book = build(&["Wrapper/1.jpg", "Wrapper/2.jpg"], None);
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].pages.len(), 2);
    }

    #[test]
    fn a_cover_beside_the_chapter_folders_is_the_cover_not_a_page() {
        let book = build(&["cover.jpg", "Ch 1/1.jpg", "Ch 2/1.jpg"], None);
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.page_count(), 2);
        assert!(book.cover.is_some());
    }

    #[test]
    fn comic_info_bookmarks_make_the_chapters_and_name_the_book() {
        let info = super::super::comicinfo::parse(
            br#"<ComicInfo><Series>Solo Leveling</Series><Writer>Chugong</Writer>
            <Pages><Page Image="0" Type="FrontCover" Bookmark="Chapter 1"/><Page Image="1"/>
            <Page Image="2" Bookmark="Chapter 2: Awake"/><Page Image="3"/></Pages></ComicInfo>"#,
        );
        let book = build(&["00001.png", "00002.png", "00003.png", "00004.jpg"], info);
        assert_eq!(book.meta.title, "Solo Leveling");
        assert_eq!(book.meta.author, "Chugong");
        let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.name.clone(), c.pages.len())).collect();
        assert_eq!(
            chapters,
            [(Some(1.0), "Chapter 1".to_string(), 2), (Some(2.0), "Chapter 2: Awake".to_string(), 2)]
        );
    }

    #[test]
    fn an_archive_with_no_pictures_is_refused_with_a_reason() {
        let result = build_book(
            "Notes.zip",
            Format::Zip,
            BookSource::Zip("x".into()),
            &entries(&["a.txt", "b.doc"]),
            None,
            &mut |_| None,
        );
        assert!(result.unwrap_err().to_string().contains("no page pictures"));
    }
}
