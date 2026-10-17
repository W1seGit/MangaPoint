use catalog::CatalogError;
use converter::xtc::XtcError;
use std::path::PathBuf;
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
