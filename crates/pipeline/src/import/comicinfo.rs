//! `ComicInfo.xml`, the details comic archives carry (the same file the CBZ
//! export writes), and what to make of it.

/// One `<Page>` of the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InfoPage {
    pub image: usize,
    pub kind: String,
    /// A chapter starts here, with this name.
    pub bookmark: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComicInfo {
    pub series: String,
    pub title: String,
    pub number: Option<f64>,
    pub summary: String,
    pub writer: String,
    pub artist: String,
    pub genres: Vec<String>,
    pub language: String,
    /// `Some(true)` for `YesAndRightToLeft`, `Some(false)` for `No`.
    pub right_to_left: Option<bool>,
    pub status: String,
    pub pages: Vec<InfoPage>,
}

/// Read a `ComicInfo.xml`. `None` if it is not XML at all; fields it lacks
/// are left empty.
pub fn parse(bytes: &[u8]) -> Option<ComicInfo> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("ComicInfo") {
        return None;
    }
    let field = |name: &str| -> String {
        root.children()
            .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case(name))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let first_of = |names: &[&str]| names.iter().map(|n| field(n)).find(|v| !v.is_empty()).unwrap_or_default();
    let genres = field("Genre")
        .split([',', ';'])
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();
    let right_to_left = match field("Manga").to_lowercase().as_str() {
        "yesandrighttoleft" => Some(true),
        "yes" => None,
        "no" => Some(false),
        _ => None,
    };
    let pages = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("Pages"))
        .map(|pages| {
            pages
                .children()
                .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("Page"))
                .filter_map(|p| {
                    let attr = |name: &str| {
                        p.attributes()
                            .find(|a| a.name().eq_ignore_ascii_case(name))
                            .map(|a| a.value().to_string())
                    };
                    Some(InfoPage {
                        image: attr("Image")?.trim().parse().ok()?,
                        kind: attr("Type").unwrap_or_default(),
                        bookmark: attr("Bookmark").filter(|b| !b.trim().is_empty()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ComicInfo {
        series: field("Series"),
        title: field("Title"),
        number: field("Number").parse().ok(),
        summary: field("Summary"),
        writer: first_of(&["Writer"]),
        artist: first_of(&["Penciller", "Artist", "Inker", "CoverArtist"]),
        genres,
        language: first_of(&["LanguageISO"]),
        right_to_left,
        status: String::new(),
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_details_and_the_chapter_bookmarks_are_read() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Series>Solo Leveling</Series>
  <Title>Volume 1</Title>
  <Number>3.5</Number>
  <Summary>A weak hunter &amp; a quest log.</Summary>
  <Writer>Chugong</Writer>
  <Penciller>DUBU</Penciller>
  <Genre>Action, Fantasy; Manhwa</Genre>
  <LanguageISO>en</LanguageISO>
  <Manga>YesAndRightToLeft</Manga>
  <Pages>
    <Page Image="0" Type="FrontCover" Bookmark="Chapter 1"/>
    <Page Image="1"/>
    <Page Image="2" Bookmark="Chapter 2"/>
  </Pages>
</ComicInfo>"#;
        let info = parse(xml.as_bytes()).unwrap();
        assert_eq!((info.series.as_str(), info.number), ("Solo Leveling", Some(3.5)));
        assert_eq!(info.summary, "A weak hunter & a quest log.");
        assert_eq!((info.writer.as_str(), info.artist.as_str()), ("Chugong", "DUBU"));
        assert_eq!(info.genres, ["Action", "Fantasy", "Manhwa"]);
        assert_eq!(info.right_to_left, Some(true));
        assert_eq!(info.pages.len(), 3);
        assert_eq!(info.pages[0].kind, "FrontCover");
        assert_eq!(info.pages[2].bookmark.as_deref(), Some("Chapter 2"));
        assert_eq!(info.pages[1].bookmark, None);
    }

    #[test]
    fn something_that_is_not_comic_info_is_not_taken_for_it() {
        assert!(parse(b"not xml").is_none());
        assert!(parse(b"<html><body/></html>").is_none());
        let empty = parse(b"<ComicInfo/>").unwrap();
        assert_eq!(empty, ComicInfo::default());
    }
}
