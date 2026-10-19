//! The chapter table: sorting and filtering it, and picking chapters by text.

use super::format::format_number;
use search::MergedChapter;

/// Which column the chapter table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Number,
    Name,
    Sources,
    Date,
}

/// What clicking a column header does: sorting by a new column uses that
/// column's natural direction (newest and biggest first, names A to Z);
/// clicking the current column flips the direction.
pub fn next_sort(current: (SortKey, bool), clicked: SortKey) -> (SortKey, bool) {
    if current.0 == clicked {
        (clicked, !current.1)
    } else {
        let descending = !matches!(clicked, SortKey::Name);
        (clicked, descending)
    }
}

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

/// Indices of chapters matching text like `1-10, 15, 20-`.
///
/// `numbers` holds each chapter's number (`None` for unnumbered ones, which
/// text can never select). A bare number selects that chapter, `a-b` selects
/// the inclusive range and `a-` selects from `a` to the end.
pub fn select_by_text(text: &str, numbers: &[Option<f64>]) -> Result<Vec<usize>, String> {
    let mut wanted: Vec<(f64, f64)> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let parse = |s: &str| {
            s.trim()
                .parse::<f64>()
                .map_err(|_| format!("'{}' is not a chapter number", s.trim()))
        };
        match part.split_once('-') {
            None => {
                let n = parse(part)?;
                wanted.push((n, n));
            }
            Some((a, "")) => wanted.push((parse(a)?, f64::INFINITY)),
            Some((a, b)) => {
                let (a, b) = (parse(a)?, parse(b)?);
                if a > b {
                    return Err(format!("the range {a}-{b} runs backwards"));
                }
                wanted.push((a, b));
            }
        }
    }
    if wanted.is_empty() {
        return Err("type chapter numbers such as 1-10, 15".into());
    }
    Ok(numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let n = (*n)?;
            wanted
                .iter()
                .any(|&(a, b)| n >= a - 0.0005 && n <= b + 0.0005)
                .then_some(i)
        })
        .collect())
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

    fn nums(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|&n| Some(n)).collect()
    }

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

    #[test]
    fn clicking_a_header_picks_a_sensible_direction() {
        // New column: numbers and dates start newest/biggest first, names A to Z.
        assert_eq!(
            next_sort((SortKey::Name, false), SortKey::Number),
            (SortKey::Number, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Date),
            (SortKey::Date, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Name),
            (SortKey::Name, false)
        );
        // Same column: flip.
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Number),
            (SortKey::Number, false)
        );
        assert_eq!(
            next_sort((SortKey::Number, false), SortKey::Number),
            (SortKey::Number, true)
        );
    }

    #[test]
    fn chapter_text_selects_numbers_ranges_and_open_ends() {
        let n = nums(&[1.0, 2.0, 3.0, 4.0, 5.0, 10.0, 11.0, 12.0]);
        assert_eq!(select_by_text("2", &n).unwrap(), [1]);
        assert_eq!(select_by_text("2-4", &n).unwrap(), [1, 2, 3]);
        assert_eq!(select_by_text("1, 5, 11", &n).unwrap(), [0, 4, 6]);
        assert_eq!(select_by_text("10-", &n).unwrap(), [5, 6, 7]);
        assert_eq!(
            select_by_text("1-2, 2-3", &n).unwrap(),
            [0, 1, 2],
            "overlaps are not doubled"
        );
        assert_eq!(select_by_text("6-9", &n).unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn chapter_text_handles_halves_and_ignores_unnumbered() {
        let n = vec![Some(1.0), Some(1.5), Some(2.0), None];
        assert_eq!(select_by_text("1-2", &n).unwrap(), [0, 1, 2]);
        assert_eq!(select_by_text("1.5", &n).unwrap(), [1]);
        assert_eq!(
            select_by_text("0-", &n).unwrap(),
            [0, 1, 2],
            "unnumbered chapters are never matched"
        );
    }

    #[test]
    fn bad_chapter_text_gets_a_helpful_message() {
        let n = nums(&[1.0]);
        assert!(select_by_text("abc", &n).unwrap_err().contains("'abc'"));
        assert!(select_by_text("5-2", &n).unwrap_err().contains("backwards"));
        assert!(select_by_text("  ,  ", &n).is_err());
        assert!(select_by_text("", &n).is_err());
    }
}
