//! PDF files written to a file. How a PDF is made is `formats::pdf` (shared with the website); here
//! every page goes to a file on disk at once, so a book never sits in memory, and a failure names the
//! file.

use crate::error::{PipelineError, Result};
pub use formats::pdf::{jpeg_info, JpegInfo, PdfColor, PdfImage, PdfMeta};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// A PDF being written to a file.
pub struct PdfBuilder {
    book: formats::pdf::PdfBuilder<BufWriter<File>>,
    path: PathBuf,
}

impl PdfBuilder {
    /// Start a document at `path`.
    pub fn create(path: &Path, meta: PdfMeta) -> Result<Self> {
        let file = File::create(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
        let book = formats::pdf::PdfBuilder::new(BufWriter::new(file), meta).map_err(|e| PipelineError::from_format(path, e))?;
        Ok(Self { book, path: path.to_path_buf() })
    }

    pub fn page_count(&self) -> usize {
        self.book.page_count()
    }

    /// A chapter that starts at the 0-based page `start`.
    pub fn push_chapter(&mut self, name: &str, start: usize) {
        self.book.push_chapter(name, start);
    }

    /// Add one page, the size of its picture.
    pub fn add_page(&mut self, image: PdfImage<'_>) -> Result<()> {
        self.book.add_page(image).map_err(|e| PipelineError::from_format(&self.path, e))
    }

    /// The document itself and its file, for the writers of `formats` that add to it.
    pub fn parts(&mut self) -> (&mut formats::pdf::PdfBuilder<BufWriter<File>>, &Path) {
        (&mut self.book, &self.path)
    }

    /// Write the outline, the page tree, the catalog and the cross-reference table, and close the file.
    pub fn finish(self) -> Result<()> {
        let path = self.path;
        self.book.finish().map(drop).map_err(|e| PipelineError::from_format(&path, e))
    }
}
