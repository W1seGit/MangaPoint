//! What the app's chapter table (`ui/chapter_table.rs`, shared with the desktop app) needs from
//! its `logic`: the columns it sorts by, and what clicking a heading does. The same as the app's
//! `logic/chapters.rs`, which also handles download sources and so cannot be built for the web.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_column_sorts_its_own_way_and_the_same_one_flips() {
        assert_eq!(next_sort((SortKey::Number, true), SortKey::Name), (SortKey::Name, false));
        assert_eq!(next_sort((SortKey::Name, false), SortKey::Name), (SortKey::Name, true));
        assert_eq!(next_sort((SortKey::Name, false), SortKey::Number), (SortKey::Number, true));
    }
}
