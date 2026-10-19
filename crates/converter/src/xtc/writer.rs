//! Assembling pages into an XTC/XTCH container.

use super::encode::encode_page;
use super::{
    Format, ReadDirection, XtcError, CHAPTER_SIZE, HEADER_SIZE, METADATA_SIZE,
    PAGE_TABLE_ENTRY_SIZE,
};
use crate::gray::LevelImage;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Book-level information shown by the reader. Strings are truncated to fit
/// their fixed-size fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub publisher: String,
    /// BCP 47 tag such as "en-US" or "ja-JP".
    pub language: String,
    /// Seconds since the Unix epoch; 0 if unknown.
    pub create_time: u32,
    /// 0-based index of the cover page.
    pub cover_page: Option<u16>,
}

/// A chapter entry for the reader's chapter menu. Pages are 0-based and
/// `end_page` is inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    pub name: String,
    pub start_page: u16,
    pub end_page: u16,
}

/// What the page table needs to know about each encoded page.
struct PageEntry {
    len: u32,
    width: u16,
    height: u16,
}

/// Encoded pages written to a temporary file as they arrive, so a book of
/// thousands of pages does not have to fit in memory. The file is deleted
/// when the builder is finished or dropped.
struct Spool {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for Spool {
    fn drop(&mut self) {
        // Close the file first: Windows cannot delete an open file. (In the
        // browser build `File` has nothing to drop, which clippy notices.)
        #[allow(clippy::drop_non_drop)]
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

enum PageData {
    Memory(Vec<Vec<u8>>),
    Spool(Spool),
}

/// Collects pages and writes them out as one file.
///
/// The page table precedes the data and needs every page's size, so pages
/// are held back until the file is written. By default they stay in memory
/// (a 1000-page XTCH book is about 96 MB); [`XtcBuilder::spool_to`] keeps
/// them in a temporary file instead.
pub struct XtcBuilder {
    format: Format,
    direction: ReadDirection,
    metadata: Option<Metadata>,
    chapters: Vec<Chapter>,
    entries: Vec<PageEntry>,
    data: PageData,
}

impl XtcBuilder {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            direction: ReadDirection::LeftToRight,
            metadata: None,
            chapters: Vec::new(),
            entries: Vec::new(),
            data: PageData::Memory(Vec::new()),
        }
    }

    /// Keep encoded pages in a temporary file at `path` instead of memory.
    /// Call before adding pages.
    pub fn spool_to(mut self, path: impl AsRef<Path>) -> Result<Self, XtcError> {
        if !self.entries.is_empty() {
            return Err(XtcError::Spool("pages were already added".into()));
        }
        let path = path.as_ref().to_path_buf();
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| XtcError::Spool(e.to_string()))?;
        self.data = PageData::Spool(Spool {
            path,
            file: Some(file),
        });
        Ok(self)
    }

