//! The files MangaPoint makes and reads, as bytes: the same code for the desktop app (through
//! `crates/pipeline`, which writes them to a file) and the website (through `crates/web-engine`, which
//! makes them in memory), so a book is the same byte for byte from both.
//!
//! - `cbz`, `epub`, `pdf`: the writers, page by page, to any `Write` (`Write + Seek` for a zip).
//! - `original`: pages kept as they were saved (for a phone or a tablet), and one width for a strip.
//! - `comicinfo`: reading a book's `ComicInfo.xml`.
//! - `fingerprint`: a file's identity, the same on every device (how the reading-state file finds a book).
//! - `names`: chapter numbers, names and titles from the names inside a book.
//!
//! The writers are behind the `write` feature (on by default), so a user of only the readers does not
//! build the picture codecs.

pub mod comicinfo;
pub mod fingerprint;
pub mod names;

#[cfg(feature = "write")]
pub mod cbz;
#[cfg(feature = "write")]
pub mod epub;
#[cfg(feature = "write")]
pub mod original;
#[cfg(feature = "write")]
pub mod pdf;

/// Why a file could not be made.
#[derive(Debug)]
pub enum FormatError {
    /// Writing it failed.
    Io(std::io::Error),
    /// It cannot be made this way: a picture of the wrong kind, a book with no pages. `format` is
    /// the kind of file ("CBZ", "EPUB", "PDF").
    Bad { format: &'static str, message: String },
    /// A page picture could not be read.
    Decode(String),
}

impl FormatError {
    pub fn bad(format: &'static str, message: impl std::fmt::Display) -> Self {
        FormatError::Bad { format, message: message.to_string() }
    }
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Io(e) => write!(f, "{e}"),
            FormatError::Bad { format, message } => write!(f, "{format}: {message}"),
            FormatError::Decode(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, FormatError>;

impl From<FormatError> for String {
    fn from(error: FormatError) -> Self { error.to_string() }
}
