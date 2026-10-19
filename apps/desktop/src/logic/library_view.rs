//! The Library's views, sorts and filter.

/// Which series the Library shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryView {
    #[default]
    All,
    /// Series with chapters on this computer.
    Downloaded,
    /// Series that have been exported for a reader.
    Exported,
    /// Series put in the library but not downloaded yet.
    Saved,
}

impl LibraryView {
    pub const ALL: [LibraryView; 4] = [
        LibraryView::All,
        LibraryView::Downloaded,
        LibraryView::Exported,
        LibraryView::Saved,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryView::All => "All",
            LibraryView::Downloaded => "Downloaded",
            LibraryView::Exported => "Exported",
            LibraryView::Saved => "Saved for later",
        }
    }

    pub fn includes(self, series: &library::DownloadedSeries, exported: bool) -> bool {
        match self {
            LibraryView::All => true,
            LibraryView::Downloaded => !series.chapters.is_empty(),
            LibraryView::Exported => exported,
            LibraryView::Saved => series.chapters.is_empty(),
        }
    }
}

/// How the Library orders its series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibrarySort {
    /// The ones touched most recently first.
    #[default]
    Recent,
    Title,
    MostChapters,
    Size,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 4] = [
        LibrarySort::Recent,
        LibrarySort::Title,
        LibrarySort::MostChapters,
        LibrarySort::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::Recent => "Recently added",
            LibrarySort::Title => "Title A to Z",
            LibrarySort::MostChapters => "Most chapters",
            LibrarySort::Size => "Biggest on disk",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LibrarySort::Recent => "recent",
            LibrarySort::Title => "title",
            LibrarySort::MostChapters => "chapters",
            LibrarySort::Size => "size",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The library's series to show, as positions in `series`: those in the view
/// that match the filter text (in the title, the author or a genre), in order.
pub fn library_order<S: std::borrow::Borrow<library::DownloadedSeries>>(
    series: &[S],
    view: LibraryView,
    sort: LibrarySort,
    filter: &str,
    exported: &std::collections::HashSet<String>,
) -> Vec<usize> {
    let at = |i: usize| -> &library::DownloadedSeries { series[i].borrow() };
    let needle = filter.trim().to_lowercase();
    let matches = |s: &library::DownloadedSeries| {
        needle.is_empty()
            || s.title.to_lowercase().contains(&needle)
            || s.info.as_ref().is_some_and(|i| {
                i.author.to_lowercase().contains(&needle)
                    || i.genres.iter().any(|g| g.to_lowercase().contains(&needle))
            })
    };
    let mut order: Vec<usize> = (0..series.len())
        .filter(|&i| view.includes(at(i), exported.contains(&at(i).id)) && matches(at(i)))
        .collect();
    match sort {
        LibrarySort::Recent => {
            order.sort_by_key(|&i| std::cmp::Reverse(at(i).updated_at.max(at(i).added_at)))
        }
        LibrarySort::Title => order.sort_by_key(|&i| at(i).title.to_lowercase()),
        LibrarySort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(at(i).chapters.len()))
        }
        LibrarySort::Size => order.sort_by_key(|&i| std::cmp::Reverse(at(i).bytes())),
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::{DownloadedChapter, DownloadedSeries, SeriesInfo};

    fn chapter(bytes: u64) -> DownloadedChapter {
        DownloadedChapter {
            key: "c1".into(),
            number: Some(1.0),
            name: "Chapter 1".into(),
            source_id: "s".into(),
            chapter_id: 1,
            pages: 10,
            bytes,
            downloaded_at: 0,
        }
    }

    fn series(
        title: &str,
        chapters: usize,
        bytes: u64,
        added: u64,
        updated: u64,
    ) -> DownloadedSeries {
        DownloadedSeries {
            id: title.to_lowercase(),
            title: title.into(),
            chapters: (0..chapters).map(|_| chapter(bytes)).collect(),
            added_at: added,
            updated_at: updated,
            ..Default::default()
        }
    }

    fn shelf() -> Vec<DownloadedSeries> {
        let mut a = series("Berserk", 3, 100, 10, 50);
        a.info = Some(SeriesInfo {
            author: "Kentaro Miura".into(),
            genres: vec!["Dark Fantasy".into()],
            ..Default::default()
        });
        vec![
            a,
            series("alpha", 0, 0, 40, 40), // saved, not downloaded
            series("Zeta", 10, 5, 30, 30),
            series("Delta", 0, 0, 5, 5),
        ]
    }

    #[test]
    fn the_views_split_downloaded_from_saved() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let order = |v| library_order(&s, v, LibrarySort::Title, "", &none);
        assert_eq!(order(LibraryView::All), [1, 0, 3, 2]);
        assert!(order(LibraryView::Exported).is_empty());
        let berserk = std::collections::HashSet::from(["berserk".to_string()]);
        assert_eq!(
            library_order(&s, LibraryView::Exported, LibrarySort::Title, "", &berserk),
            [0]
        );
        assert_eq!(order(LibraryView::Downloaded), [0, 2]);
        assert_eq!(order(LibraryView::Saved), [1, 3]);
    }

    #[test]
    fn the_sorts_order_as_they_say() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let by = |sort| library_order(&s, LibraryView::All, sort, "", &none);
        assert_eq!(
            by(LibrarySort::Recent),
            [0, 1, 2, 3],
            "latest touch first (50, 40, 30, 5)"
        );
        assert_eq!(
            by(LibrarySort::Title),
            [1, 0, 3, 2],
            "A to Z, ignoring case"
        );
        assert_eq!(by(LibrarySort::MostChapters), [2, 0, 1, 3]);
        assert_eq!(
            by(LibrarySort::Size),
            [0, 2, 1, 3],
            "300 bytes, 50, then the empty ones"
        );
    }

    #[test]
    fn the_filter_looks_at_the_title_author_and_genres() {
        let s = shelf();
        let none = std::collections::HashSet::new();
        let find = |text: &str| library_order(&s, LibraryView::All, LibrarySort::Title, text, &none);
        assert_eq!(find("  ZET "), [2]);
        assert_eq!(find("miura"), [0], "author");
        assert_eq!(find("dark fantasy"), [0], "genre");
        assert!(find("nothing like this").is_empty());
        assert_eq!(
            library_order(&s, LibraryView::Saved, LibrarySort::Title, "berserk", &none),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in LibrarySort::ALL {
            assert_eq!(LibrarySort::from_key(sort.key()), sort);
        }
        assert_eq!(LibrarySort::from_key("???"), LibrarySort::Recent);
    }
}