    pub fn direction(mut self, direction: ReadDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn chapters(mut self, chapters: Vec<Chapter>) -> Self {
        self.chapters = chapters;
        self
    }

    pub fn page_count(&self) -> usize {
        self.entries.len()
    }

    /// Add a chapter entry after the builder was created, for callers that
    /// only learn where a chapter ends once its pages have been added.
    pub fn push_chapter(&mut self, chapter: Chapter) {
        self.chapters.push(chapter);
    }

    /// Encode and append a page. Its depth must match the container format.
    pub fn add_page(&mut self, page: &LevelImage) -> Result<(), XtcError> {
        if page.depth != self.format.depth() {
            return Err(XtcError::DepthMismatch {
                expected: self.format.depth(),
                found: page.depth,
            });
        }
        let bytes = encode_page(page)?;
        self.add_encoded_page(&bytes)
    }

    /// Append a previously packed XTG/XTH page without unpacking or encoding
    /// its pixels again. The header, depth and bitmap length are validated.
    pub fn add_encoded_page(&mut self, bytes: &[u8]) -> Result<(), XtcError> {
        let (width, height, depth) = super::encode::validate_page(bytes, self.entries.len())?;
        if depth != self.format.depth() {
            return Err(XtcError::DepthMismatch {
                expected: self.format.depth(),
                found: depth,
            });
        }
        if self.entries.len() >= u16::MAX as usize {
            return Err(XtcError::TooManyPages(self.entries.len() + 1));
        }
        let entry = PageEntry {
            len: bytes.len() as u32,
            width: width as u16,
            height: height as u16,
        };
        match &mut self.data {
            PageData::Memory(pages) => pages.push(bytes.to_vec()),
            PageData::Spool(spool) => spool
                .file
                .as_mut()
                .expect("the spool file is open until the builder is finished")
                .write_all(bytes)
                .map_err(|e| XtcError::Spool(e.to_string()))?,
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Produce the finished file.
    pub fn build(self) -> Result<Vec<u8>, XtcError> {
        let mut out = Vec::new();
        self.write_to(&mut out).map_err(|e| match e {
            WriteError::Xtc(e) => e,
            WriteError::Io(_) | WriteError::Cancelled => {
                unreachable!("writing to a Vec cannot fail or be cancelled")
            }
        })?;
        Ok(out)
    }

    /// Write the finished file to any `Write`.
    pub fn write_to<W: Write>(self, w: &mut W) -> Result<(), WriteError> {
        self.write_to_cancellable(w, &|| false)
    }

    /// [`write_to`](Self::write_to), asking `cancel` between pieces of a
    /// book that may be gigabytes; `Err(WriteError::Cancelled)` once it is true.
    pub fn write_to_cancellable<W: Write>(
        mut self,
        w: &mut W,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), WriteError> {
        let count = self.entries.len();
        if count == 0 {
            return Err(XtcError::NoPages.into());
        }
        for c in &self.chapters {
            if c.start_page > c.end_page || c.end_page as usize >= count {
                return Err(XtcError::BadChapter {
                    name: c.name.clone(),
                    start: c.start_page,
                    end: c.end_page,
                    pages: count,
                }
                .into());
            }
        }

        let has_meta = self.metadata.is_some();
        let has_chapters = !self.chapters.is_empty();
        let meta_offset = HEADER_SIZE;
        let chapter_offset = meta_offset + if has_meta { METADATA_SIZE } else { 0 };
        let table_offset = chapter_offset + self.chapters.len() * CHAPTER_SIZE;
        let data_offset = table_offset + count * PAGE_TABLE_ENTRY_SIZE;

        // Header (56 bytes).
        let mut head = Vec::with_capacity(data_offset);
        head.extend_from_slice(&self.format.magic().to_le_bytes());
        head.push(1); // version major
        head.push(0); // version minor
        head.extend_from_slice(&(count as u16).to_le_bytes());
        head.push(self.direction as u8);
        head.push(has_meta as u8);
        head.push(0); // hasThumbnails
        head.push(has_chapters as u8);
        head.extend_from_slice(&1u32.to_le_bytes()); // currentPage, 1-based
        head.extend_from_slice(&(if has_meta { meta_offset as u64 } else { 0 }).to_le_bytes());
        head.extend_from_slice(&(table_offset as u64).to_le_bytes());
        head.extend_from_slice(&(data_offset as u64).to_le_bytes());
        head.extend_from_slice(&0u64.to_le_bytes()); // thumbOffset
        head.extend_from_slice(
            &(if has_chapters {
                chapter_offset as u32
            } else {
                0
            })
            .to_le_bytes(),
        );
        head.extend_from_slice(&0u32.to_le_bytes()); // padding
        debug_assert_eq!(head.len(), HEADER_SIZE);

        // Metadata (256 bytes) right after the header.
        if let Some(m) = &self.metadata {
            put_str(&mut head, &m.title, 128);
            put_str(&mut head, &m.author, 64);
            put_str(&mut head, &m.publisher, 32);
            put_str(&mut head, &m.language, 16);
            head.extend_from_slice(&m.create_time.to_le_bytes());
            head.extend_from_slice(&m.cover_page.unwrap_or(0xFFFF).to_le_bytes());
            head.extend_from_slice(&(self.chapters.len() as u16).to_le_bytes());
            head.extend_from_slice(&0u64.to_le_bytes()); // reserved
            debug_assert_eq!(head.len(), HEADER_SIZE + METADATA_SIZE);
        }

        // Chapters (96 bytes each), packed directly before the page table.
        for c in &self.chapters {
            put_str(&mut head, &c.name, 80);
            head.extend_from_slice(&c.start_page.to_le_bytes());
            head.extend_from_slice(&c.end_page.to_le_bytes());
            head.extend_from_slice(&[0u8; 12]);
        }

        // Page table (16 bytes each).
        let mut offset = data_offset as u64;
        for p in &self.entries {
            head.extend_from_slice(&offset.to_le_bytes());
            head.extend_from_slice(&p.len.to_le_bytes());
            head.extend_from_slice(&p.width.to_le_bytes());
            head.extend_from_slice(&p.height.to_le_bytes());
            offset += p.len as u64;
        }
        debug_assert_eq!(head.len(), data_offset);

        w.write_all(&head)?;
        match &mut self.data {
            PageData::Memory(pages) => {
                for bytes in pages.iter() {
                    if cancel() {
                        return Err(WriteError::Cancelled);
                    }
                    w.write_all(bytes)?;
                }
            }
            PageData::Spool(spool) => {
                let file = spool
                    .file
                    .as_mut()
                    .expect("the spool file is open until the builder is finished");
                file.flush()?;
                file.seek(SeekFrom::Start(0))?;
                let mut reader = BufReader::new(&*file);
                let mut chunk = vec![0u8; 256 * 1024];
                loop {
                    if cancel() {
                        return Err(WriteError::Cancelled);
                    }
                    let n = reader.read(&mut chunk)?;
                    if n == 0 {
                        break;
                    }
                    w.write_all(&chunk[..n])?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Xtc(#[from] XtcError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("writing the book was cancelled")]
    Cancelled,
}

/// Append `s` as a fixed-size UTF-8 field: truncated on a character boundary
/// to leave room for a terminating NUL, then zero-padded.
fn put_str(buf: &mut Vec<u8>, s: &str, len: usize) {
    let mut end = s.len().min(len - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    buf.extend_from_slice(&s.as_bytes()[..end]);
    buf.resize(buf.len() + (len - end), 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gray::Depth;

    fn blank(depth: Depth) -> LevelImage {
        LevelImage {
            width: 8,
            height: 8,
            depth,
            levels: vec![depth.max_level(); 64],
        }
    }

    #[test]
    fn packed_pages_preserve_both_formats_and_reject_invalid_data_before_appending() {
        for format in [Format::Xtc, Format::Xtch] {
            let mut builder = XtcBuilder::new(format);
            let image = varied(format.depth(), 3);
            let packed = encode_page(&image).unwrap();
            let other = if format == Format::Xtc {
                Depth::Two
            } else {
                Depth::One
            };
            assert!(matches!(
                builder.add_encoded_page(&encode_page(&blank(other)).unwrap()),
                Err(XtcError::DepthMismatch { .. })
            ));
            assert!(builder
                .add_encoded_page(&packed[..packed.len() - 1])
                .is_err());
            let mut oversized = packed.clone();
            oversized.push(0);
            assert!(builder.add_encoded_page(&oversized).is_err());
            for offset in [0, 4, 8, 9, 10] {
                let mut damaged = packed.clone();
                if offset == 4 {
                    damaged[4..6].fill(0);
                } else {
                    damaged[offset] ^= 0xff;
                }
                assert!(builder.add_encoded_page(&damaged).is_err());
            }
            assert_eq!(builder.page_count(), 0);
            builder.add_encoded_page(&packed).unwrap();
            let bytes = builder.build().unwrap();
            let file = super::super::XtcFile::parse(&bytes).unwrap();
            assert_eq!(file.decode_page(0).unwrap().image, image);
        }
    }

    #[test]
    fn put_str_truncates_on_char_boundary_and_pads() {
        let mut b = Vec::new();
        put_str(&mut b, "héllo", 4); // 'é' is two bytes: "h" + 'é' would need 3, then NUL
        assert_eq!(b.len(), 4);
        assert_eq!(&b[..3], "hé".as_bytes());
        assert_eq!(b[3], 0);

        let mut b = Vec::new();
        put_str(&mut b, "日本語", 5); // each char is 3 bytes; only one fits in 4
        assert_eq!(b.len(), 5);
        assert_eq!(&b[..3], "日".as_bytes());
        assert_eq!(&b[3..], &[0, 0]);
    }

    #[test]
    fn rejects_empty_mismatched_and_bad_chapters() {
        let b = XtcBuilder::new(Format::Xtc);
        assert_eq!(b.build(), Err(XtcError::NoPages));

        let mut b = XtcBuilder::new(Format::Xtc);
        assert!(matches!(
            b.add_page(&blank(Depth::Two)),
            Err(XtcError::DepthMismatch { .. })
        ));

        let mut b = XtcBuilder::new(Format::Xtc).chapters(vec![Chapter {
            name: "x".into(),
            start_page: 0,
            end_page: 5,
        }]);
        b.add_page(&blank(Depth::One)).unwrap();
        assert!(matches!(b.build(), Err(XtcError::BadChapter { .. })));
    }

    fn varied(depth: Depth, seed: u32) -> LevelImage {
        let max = depth.max_level() as u32 + 1;
        LevelImage {
            width: 12,
            height: 20,
            depth,
            levels: (0..240).map(|i| ((i * 7 + seed) % max) as u8).collect(),
        }
    }

    fn book(format: Format, spool: Option<&Path>) -> XtcBuilder {
        let depth = format.depth();
        let mut b = XtcBuilder::new(format)
            .direction(ReadDirection::RightToLeft)
            .metadata(Metadata {
                title: "Spool test".into(),
                ..Default::default()
            });
        if let Some(path) = spool {
            b = b.spool_to(path).unwrap();
        }
        for seed in 0..5 {
            b.add_page(&varied(depth, seed)).unwrap();
        }
        b.push_chapter(Chapter {
            name: "One".into(),
            start_page: 0,
            end_page: 4,
        });
        b
    }

    #[test]
    fn a_book_being_written_stops_when_cancelled() {
        for spool in [false, true] {
            let dir = std::env::temp_dir().join(format!("mp-cancel-write-{}-{spool}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("book.spool");
            let b = book(Format::Xtc, spool.then_some(path.as_path()));
            let mut out = Vec::new();
            let result = b.write_to_cancellable(&mut out, &|| true);
            assert!(matches!(result, Err(WriteError::Cancelled)), "spool {spool}");
            assert!(out.len() < 100_000, "stopped before the pages: {} bytes", out.len());
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_spooled_book_is_byte_identical_to_one_built_in_memory() {
        for format in [Format::Xtc, Format::Xtch] {
            let path = std::env::temp_dir().join(format!(
                "mangapoint-spool-{}-{}.tmp",
                format.extension(),
                std::process::id()
            ));
            let in_memory = book(format, None).build().unwrap();
            let mut spooled = Vec::new();
            book(format, Some(&path)).write_to(&mut spooled).unwrap();
            assert_eq!(in_memory, spooled, "{format:?}");
        }
    }

    #[test]
    fn the_spool_file_is_deleted_when_the_builder_is_done_or_dropped() {
        let dir = std::env::temp_dir();
        let finished = dir.join(format!("mangapoint-spool-done-{}.tmp", std::process::id()));
        let mut sink = Vec::new();
        book(Format::Xtc, Some(&finished))
            .write_to(&mut sink)
            .unwrap();
        assert!(!finished.exists(), "removed after writing");

        let abandoned = dir.join(format!("mangapoint-spool-drop-{}.tmp", std::process::id()));
        let b = book(Format::Xtc, Some(&abandoned));
        assert!(abandoned.exists(), "present while the book is being built");
        drop(b);
        assert!(!abandoned.exists(), "removed when dropped without writing");
    }

    #[test]
    fn spooling_must_start_before_the_first_page() {
        let path =
            std::env::temp_dir().join(format!("mangapoint-spool-late-{}.tmp", std::process::id()));
        let mut b = XtcBuilder::new(Format::Xtc);
        b.add_page(&blank(Depth::One)).unwrap();
        assert!(matches!(b.spool_to(&path), Err(XtcError::Spool(_))));
        assert!(!path.exists());
    }

    #[test]
    fn header_fields_are_at_the_documented_offsets() {
        let mut b = XtcBuilder::new(Format::Xtch)
            .direction(ReadDirection::RightToLeft)
            .metadata(Metadata {
                title: "T".into(),
                author: "A".into(),
                ..Default::default()
            })
            .chapters(vec![Chapter {
                name: "C1".into(),
                start_page: 0,
                end_page: 1,
            }]);
        b.add_page(&blank(Depth::Two)).unwrap();
        b.add_page(&blank(Depth::Two)).unwrap();
        let f = b.build().unwrap();

        assert_eq!(&f[0..4], b"XTCH");
        assert_eq!(f[4], 1); // version major
        assert_eq!(f[5], 0); // version minor
        assert_eq!(u16::from_le_bytes([f[6], f[7]]), 2); // pageCount
        assert_eq!(f[8], 1); // readDirection: RTL
        assert_eq!((f[9], f[10], f[11]), (1, 0, 1)); // meta, thumbs, chapters
        let u64_at = |o: usize| u64::from_le_bytes(f[o..o + 8].try_into().unwrap());
        assert_eq!(u64_at(0x10), 56); // metadataOffset
        let table = 56 + 256 + 96;
        assert_eq!(u64_at(0x18), table as u64); // pageTableOffset
        assert_eq!(u64_at(0x20), (table + 32) as u64); // dataOffset
        assert_eq!(u64_at(0x28), 0); // thumbOffset
                                     // chapterOffset is read as u64 by CrossPoint; the padding must be zero.
        assert_eq!(u64_at(0x30), 56 + 256);
        // Title and author at the absolute offsets CrossPoint uses.
        assert_eq!(f[0x38], b'T');
        assert_eq!(f[0xB8], b'A');
    }
}
