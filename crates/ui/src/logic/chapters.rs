//! The chapter table's choices: which column it is sorted by, what clicking a heading does, and
//! which chapters a typed range such as "1-20, 25, 30-" picks.

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

/// Indices of chapters matching text like `1-10, 15, 20-`.
///
/// `numbers` holds each chapter's number (`None` for unnumbered ones, which
/// text can never select). A bare number selects that chapter, `a-b` selects
/// the inclusive range and `a-` selects from `a` to the end. A part that starts with `!` (`!`, `!.`)
/// leaves out the chapters numbered with a point (12.1, 12.5: extras and notices): `1-20, !` is the
/// whole-numbered chapters of 1 to 20, and `!` alone is every whole-numbered chapter.
pub fn select_by_text(text: &str, numbers: &[Option<f64>]) -> Result<Vec<usize>, String> {
    let mut wanted: Vec<(f64, f64)> = Vec::new();
    let mut whole_only = false;
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if part.starts_with('!') {
            whole_only = true;
            continue;
        }
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
    if wanted.is_empty() && whole_only {
        wanted.push((f64::NEG_INFINITY, f64::INFINITY));
    }
    if wanted.is_empty() {
        return Err("type chapter numbers such as 1-10, 15".into());
    }
    Ok(numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let n = (*n)?;
            if whole_only && (n - n.round()).abs() >= 0.0005 {
                return None;
            }
            wanted
                .iter()
                .any(|&(a, b)| n >= a - 0.0005 && n <= b + 0.0005)
                .then_some(i)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|&n| Some(n)).collect()
    }

    #[test]
    fn an_exclamation_mark_leaves_out_the_numbers_with_a_point() {
        let numbers = vec![Some(1.0), Some(1.5), Some(2.0), Some(2.1), Some(3.0), None];
        assert_eq!(select_by_text("1-3", &numbers).unwrap(), vec![0, 1, 2, 3, 4]);
        assert_eq!(select_by_text("1-3, !", &numbers).unwrap(), vec![0, 2, 4]);
        assert_eq!(select_by_text("!.", &numbers).unwrap(), vec![0, 2, 4]);
        assert_eq!(select_by_text("2-, !1", &numbers).unwrap(), vec![2, 4]);
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

    #[test]
    fn a_range_selects_by_chapter_number() {
        let numbers = [Some(1.0), Some(2.0), Some(2.5), None, Some(10.0)];
        assert_eq!(select_by_text("1-2", &numbers).unwrap(), vec![0, 1]);
        assert_eq!(select_by_text("2.5, 10", &numbers).unwrap(), vec![2, 4]);
        assert_eq!(select_by_text("2-", &numbers).unwrap(), vec![1, 2, 4]);
    }
}
