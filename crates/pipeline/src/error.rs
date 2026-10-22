use catalog::CatalogError;
use converter::xtc::XtcError;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("could not read a page image: {0}")]
    Decode(String),
    #[error("could not build the file: {0}")]
    Xtc(#[from] XtcError),
    #[error("could not build the EPUB: {0}")]
    Epub(String),
    #[error("file error on {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("the job was cancelled")]
    Cancelled,
    #[error("no chapters could be downloaded")]
    NothingDownloaded,
    #[error("chapter '{name}' could not be downloaded from any source: {reason}")]
    ChapterFailed { name: String, reason: String },
    #[error("not enough space on {mount}: need {needed} bytes, {available} available")]
    NoSpace {
        mount: PathBuf,
        needed: u64,
        available: u64,
    },
    #[error("upload failed: {0}")]
    Upload(String),
    #[error("the job has no chapters")]
    EmptyJob,
    #[error("import: {0}")]
    Import(String),
}

pub type Result<T> = std::result::Result<T, PipelineError>;

impl PipelineError {
    /// What `formats` could not do while writing the file at `path`.
    pub fn from_format(path: &Path, e: formats::FormatError) -> Self {
        match e {
            formats::FormatError::Io(e) => PipelineError::Io(path.to_path_buf(), e),
            formats::FormatError::Bad { format: "EPUB", message } => PipelineError::Epub(message),
            formats::FormatError::Bad { format, message } => PipelineError::Epub(format!("{format}: {message}")),
            formats::FormatError::Decode(message) => PipelineError::Decode(message),
        }
    }
}

/// What `formats` could not do with no file involved (making a page picture in memory).
impl From<formats::FormatError> for PipelineError {
    fn from(e: formats::FormatError) -> Self {
        PipelineError::from_format(Path::new(""), e)
    }
}
