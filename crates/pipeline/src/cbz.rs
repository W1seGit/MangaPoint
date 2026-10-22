//! CBZ files written to a file. How a CBZ is made is `formats::cbz` (shared with the website); here
//! the pages go to a file on disk as they arrive, so a book does not have to fit in memory, and a
//! failure names the file.

use crate::error::{PipelineError, Result};
pub use formats::cbz::CbzMeta;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// A CBZ being written to a file, page by page.
pub struct CbzBuilder {
    book: formats::cbz::CbzBuilder<BufWriter<File>>,
    path: PathBuf,
}

impl CbzBuilder {
    /// Start a book at `path`.
    pub fn create(path: &Path, meta: CbzMeta) -> Result<Self> {
        let file = File::create(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        Ok(Self { book: formats::cbz::CbzBuilder::new(BufWriter::new(file), meta), path: path.to_path_buf() })
    }

    /// How many pages the book has so far.
    pub fn page_count(&self) -> usize {
        self.book.page_count()
    }

    /// Add a page that `formats::epub::encode_page` made.
    pub fn add_page(&mut self, picture: &[u8]) -> Result<()> {
        self.book.add_page(picture).map_err(|e| PipelineError::from_format(&self.path, e))
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize, number: Option<f64>) {
        self.book.push_chapter(name, start, number);
    }

    /// The book itself and its file, for the writers of `formats` that add to it.
    pub fn parts(&mut self) -> (&mut formats::cbz::CbzBuilder<BufWriter<File>>, &Path) {
        (&mut self.book, &self.path)
    }

    /// Write `ComicInfo.xml` and close the file.
    pub fn finish(self) -> Result<()> {
        let path = self.path;
        self.book.finish().map(drop).map_err(|e| PipelineError::from_format(&path, e))
    }
}
