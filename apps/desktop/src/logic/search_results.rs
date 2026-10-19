//! Search results: one entry per source, the sort order and the filters.

use search::ScoredMember;
use std::collections::HashSet;

/// The best entry from each source, at most `limit` of them. `members` must
/// be ordered best first, as search results are.
///
/// A source can list one series twice (a duplicate page, another edition).
/// Loading and merging both adds nothing, and the screens key their lists on
/// the source, so each source appears once.
pub fn one_per_source(members: &[ScoredMember], limit: usize) -> Vec<ScoredMember> {
    let mut seen = HashSet::new();
    members
        .iter()
        .filter(|m| seen.insert(m.candidate.source_id.clone()))
        .take(limit)
        .cloned()
        .collect()
}

/// How search results are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSort {
    /// How well the title matches, then how good the sources are.
    #[default]
    BestMatch,
    MostChapters,
    RecentlyUpdated,
    MostSources,
    TitleAz,
}

impl SearchSort {
    pub const ALL: [SearchSort; 5] = [
        SearchSort::BestMatch,
        SearchSort::MostChapters,
        SearchSort::RecentlyUpdated,
        SearchSort::MostSources,
        SearchSort::TitleAz,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "Best match",
            SearchSort::MostChapters => "Most chapters",
            SearchSort::RecentlyUpdated => "Recently updated",
            SearchSort::MostSources => "Most sources",
            SearchSort::TitleAz => "Title A to Z",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "match",
            SearchSort::MostChapters => "chapters",
            SearchSort::RecentlyUpdated => "recent",
            SearchSort::MostSources => "sources",
            SearchSort::TitleAz => "az",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The order to show results in, as positions in `ranked`. Ties keep the
/// original (best match) order, and results that lack the thing being sorted
/// on come last.
pub fn sorted_order(ranked: &[search::RankedSeries], sort: SearchSort) -> Vec<usize> {
    let chapters = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.chapter_count))
            .max()
    };
    let updated = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.latest_upload_ms))
            .max()
    };
    let mut order: Vec<usize> = (0..ranked.len()).collect();
    match sort {
        SearchSort::BestMatch => {}
        SearchSort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(chapters(&ranked[i]).map(i64::from)))
        }
        SearchSort::RecentlyUpdated => {
            order.sort_by_key(|&i| std::cmp::Reverse(updated(&ranked[i])))
        }
        SearchSort::MostSources => {
            order.sort_by_key(|&i| std::cmp::Reverse(ranked[i].members.len()))
        }
        SearchSort::TitleAz => {
            order.sort_by_key(|&i| ranked[i].title.to_lowercase());
        }
    }
    order
}

/// The kinds of series that can be picked, as sources tag them.
pub const SERIES_TYPES: [&str; 5] = ["Manga", "Manhwa", "Manhua", "Webtoon", "Comic"];

/// Genres offered in the filter.
pub const COMMON_GENRES: [&str; 24] = [
    "Action", "Adventure", "Comedy", "Drama", "Fantasy", "Horror", "Isekai", "Martial Arts",
    "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural",
    "Thriller", "School Life", "Historical", "Mecha", "Seinen", "Shounen", "Shoujo", "Josei",
    "Tragedy",
];

