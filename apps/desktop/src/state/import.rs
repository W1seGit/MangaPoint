use std::sync::Arc;

// ---- Importing -------------------------------------------------------------

/// The details of a book or series that a person can type: what an import
/// shows for each book before it is made, and what the edit page changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InfoDraft {
    pub title: String,
    pub description: String,
    pub author: String,
    pub artist: String,
    /// "", "Ongoing", "Completed", "On hiatus" or "Cancelled".
    pub status: String,
    pub genres: Vec<String>,
    /// The cover as a picture to show.
    pub cover_url: Option<String>,
    /// A cover the person chose: the bytes of the picture. `None` keeps the
    /// book's own (or the series' current) cover.
    pub cover: Option<Arc<Vec<u8>>>,
}

/// What to do with a chapter that has the same number as one the series has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChapterPlace {
    /// Leave it out.
    #[default]
    Skip,
    /// Put its pages after the ones that chapter has.
    Add,
    /// Take the place of that chapter.
    Replace,
    /// Keep both, as two chapters.
    Both,
}

/// One chapter of a book to import, as the person sees and corrects it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterDraft {
    /// What is typed in the number box; empty means none.
    pub number: String,
    pub name: String,
    pub pages: usize,
    pub include: bool,
    /// The key of the chapter this one has the number of, if there is one.
    pub meets: Option<String>,
    pub place: ChapterPlace,
}

/// One book to import.
#[derive(Clone, Debug, PartialEq)]
pub struct BookDraft {
    pub label: String,
    pub format: &'static str,
    pub info: InfoDraft,
    pub chapters: Vec<ChapterDraft>,
    pub warnings: Vec<String>,
    pub include: bool,
    /// Whether the book's card is open (a folded card shows only its title bar).
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportStage {
    /// Reading the files that were chosen.
    Reading,
    /// Showing what was found, to correct before importing.
    Review,
    Importing,
    Finished,
}

/// A series made or added to by an import.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportDone {
    pub series_id: String,
    pub title: String,
    pub chapters: usize,
    pub pages: usize,
    pub skipped: usize,
    /// Chapters the series already had, which were left out.
    pub already: usize,
}

/// The import in progress: the dialog's whole state.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSession {
    /// The series pages are being added to, or `None` for new ones.
    pub target: Option<String>,
    pub stage: ImportStage,
    pub books: Vec<BookDraft>,
    /// Files that could not be read, and why.
    pub failures: Vec<(String, String)>,
    /// Make one series of all the books.
    pub combine: bool,
    pub done_pages: usize,
    pub total_pages: usize,
    pub finished: Vec<ImportDone>,
    pub errors: Vec<String>,
}
