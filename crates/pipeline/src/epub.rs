//! EPUB files written to a file. How an EPUB is made is `formats::epub` (shared with the website);
//! here the pages go to a file on disk as they arrive, so a book does not have to fit in memory, and a
//! failure names the file.

use crate::error::{PipelineError, Result};
pub use formats::epub::{typical_color_page_bytes, typical_page_bytes, EpubMeta, MAX_PAGES};
use converter::{ColorImage, LevelImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// One page as a picture: a PNG for pages of up to four grays, a JPEG for full grays.
pub fn encode_page(img: &LevelImage) -> Result<Vec<u8>> {
    Ok(formats::epub::encode_page(img)?)
}

/// One page kept in colour, as a JPEG (see `formats::epub::encode_color_page`).
pub fn encode_color_page(img: &ColorImage) -> Result<Vec<u8>> {
    Ok(formats::epub::encode_color_page(img)?)
}

/// An EPUB being written to a file, page by page.
pub struct EpubBuilder {
    book: formats::epub::EpubBuilder<BufWriter<File>>,
    path: PathBuf,
}

impl EpubBuilder {
    /// Start a book at `path`.
    pub fn create(path: &Path, meta: EpubMeta) -> Result<Self> {
        let file = File::create(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        let book = formats::epub::EpubBuilder::new(BufWriter::new(file), meta).map_err(|e| PipelineError::from_format(path, e))?;
        Ok(Self { book, path: path.to_path_buf() })
    }

    /// How many pages the book has so far.
    pub fn page_count(&self) -> usize {
        self.book.page_count()
    }

    /// Add a page that [`encode_page`] made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<()> {
        self.book.add_page(picture).map_err(|e| PipelineError::from_format(&self.path, e))
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize) {
        self.book.push_chapter(name, start);
    }

    /// Write the contents and the package, and close the file.
    pub fn finish(self) -> Result<()> {
        let path = self.path;
        self.book.finish().map(drop).map_err(|e| PipelineError::from_format(&path, e))
    }
}
