//! Grouping search results that are the same series.

use crate::normalize::{normalize_title, same_series};
use catalog::{MangaSummary, Source};
use std::collections::HashMap;

/// One search result from one source.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub source_id: String,
    pub source_name: String,
    pub lang: String,
    pub manga_id: i64,
    pub title: String,
    pub thumbnail: Option<String>,
    /// Perceptual hash of the cover, when known (helps tell look-alike titles apart).
    pub cover_hash: Option<u64>,
    /// `title` after normalization.
    pub norm: String,
}

impl Candidate {
    pub fn new(source: &Source, manga: &MangaSummary) -> Self {
        Self {
            source_id: source.id.clone(),
            source_name: source.name.clone(),
            lang: source.lang.clone(),
            manga_id: manga.id,
            title: manga.title.clone(),
            thumbnail: manga.thumbnail_url.clone(),
            cover_hash: None,
            norm: normalize_title(&manga.title),
        }
    }

    /// Identifies this result across calls.
    pub fn key(&self) -> (String, i64) {
        (self.source_id.clone(), self.manga_id)
    }
}

/// Results judged to be one series.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// The title to show: the most common spelling among the members.
    pub title: String,
    pub norm: String,
    pub members: Vec<Candidate>,
}

/// Group `candidates` into series. Merging is transitive (if A matches B and
/// B matches C, all three are one series) and the output order depends only
/// on the input order, so results do not jump around between refreshes.
pub fn cluster(candidates: &[Candidate]) -> Vec<Cluster> {
    let n = candidates.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&candidates[i], &candidates[j]);
            // The same source never lists one series twice as two entries
            // that we should merge unless the titles really are equal.
            if same_series(&a.norm, &b.norm, a.cover_hash, b.cover_hash) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj.max(ri)] = rj.min(ri);
                }
            }
        }
    }

    let mut order: Vec<usize> = Vec::new();
    let mut groups: HashMap<usize, Vec<Candidate>> = HashMap::new();
    for (i, cand) in candidates.iter().enumerate() {
        let root = find(&mut parent, i);
        if !groups.contains_key(&root) {
            order.push(root);
        }
        groups.entry(root).or_default().push(cand.clone());
    }
    order
        .into_iter()
        .map(|root| {
            let members = groups.remove(&root).unwrap_or_default();
            let title = display_title(&members);
            let norm = normalize_title(&title);
            Cluster {
                title,
                norm,
                members,
            }
        })
        .collect()
}

/// The most frequent exact title; ties go to the one seen first.
fn display_title(members: &[Candidate]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for m in members {
        match counts.iter_mut().find(|(t, _)| *t == m.title) {
            Some((_, n)) => *n += 1,
            None => counts.push((&m.title, 1)),
        }
    }
    counts
        .iter()
        .fold(None::<(&str, usize)>, |best, &(t, n)| match best {
            Some((_, bn)) if bn >= n => best,
            _ => Some((t, n)),
        })
        .map(|(t, _)| t.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn cand(source: &str, id: i64, title: &str) -> Candidate {
        Candidate::new(
            &Source {
                id: source.into(),
                name: format!("Source {source}"),
                lang: "en".into(),
                is_nsfw: false,
                extension_pkg: None,
            },
            &MangaSummary {
                id,
                title: title.into(),
                thumbnail_url: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::cand;
    use super::*;

    #[test]
    fn one_series_across_sources_becomes_one_cluster() {
        let list = vec![
            cand("a", 1, "One Piece"),
            cand("b", 2, "ONE PIECE"),
            cand("c", 3, "One Piece [Official]"),
            cand("a", 4, "One Punch Man"),
        ];
        let clusters = cluster(&list);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].members.len(), 3);
        assert_eq!(clusters[1].title, "One Punch Man");
    }

    #[test]
    fn related_series_stay_separate() {
        let list = vec![
            cand("a", 1, "Naruto"),
            cand("a", 2, "Naruto Shippuden"),
            cand("b", 3, "Naruto"),
        ];
        let clusters = cluster(&list);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].members.len(), 2);
    }

    #[test]
    fn merging_is_transitive_through_a_middle_spelling() {
        // a~b and b~c by token order, so all three are one series.
        let list = vec![
            cand("a", 1, "Tower of God"),
            cand("b", 2, "God of Tower"),
            cand("c", 3, "Tower Of God (Webtoon)"),
        ];
        assert_eq!(cluster(&list).len(), 1);
    }

    #[test]
    fn display_title_is_the_most_common_spelling() {
        let list = vec![
            cand("a", 1, "ONE PIECE"),
            cand("b", 2, "One Piece"),
            cand("c", 3, "One Piece"),
        ];
        assert_eq!(cluster(&list)[0].title, "One Piece");
        // On a tie the first one seen wins.
        let tie = vec![cand("a", 1, "Berserk"), cand("b", 2, "BERSERK")];
        assert_eq!(cluster(&tie)[0].title, "Berserk");
    }

    #[test]
    fn order_follows_first_appearance_and_is_stable() {
        let list = vec![
            cand("a", 1, "Zeta"),
            cand("a", 2, "Alpha"),
            cand("b", 3, "Zeta"),
        ];
        let titles: Vec<_> = cluster(&list).into_iter().map(|c| c.title).collect();
        assert_eq!(titles, ["Zeta", "Alpha"]);
        assert_eq!(cluster(&list), cluster(&list));
    }

    #[test]
    fn empty_input_and_untitled_results_are_handled() {
        assert!(cluster(&[]).is_empty());
        // Titles that normalize to nothing never merge with each other.
        let list = vec![cand("a", 1, "!!!"), cand("b", 2, "???")];
        assert_eq!(cluster(&list).len(), 2);
    }
}
