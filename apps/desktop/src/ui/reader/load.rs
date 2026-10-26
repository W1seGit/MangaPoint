//! Reading a chapter's pages off the disk for the reader: which files they are
//! and how big each is, from their headers only (the pictures themselves are
//! fetched by the web view, see `pages`). All of it blocks, so it is called
//! from a thread of its own.

use crate::logic;
use library::{DownloadedChapter, LibraryFiles};
use std::io::Read;
use std::path::Path;

/// One saved page.
#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    /// The file's name in the chapter folder, which is its address.
    pub file: String,
    /// Its size in pixels; 0 when the header could not be read.
    pub w: u32,
    pub h: u32,
}

/// The pages of one chapter, as they are on disk now.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterPages {
    pub key: String,
    /// Changes when the chapter's pages are replaced by a new download.
    pub version: String,
    pub pages: Vec<PageInfo>,
}

/// What tells one download of a chapter from another.
pub fn version_of(chapter: &DownloadedChapter) -> String {
    format!("{}-{}-{}", chapter.downloaded_at, chapter.bytes, chapter.pages)
}

/// The first `limit` bytes of a file.
fn head(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    std::fs::File::open(path).ok()?.take(limit).read_to_end(&mut out).ok()?;
    Some(out)
}

/// The size of a picture from its header. A header further in than the start
/// of the file (a JPEG with a big thumbnail in it) is looked for in more of it.
fn size_of(path: &Path) -> (u32, u32) {
    for limit in [64 * 1024, 1024 * 1024, 64 * 1024 * 1024] {
        let Some(bytes) = head(path, limit) else { break };
        if let Ok(size) = converter::decode::image_dimensions(&bytes) {
            return size;
        }
        if (bytes.len() as u64) < limit {
            break;
        }
    }
    (0, 0)
}

/// List a chapter's saved pages in reading order.
pub fn list_pages(files: &LibraryFiles, series_id: &str, chapter: &DownloadedChapter) -> ChapterPages {
    let pages = files
        .page_files(series_id, &chapter.key)
        .iter()
        .filter_map(|path| {
            let file = path.file_name()?.to_string_lossy().into_owned();
            let (w, h) = size_of(path);
            Some(PageInfo { file, w, h })
        })
        .collect();
    ChapterPages { key: chapter.key.clone(), version: version_of(chapter), pages }
}

/// The chapters that were saved small and could be downloaded again at the
/// original quality (see `logic::chapters_to_improve`).
pub fn improvable(files: &LibraryFiles, series_id: &str, chapters: &[DownloadedChapter]) -> Vec<String> {
    logic::chapters_to_improve(chapters, &|chapter| {
        files
            .page_files(series_id, &chapter.key)
            .first()
            .and_then(|path| head(path, 64 * 1024))
            .is_some_and(|bytes| logic::looks_saved_small(&bytes))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::PageQuality;

    fn temp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-reader-load-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        use image::ImageEncoder;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&vec![200u8; (w * h) as usize], w, h, image::ExtendedColorType::L8)
            .unwrap();
        out
    }

    #[test]
    fn pages_are_listed_in_order_with_their_sizes_and_bad_ones_get_none() {
        let root = temp("list");
        let files = LibraryFiles::new(root.join("library"));
        std::fs::create_dir_all(files.chapter_dir("s-1", "c1")).unwrap();
        std::fs::write(files.page_path("s-1", "c1", 0), png(30, 40)).unwrap();
        std::fs::write(files.page_path("s-1", "c1", 1), b"not a picture").unwrap();
        std::fs::write(files.page_path("s-1", "c1", 2), png(50, 20)).unwrap();
        let chapter = DownloadedChapter { key: "c1".into(), pages: 3, bytes: 9, downloaded_at: 4, ..Default::default() };
        let listed = list_pages(&files, "s-1", &chapter);
        let sizes: Vec<_> = listed.pages.iter().map(|p| (p.file.as_str(), p.w, p.h)).collect();
        assert_eq!(sizes, [("00001.img", 30, 40), ("00002.img", 0, 0), ("00003.img", 50, 20)]);
        assert_eq!(listed.version, "4-9-3");
        // A chapter folder that is gone lists nothing.
        let gone = DownloadedChapter { key: "c9".into(), ..Default::default() };
        assert!(list_pages(&files, "s-1", &gone).pages.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn chapters_saved_small_are_found_by_their_record_or_their_first_page() {
        let root = temp("improve");
        let files = LibraryFiles::new(root.join("library"));
        let gray_jpeg = {
            use image::ImageEncoder;
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
                .write_image(&vec![128u8; 16 * 16], 16, 16, image::ExtendedColorType::L8)
                .unwrap();
            out
        };
        for (key, bytes) in [("c1", gray_jpeg.clone()), ("c2", png(8, 8))] {
            std::fs::create_dir_all(files.chapter_dir("s-1", key)).unwrap();
            std::fs::write(files.page_path("s-1", key, 0), bytes).unwrap();
        }
        let chapter = |key: &str, quality| DownloadedChapter {
            key: key.into(),
            source_id: "src".into(),
            pages: 1,
            quality,
            ..Default::default()
        };
        let chapters = vec![
            chapter("c1", None),                     // old, first page is a gray JPEG
            chapter("c2", None),                     // old, a PNG: nothing claimed
            chapter("c3", Some(PageQuality::Save)),  // recorded
            chapter("c4", Some(PageQuality::Full)),  // recorded as original
        ];
        assert_eq!(improvable(&files, "s-1", &chapters), ["c1", "c3"]);
        let _ = std::fs::remove_dir_all(root);
    }
}
