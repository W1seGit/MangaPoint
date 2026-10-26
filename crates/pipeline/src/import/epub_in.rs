//! EPUB: a book whose pages are pictures (a manga or comic EPUB, such as the
//! ones MangaPoint writes). The pictures are taken in reading order, and the
//! table of contents becomes the chapters. A book of text has none, and is
//! refused.

use super::archive::Entry;
use super::names::{chapter_name, chapter_number, clean_name};
use super::zip_in::{read_entry, Archive};
use super::{fail, BookSource, Format, ImportBook, ImportChapter, PageRef};
use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// `dir/rel` with `.` and `..` resolved and `%20`-style escapes undone.
fn resolve(dir: &str, rel: &str) -> String {
    let rel = rel.split(['#', '?']).next().unwrap_or(rel);
    let rel = percent_decode(rel);
    let mut parts: Vec<&str> = if rel.starts_with('/') || dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in rel.split('/') {
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

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The text of a node and what is under it, with tags dropped.
fn plain_text(node: roxmltree::Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A description that may hold HTML as text: tags removed, entities undone.
fn strip_html(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

struct Item {
    href: String,
    media: String,
    properties: String,
}

/// The pictures a page of the book shows, in order, as entry names.
fn pictures_of(doc_path: &str, bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let dir = dir_of(doc_path);
    let mut found = Vec::new();
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    if let Ok(doc) = roxmltree::Document::parse_with_options(&text, options) {
        for node in doc.descendants().filter(|n| n.is_element()) {
            let name = node.tag_name().name();
            if name.eq_ignore_ascii_case("img") || name.eq_ignore_ascii_case("image") {
                let source = node
                    .attributes()
                    .find(|a| a.name().eq_ignore_ascii_case("src") || a.name().eq_ignore_ascii_case("href"))
                    .map(|a| a.value().to_string());
                if let Some(s) = source.filter(|s| !s.starts_with("data:")) {
                    found.push(resolve(dir, &s));
                }
            }
        }
        return found;
    }
    // Not well-formed XML: look for the tags by hand.
    // ASCII-only lowercasing keeps every byte offset the same as in `text`
    // (a full lowercase can change lengths, and slicing would then panic).
    let lower = text.to_ascii_lowercase();
    for tag in ["<img", "<image"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(tag) {
            let start = from + at;
            let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
            let chunk = &text[start..end];
            let chunk_lower = &lower[start..end];
            for attr in ["src=", "href="] {
                if let Some(a) = chunk_lower.find(attr) {
                    let rest = &chunk[a + attr.len()..];
                    if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                        if let Some(close) = rest[1..].find(quote) {
                            found.push(resolve(dir, &rest[1..1 + close]));
                            break;
                        }
                    }
                }
            }
            from = end;
        }
    }
    found
}

pub(crate) fn scan(
    path: &Path,
    archive: &mut Archive,
    entries: Vec<Entry>,
    cancel: &CancelFlag,
) -> Result<ImportBook> {
    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let by_lower: HashMap<String, String> = entries
        .iter()
        .map(|e| (e.name.to_lowercase(), e.name.clone()))
        .collect();
    let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let exists = |name: &str| -> Option<String> {
        names
            .get(name)
            .map(|n| (*n).to_string())
            .or_else(|| by_lower.get(&name.to_lowercase()).cloned())
    };
    let container = read_entry(archive, "META-INF/container.xml")?;
    let container = String::from_utf8_lossy(&container).into_owned();
    let container = roxmltree::Document::parse(&container).map_err(|e| fail(format!("{label}: {e}")))?;
    let opf_path = container
        .descendants()
        .find(|n| n.tag_name().name() == "rootfile")
        .and_then(|n| n.attribute("full-path"))
        .map(|p| resolve("", p))
        .ok_or_else(|| fail(format!("{label} does not say where its content is")))?;
    let opf_bytes = read_entry(archive, &exists(&opf_path).unwrap_or(opf_path.clone()))?;
    let opf_text = String::from_utf8_lossy(&opf_bytes).into_owned();
    let opf = roxmltree::Document::parse(&opf_text).map_err(|e| fail(format!("{label}: {e}")))?;
    let opf_dir = dir_of(&opf_path).to_string();

    // The list of files, and the order they are read in.
    let mut manifest: HashMap<String, Item> = HashMap::new();
    for item in opf.descendants().filter(|n| n.tag_name().name() == "item") {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            manifest.insert(
                id.to_string(),
                Item {
                    href: resolve(&opf_dir, href),
                    media: item.attribute("media-type").unwrap_or("").to_string(),
                    properties: item.attribute("properties").unwrap_or("").to_string(),
                },
            );
        }
    }
    let spine_node = opf.descendants().find(|n| n.tag_name().name() == "spine");
    let spine: Vec<String> = spine_node
        .map(|s| {
            s.children()
                .filter(|n| n.tag_name().name() == "itemref")
                .filter_map(|n| n.attribute("idref"))
                .filter_map(|id| manifest.get(id).map(|i| i.href.clone()))
                .collect()
        })
        .unwrap_or_default();
    let rtl = spine_node
        .and_then(|s| s.attribute("page-progression-direction"))
        .map(|d| d.eq_ignore_ascii_case("rtl"));

    // The pictures, spine item by spine item.
    let mut pages: Vec<String> = Vec::new();
    let mut first_page_of: HashMap<String, usize> = HashMap::new();
    let mut page_set: HashSet<String> = HashSet::new();
    let media_of: HashMap<&str, &str> = manifest
        .values()
        .map(|i| (i.href.as_str(), i.media.as_str()))
        .collect();
    for href in &spine {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let Some(name) = exists(href) else { continue };
        first_page_of.insert(href.clone(), pages.len());
        let media = media_of.get(href.as_str()).copied().unwrap_or("");
        let found = if media.starts_with("image/") {
            vec![name.clone()]
        } else {
            match read_entry(archive, &name) {
                Ok(bytes) => pictures_of(&name, &bytes),
                Err(_) => Vec::new(),
            }
        };
        for picture in found {
            if let Some(real) = exists(&picture) {
                if page_set.insert(real.clone()) {
                    pages.push(real);
                }
            }
        }
    }
    if pages.is_empty() {
        return Err(fail(format!(
            "{label} has no page pictures: it is a book of text, which MangaPoint cannot import."
        )));
    }

    // What it says about itself.
    let mut book = ImportBook::new(&label, Format::Epub, BookSource::Zip(path.to_path_buf()));
    let meta_text = |name: &str| -> Vec<String> {
        opf.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == name)
            .map(plain_text)
            .filter(|t| !t.is_empty())
            .collect()
    };
    let series = opf
        .descendants()
        .find(|n| n.tag_name().name() == "meta" && n.attribute("name") == Some("calibre:series"))
        .and_then(|n| n.attribute("content").map(str::to_string))
        .or_else(|| {
            opf.descendants()
                .find(|n| n.tag_name().name() == "meta" && n.attribute("property") == Some("belongs-to-collection"))
                .map(plain_text)
        })
        .filter(|s| !s.is_empty());
    book.meta.title = series
        .or_else(|| meta_text("title").into_iter().next())
        .unwrap_or_else(|| clean_name(&label));
    let creators: Vec<(String, String)> = opf
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "creator")
        .map(|n| {
            let role = n
                .attributes()
                .find(|a| a.name() == "role")
                .map(|a| a.value().to_lowercase())
                .unwrap_or_default();
            (plain_text(n), role)
        })
        .collect();
    book.meta.artist = creators
        .iter()
        .find(|(_, r)| matches!(r.as_str(), "ill" | "art" | "pcl"))
        .map(|(n, _)| n.clone())
        .unwrap_or_default();
    book.meta.author = creators
        .iter()
        .find(|(_, r)| !matches!(r.as_str(), "ill" | "art" | "pcl"))
        .map(|(n, _)| n.clone())
        .unwrap_or_default();
    book.meta.description = meta_text("description").first().map(|d| strip_html(d)).unwrap_or_default();
    book.meta.genres = meta_text("subject");
    book.meta.language = meta_text("language").into_iter().next().unwrap_or_default();
    book.meta.right_to_left = rtl;

    // The chapters: the table of contents, as the pages they start at.
    let mut starts: Vec<(usize, String)> = Vec::new();
    let toc_items: Vec<(String, String)> = {
        let mut list = Vec::new();
        // EPUB 3: the nav document.
        if let Some(nav) = manifest.values().find(|i| i.properties.split_whitespace().any(|p| p == "nav")) {
            if let Some(real) = exists(&nav.href) {
                if let Ok(bytes) = read_entry(archive, &real) {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
                    if let Ok(doc) = roxmltree::Document::parse_with_options(&text, options) {
                        let toc = doc
                            .descendants()
                            .find(|n| {
                                n.tag_name().name() == "nav"
                                    && n.attributes().any(|a| a.name() == "type" && a.value().contains("toc"))
                            })
                            .or_else(|| doc.descendants().find(|n| n.tag_name().name() == "nav"));
                        if let Some(toc) = toc {
                            for a in toc.descendants().filter(|n| n.tag_name().name() == "a") {
                                if let Some(href) = a.attribute("href") {
                                    list.push((resolve(dir_of(&real), href), plain_text(a)));
                                }
                            }
                        }
                    }
                }
            }
        }
        // EPUB 2: the NCX.
        if list.is_empty() {
            let ncx = manifest.values().find(|i| i.media == "application/x-dtbncx+xml");
            if let Some(real) = ncx.and_then(|i| exists(&i.href)) {
                if let Ok(bytes) = read_entry(archive, &real) {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    if let Ok(doc) = roxmltree::Document::parse(&text) {
                        for point in doc.descendants().filter(|n| n.tag_name().name() == "navPoint") {
                            let label = point
                                .children()
                                .find(|n| n.tag_name().name() == "navLabel")
                                .map(plain_text)
                                .unwrap_or_default();
                            let src = point
                                .children()
                                .find(|n| n.tag_name().name() == "content")
                                .and_then(|n| n.attribute("src"));
                            if let Some(src) = src {
                                list.push((resolve(dir_of(&real), src), label));
                            }
                        }
                    }
                }
            }
        }
        list
    };
    for (href, title) in toc_items {
        if let Some(&first) = first_page_of.get(&href) {
            // The first picture at or after this part of the book.
            if first < pages.len() && !title.is_empty() {
                starts.push((first, title));
            }
        }
    }
    starts.sort_by_key(|(i, _)| *i);
    starts.dedup_by_key(|(i, _)| *i);
    if starts.len() > 1 {
        if starts[0].0 > 0 {
            starts.insert(0, (0, String::new()));
        }
        for (n, (from, title)) in starts.iter().enumerate() {
            let end = starts.get(n + 1).map_or(pages.len(), |(i, _)| *i);
            let number = chapter_number(title);
            book.chapters.push(ImportChapter::new(
                number,
                if title.is_empty() { chapter_name("", number) } else { chapter_name(title, number) },
                pages[*from..end]
                    .iter()
                    .map(|k| PageRef { source: 0, key: k.clone() })
                    .collect(),
            ));
        }
    } else {
        let number = chapter_number(&label);
        book.chapters.push(ImportChapter::new(
            number,
            chapter_name(&book.meta.title, number),
            pages.iter().map(|k| PageRef { source: 0, key: k.clone() }).collect(),
        ));
    }

    // The cover: the one the book names, else the first page.
    let cover_id = opf
        .descendants()
        .find(|n| n.tag_name().name() == "meta" && n.attribute("name") == Some("cover"))
        .and_then(|n| n.attribute("content"));
    let cover_href = manifest
        .values()
        .find(|i| i.properties.split_whitespace().any(|p| p == "cover-image"))
        .map(|i| i.href.clone())
        .or_else(|| cover_id.and_then(|id| manifest.get(id)).map(|i| i.href.clone()));
    let cover_entry = cover_href
        .and_then(|h| exists(&h))
        .filter(|name| super::is_page_name(name))
        .unwrap_or_else(|| pages[0].clone());
    book.cover = read_entry(archive, &cover_entry).ok();
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_book_are_resolved_and_unescaped() {
        assert_eq!(resolve("OEBPS/text", "../images/p%201.jpg"), "OEBPS/images/p 1.jpg");
        assert_eq!(resolve("", "OEBPS/content.opf"), "OEBPS/content.opf");
        assert_eq!(resolve("a", "b.xhtml#frag"), "a/b.xhtml");
        assert_eq!(resolve("a/b", "/root.png"), "root.png");
    }

    #[test]
    fn pictures_are_found_in_html_and_svg_pages_and_in_broken_markup() {
        let html = br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="../img/1.jpg"/></body></html>"#;
        assert_eq!(pictures_of("OEBPS/text/p1.xhtml", html), ["OEBPS/img/1.jpg"]);
        let svg = br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:xlink="http://www.w3.org/1999/xlink"><body><svg><image xlink:href="p2.png"/></svg></body></html>"#;
        assert_eq!(pictures_of("p.xhtml", svg), ["p2.png"]);
        let broken = b"<html><body>&nbsp;<br><img src='x/3.webp'></body>";
        assert_eq!(pictures_of("d/p.html", broken), ["d/x/3.webp"]);
    }

    #[test]
    fn broken_markup_with_letters_that_change_length_when_lowercased_does_not_panic() {
        // 'İ' is two bytes, and three when lowercased: offsets taken from the
        // lowercase text must still fit the original.
        let html = "<html><body>İİİİİİ<br><IMG SRC='x/İ.png'>İİ<img
src=\"y.webp\"></body>".as_bytes();
        assert_eq!(pictures_of("d/p.html", html), ["d/x/İ.png", "d/y.webp"]);
    }

    #[test]
    fn a_description_loses_its_html() {
        assert_eq!(strip_html("&lt;p&gt;A &amp; B&lt;/p&gt;".replace("&lt;", "<").replace("&gt;", ">").as_str()), "A & B");
        assert_eq!(strip_html("Plain text"), "Plain text");
    }
}
