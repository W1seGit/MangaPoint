//! The chapter table of a series: sorting and filtering the merged chapters of its sources. The
//! column choice and picking chapters by text are shared with the website (`mangapoint_ui`).

use mangapoint_ui::logic::format::format_number;
pub use mangapoint_ui::logic::chapters::{next_sort, select_by_text, SortKey};
use search::MergedChapter;

/// Newest upload time among the sources that have the chapter (0 if unknown).
pub fn latest_upload(chapter: &MergedChapter) -> i64 {
    chapter
        .options
        .iter()
        .map(|o| o.upload_date)
        .max()
        .unwrap_or(0)
}

/// Indices of the chapters to show: those matching every word of `filter`
/// (in the name or the number), ordered by `key`. Chapters with no value for
/// the sort column (no number, no date) always come last.
pub fn visible_chapters(
    merged: &[MergedChapter],
    filter: &str,
    key: SortKey,
    descending: bool,
) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<usize> = (0..merged.len())
        .filter(|&i| {
            let ch = &merged[i];
            let haystack = format!(
                "{} {}",
                ch.name.to_lowercase(),
                ch.number.map(format_number).unwrap_or_default()
            );
            words.iter().all(|w| haystack.contains(w.as_str()))
        })
        .collect();

    // Value to sort on; `None` sorts after everything else.
    let value = |i: usize| -> Option<f64> {
        let ch = &merged[i];
        match key {
            SortKey::Number => ch.number,
            SortKey::Sources => Some(ch.options.len() as f64),
            SortKey::Date => {
                let d = latest_upload(ch);
                (d > 0).then_some(d as f64)
            }
            SortKey::Name => Some(0.0),
        }
    };
    let (mut present, absent): (Vec<usize>, Vec<usize>) =
        shown.drain(..).partition(|&i| value(i).is_some());
    present.sort_by(|&a, &b| {
        let ord = if key == SortKey::Name {
            merged[a]
                .name
                .to_lowercase()
                .cmp(&merged[b].name.to_lowercase())
        } else {
            value(a).unwrap().total_cmp(&value(b).unwrap())
        };
        // Ties keep the chapters' own order (by number, then position).
        let ord = ord.then_with(|| {
            merged[a]
                .number
                .unwrap_or(f64::MAX)
                .total_cmp(&merged[b].number.unwrap_or(f64::MAX))
        });
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });
    present.extend(absent);
    present
}

/// Where a chapter stands in the Downloads tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterStatus {
    Saved,
    Downloading,
    Waiting,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::chapter;
    use search::merge_chapters;

    fn merged_list() -> Vec<MergedChapter> {
        let mut a = chapter(1, 1.0, 1_000_000_000_000);
        a.name = "Chapter 1: Start".into();
        let mut b = chapter(2, 2.0, 1_500_000_000_000);
        b.name = "Chapter 2: Middle".into();
        let mut c = chapter(3, 10.0, 0);
        c.name = "Chapter 10: Finale".into();
        let mut extra = chapter(4, -1.0, 1_700_000_000_000);
        extra.name = "Author's note".into();
        // A second source also has chapter 2, newer.
        let other = vec![chapter(12, 2.0, 1_600_000_000_000)];
        merge_chapters(&[vec![a, b, c, extra], other])
    }

    fn names(merged: &[MergedChapter], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| merged[i].name.clone()).collect()
    }

    #[test]
    fn the_table_sorts_by_each_column_in_both_directions() {
        let m = merged_list();
        let by = |key, desc| names(&m, &visible_chapters(&m, "", key, desc));

        // Number: numbered first; the unnumbered note is last in either direction.
        assert_eq!(
            by(SortKey::Number, false),
            [
                "Chapter 1: Start",
                "Chapter 2: Middle",
                "Chapter 10: Finale",
                "Author's note"
            ]
        );
        assert_eq!(
            by(SortKey::Number, true),
            [
                "Chapter 10: Finale",
                "Chapter 2: Middle",
                "Chapter 1: Start",
                "Author's note"
            ]
        );
        // Name: A to Z, case-insensitive.
        assert_eq!(by(SortKey::Name, false)[0], "Author's note");
        assert_eq!(by(SortKey::Name, true)[0], "Chapter 2: Middle");
        // Sources: chapter 2 has two sources.
        assert_eq!(by(SortKey::Sources, true)[0], "Chapter 2: Middle");
        // Date: newest first; chapter 10 has no date, so it is last either way.
        assert_eq!(by(SortKey::Date, true)[0], "Author's note");
        assert_eq!(
            *by(SortKey::Date, true).last().unwrap(),
            "Chapter 10: Finale"
        );
        assert_eq!(
            *by(SortKey::Date, false).last().unwrap(),
            "Chapter 10: Finale"
        );
    }

    #[test]
    fn the_filter_matches_every_word_in_names_and_numbers() {
        let m = merged_list();
        let hit = |f: &str| names(&m, &visible_chapters(&m, f, SortKey::Number, false));
        assert_eq!(hit("finale"), ["Chapter 10: Finale"]);
        assert_eq!(
            hit("CHAPTER middle"),
            ["Chapter 2: Middle"],
            "all words, any case"
        );
        assert_eq!(hit("10"), ["Chapter 10: Finale"], "by number");
        assert_eq!(hit("note"), ["Author's note"]);
        assert!(hit("nothing like this").is_empty());
        assert_eq!(hit("   ").len(), 4, "a blank filter shows everything");
    }

    #[test]
    fn a_chapter_with_several_sources_shows_its_newest_upload() {
        let m = merged_list();
        let two = m.iter().find(|c| c.name == "Chapter 2: Middle").unwrap();
        assert_eq!(latest_upload(two), 1_600_000_000_000);
        let ten = m.iter().find(|c| c.name == "Chapter 10: Finale").unwrap();
        assert_eq!(latest_upload(ten), 0);
    }
}