/// Tags that mean a series is for adults.
const EXPLICIT_TAGS: [&str; 7] = [
    "adult", "hentai", "smut", "erotica", "mature", "pornographic", "explicit",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExplicitFilter {
    #[default]
    Show,
    Hide,
    Only,
}

/// What the search results are narrowed to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchFilters {
    pub types: Vec<String>,
    pub explicit: ExplicitFilter,
    pub genres: Vec<String>,
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Types and genres can only be told from the details of each series.
    pub fn needs_tags(&self) -> bool {
        !self.types.is_empty() || !self.genres.is_empty()
    }

    /// What to show on the folded pills: each choice made, or "All types" and
    /// "All genres" while there is none.
    pub fn pills(&self) -> Vec<String> {
        let mut out: Vec<String> = if self.types.is_empty() {
            vec!["All types".into()]
        } else {
            self.types.clone()
        };
        match self.explicit {
            ExplicitFilter::Show => {}
            ExplicitFilter::Hide => out.push("No explicit".into()),
            ExplicitFilter::Only => out.push("Explicit only".into()),
        }
        if self.genres.is_empty() {
            out.push("All genres".into());
        } else {
            out.extend(self.genres.iter().cloned());
        }
        out
    }

    /// Short words for each choice made, for the pills.
    pub fn labels(&self) -> Vec<String> {
        let mut out: Vec<String> = self.types.clone();
        match self.explicit {
            ExplicitFilter::Show => {}
            ExplicitFilter::Hide => out.push("No explicit".into()),
            ExplicitFilter::Only => out.push("Explicit only".into()),
        }
        out.extend(self.genres.iter().cloned());
        out
    }

    /// Does a series pass? `tags` are the genres its source lists (`None`
    /// until they are known); `adult_source` says its source is for adults.
    pub fn allows(&self, tags: Option<&[String]>, adult_source: bool) -> bool {
        let has = |tags: &[String], word: &str| tags.iter().any(|t| t.trim().eq_ignore_ascii_case(word));
        let explicit = adult_source
            || tags.is_some_and(|t| EXPLICIT_TAGS.iter().any(|w| has(t, w)));
        match self.explicit {
            ExplicitFilter::Hide if explicit => return false,
            ExplicitFilter::Only if !explicit => return false,
            _ => {}
        }
        if !self.needs_tags() {
            return true;
        }
        let Some(tags) = tags else {
            return false;
        };
        (self.types.is_empty() || self.types.iter().any(|t| has(tags, t)))
            && self.genres.iter().all(|g| has(tags, g))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(source: &str, manga_id: i64, score: f32) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: manga_id,
                    title: "Solo Leveling".into(),
                    thumbnail_url: None,
                },
            ),
            score,
            enrichment: None,
        }
    }

    #[test]
    fn a_source_listing_a_series_twice_appears_once_with_its_best_entry() {
        // MangaFire lists the series twice; the better-scored entry comes first.
        let members = vec![
            member("fire", 1, 0.9),
            member("dex", 2, 0.8),
            member("fire", 3, 0.7),
            member("pill", 4, 0.6),
        ];
        let picked = one_per_source(&members, 10);
        let ids: Vec<_> = picked
            .iter()
            .map(|m| (m.candidate.source_id.as_str(), m.candidate.manga_id))
            .collect();
        assert_eq!(ids, [("fire", 1), ("dex", 2), ("pill", 4)]);
        // The limit applies after de-duplication.
        assert_eq!(one_per_source(&members, 2).len(), 2);
        assert!(one_per_source(&[], 4).is_empty());
    }

    #[test]
    fn the_search_filters_narrow_by_type_genre_and_explicit() {
        let tags = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let none = SearchFilters::default();
        assert!(none.is_empty() && none.allows(None, false) && none.allows(None, true));

        let manhwa_action = SearchFilters {
            types: vec!["Manhwa".into()],
            genres: vec!["Action".into()],
            ..Default::default()
        };
        assert!(manhwa_action.allows(Some(&tags(&["manhwa", "ACTION", "Fantasy"])), false));
        assert!(!manhwa_action.allows(Some(&tags(&["Manhwa"])), false), "every genre is needed");
        assert!(!manhwa_action.allows(Some(&tags(&["Manga", "Action"])), false), "wrong type");
        assert!(!manhwa_action.allows(None, false), "not known yet");

        let hide = SearchFilters { explicit: ExplicitFilter::Hide, ..Default::default() };
        assert!(hide.allows(None, false));
        assert!(!hide.allows(None, true), "an adult source");
        assert!(!hide.allows(Some(&tags(&["Hentai"])), false), "an adult tag");
        let only = SearchFilters { explicit: ExplicitFilter::Only, ..Default::default() };
        assert!(only.allows(None, true) && !only.allows(Some(&tags(&["Action"])), false));
        assert_eq!(manhwa_action.labels(), ["Manhwa", "Action"]);
        assert_eq!(hide.labels(), ["No explicit"]);
        assert_eq!(none.pills(), ["All types", "All genres"]);
        assert_eq!(hide.pills(), ["All types", "No explicit", "All genres"]);
        assert_eq!(manhwa_action.pills(), ["Manhwa", "Action"]);
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use search::{Enrichment, RankedSeries, ScoredMember};

    fn member(source: &str, chapters: Option<u32>, updated: Option<i64>) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: 1,
                    title: "x".into(),
                    thumbnail_url: None,
                },
            ),
            score: 0.5,
            enrichment: Some(Enrichment {
                chapter_count: chapters,
                latest_upload_ms: updated,
            }),
        }
    }

    fn series(title: &str, members: Vec<ScoredMember>) -> RankedSeries {
        RankedSeries {
            title: title.into(),
            members,
            relevance: 1.0,
        }
    }

    fn results() -> Vec<RankedSeries> {
        vec![
            series("Bravo", vec![member("a", Some(10), Some(300))]),
            series(
                "alpha",
                vec![
                    member("a", Some(50), Some(100)),
                    member("b", Some(80), None),
                ],
            ),
            series("Charlie", vec![member("a", None, None)]),
            series("Delta", vec![member("a", Some(50), Some(900))]),
        ]
    }

    #[test]
    fn best_match_keeps_the_order_the_search_gave() {
        assert_eq!(
            sorted_order(&results(), SearchSort::BestMatch),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn most_chapters_uses_the_biggest_count_any_source_has_and_puts_unknown_last() {
        // alpha has 80 (from its second source), Delta 50, Bravo 10, Charlie unknown.
        assert_eq!(
            sorted_order(&results(), SearchSort::MostChapters),
            [1, 3, 0, 2]
        );
    }

    #[test]
    fn recently_updated_puts_the_newest_first_and_unknown_last() {
        assert_eq!(
            sorted_order(&results(), SearchSort::RecentlyUpdated),
            [3, 0, 1, 2]
        );
    }

    #[test]
    fn most_sources_breaks_ties_by_the_original_order() {
        assert_eq!(
            sorted_order(&results(), SearchSort::MostSources),
            [1, 0, 2, 3]
        );
    }

    #[test]
    fn titles_sort_without_regard_to_case() {
        assert_eq!(sorted_order(&results(), SearchSort::TitleAz), [1, 0, 2, 3]);
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in SearchSort::ALL {
            assert_eq!(SearchSort::from_key(sort.key()), sort);
        }
        assert_eq!(SearchSort::from_key("nonsense"), SearchSort::BestMatch);
        assert_eq!(
            sorted_order(&[], SearchSort::MostChapters),
            Vec::<usize>::new()
        );
    }
}
