//! The import review: what a draft book looks like while the visitor checks
//! it, and the edits they can make (title, chapter names and numbers, leaving a
//! chapter out). Plain data, tested natively.

use crate::model::Chapter;
use serde::Deserialize;

/// What the browser found in one picked file, folder or set of pictures.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Draft {
    pub draft_id: String,
    pub title: String,
    pub file_name: String,
    pub kind: String,
    pub size: u64,
    pub fingerprint: String,
    pub page_count: usize,
    pub chapters: Vec<Chapter>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub aspect: Option<f32>,
    #[serde(default)]
    pub rtl: Option<bool>,
    /// The same book is already kept here (`stored` says where).
    #[serde(default)]
    pub existing: Option<Existing>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Existing {
    pub stored: String,
}

/// A file that could not be turned into a book, and why, in plain words.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Failure {
    pub name: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewChapter {
    pub included: bool,
    pub name: String,
    /// What is in the number box; empty means "no number".
    pub number: String,
    pub first: usize,
    pub count: usize,
}

/// A draft with the visitor's edits.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub draft: Draft,
    pub title: String,
    pub chapters: Vec<ReviewChapter>,
    /// How many chapters the book they are added to has already (left unticked).
    pub duplicates: usize,
}

/// A chapter number as the box shows it.
pub fn number_text(n: Option<f64>) -> String {
    match n {
        None => String::new(),
        Some(n) if n.fract().abs() < 0.0005 => format!("{}", n.round() as i64),
        Some(n) => format!("{n:.3}").trim_end_matches('0').trim_end_matches('.').to_string(),
    }
}

/// What a number box means. `Err` for text that is not a number (the box is
/// left as typed and marked); an empty box is "no number".
/// A number box holding something that is not a chapter number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotANumber;

pub fn parse_number(text: &str) -> Result<Option<f64>, NotANumber> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<f64>() {
        Ok(n) if n.is_finite() && (0.0..100_000.0).contains(&n) => Ok(Some(n)),
        _ => Err(NotANumber),
    }
}

impl Review {
    pub fn new(draft: Draft) -> Self {
        let chapters = draft
            .chapters
            .iter()
            .map(|c| ReviewChapter {
                included: true,
                name: c.name.clone(),
                number: number_text(c.number),
                first: c.first,
                count: c.count,
            })
            .collect();
        Self { title: draft.title.clone(), draft, chapters, duplicates: 0 }
    }

    /// Untick the chapters a book already has (the same number, or for a chapter with no number
    /// the same name), and count them, so adding them again is a choice the visitor makes.
    pub fn leave_out_duplicates(&mut self, existing: &[Chapter]) {
        let numbers: Vec<String> = existing.iter().filter_map(|c| c.number).map(|n| number_text(Some(n))).collect();
        let names: Vec<String> = existing.iter().map(|c| c.name.trim().to_lowercase()).collect();
        let mut found = 0;
        for c in &mut self.chapters {
            let same = if c.number.trim().is_empty() {
                !c.name.trim().is_empty() && names.contains(&c.name.trim().to_lowercase())
            } else {
                numbers.contains(&c.number.trim().to_string())
            };
            if same {
                c.included = false;
                found += 1;
            }
        }
        self.duplicates = found;
    }

    pub fn set_name(&mut self, i: usize, name: &str) {
        if let Some(c) = self.chapters.get_mut(i) {
            c.name = name.chars().take(200).collect();
        }
    }

    pub fn set_number(&mut self, i: usize, text: &str) {
        if let Some(c) = self.chapters.get_mut(i) {
            c.number = text.chars().take(12).collect();
        }
    }

    pub fn number_is_valid(&self, i: usize) -> bool {
        self.chapters.get(i).is_some_and(|c| parse_number(&c.number).is_ok())
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(c) = self.chapters.get_mut(i) {
            c.included = !c.included;
        }
    }

    pub fn set_all(&mut self, on: bool) {
        for c in &mut self.chapters {
            c.included = on;
        }
    }

    pub fn included_count(&self) -> usize {
        self.chapters.iter().filter(|c| c.included).count()
    }

    pub fn included_pages(&self) -> usize {
        self.chapters.iter().filter(|c| c.included).map(|c| c.count).sum()
    }

    /// Why the book cannot be added yet, in a few words.
    pub fn problem(&self) -> Option<&'static str> {
        if self.title.trim().is_empty() {
            return Some("Give the book a title.");
        }
        if self.included_count() == 0 {
            return Some("Keep at least one chapter.");
        }
        if (0..self.chapters.len()).any(|i| self.chapters[i].included && !self.number_is_valid(i)) {
            return Some("A chapter number is not a number.");
        }
        None
    }

    /// The chapters to keep, as the book will list them. A chapter with no name
    /// gets "Chapter N" (its number, or its place).
    pub fn final_chapters(&self) -> Vec<Chapter> {
        let mut out = Vec::new();
        for c in self.chapters.iter().filter(|c| c.included) {
            let number = parse_number(&c.number).ok().flatten();
            let name = c.name.trim();
            let name = if name.is_empty() {
                match number {
                    Some(n) => format!("Chapter {}", number_text(Some(n))),
                    None => format!("Chapter {}", out.len() + 1),
                }
            } else {
                name.to_string()
            };
            out.push(Chapter { name, number, first: c.first, count: c.count });
        }
        out
    }

    /// The pages not in any kept chapter (they stay in the file but cannot be read).
    pub fn left_out_pages(&self) -> usize {
        self.draft.page_count - self.included_pages().min(self.draft.page_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Draft {
        Draft {
            draft_id: "d1".into(),
            title: "Hero Saga".into(),
            file_name: "hero.cbz".into(),
            kind: "zip".into(),
            size: 10,
            fingerprint: "ab".repeat(32),
            page_count: 9,
            chapters: vec![
                Chapter { name: "Chapter 1".into(), number: Some(1.0), first: 0, count: 4 },
                Chapter { name: "Chapter 2.5".into(), number: Some(2.5), first: 4, count: 3 },
                Chapter { name: "Extra".into(), number: None, first: 7, count: 2 },
            ],
            warnings: vec![],
            cover_url: None,
            aspect: None,
            rtl: None,
            existing: None,
        }
    }

    #[test]
    fn chapters_a_book_has_already_are_left_unticked() {
        let mut r = Review::new(draft());
        let existing = vec![
            Chapter { name: "Chapter 1".into(), number: Some(1.0), first: 0, count: 3 },
            Chapter { name: "extra".into(), number: None, first: 3, count: 2 },
        ];
        r.leave_out_duplicates(&existing);
        assert_eq!(r.duplicates, 2);
        assert!(!r.chapters[0].included, "number 1 is there already");
        assert!(r.chapters[1].included, "2.5 is new");
        assert!(!r.chapters[2].included, "a chapter with no number and the same name");
        r.toggle(0);
        assert!(r.chapters[0].included, "and can still be added again");
    }

    #[test]
    fn numbers_read_and_write_like_the_desktop() {
        assert_eq!(number_text(Some(1.0)), "1");
        assert_eq!(number_text(Some(2.5)), "2.5");
        assert_eq!(number_text(Some(2.125)), "2.125");
        assert_eq!(number_text(None), "");
        assert_eq!(parse_number("12"), Ok(Some(12.0)));
        assert_eq!(parse_number(" 3,5 "), Ok(Some(3.5)));
        assert_eq!(parse_number(""), Ok(None));
        assert_eq!(parse_number("abc"), Err(NotANumber));
        assert_eq!(parse_number("-1"), Err(NotANumber));
        assert_eq!(parse_number("NaN"), Err(NotANumber));
        assert_eq!(parse_number("1e99"), Err(NotANumber));
    }

    #[test]
    fn a_fresh_review_keeps_everything_as_guessed() {
        let r = Review::new(draft());
        assert_eq!(r.included_count(), 3);
        assert_eq!(r.included_pages(), 9);
        assert_eq!(r.problem(), None);
        assert_eq!(r.final_chapters(), draft().chapters);
        assert_eq!(r.left_out_pages(), 0);
    }

    #[test]
    fn edits_change_the_chapters_that_are_kept() {
        let mut r = Review::new(draft());
        r.set_name(0, "  Prologue ");
        r.set_number(0, "0");
        r.set_name(1, "");
        r.set_number(1, "7");
        r.toggle(2);
        let chapters = r.final_chapters();
        assert_eq!(chapters.len(), 2);
        assert_eq!((chapters[0].name.as_str(), chapters[0].number), ("Prologue", Some(0.0)));
        assert_eq!((chapters[1].name.as_str(), chapters[1].number), ("Chapter 7", Some(7.0)));
        assert_eq!((chapters[1].first, chapters[1].count), (4, 3));
        assert_eq!(r.left_out_pages(), 2);
    }

    #[test]
    fn a_chapter_without_name_or_number_gets_its_place() {
        let mut r = Review::new(draft());
        r.toggle(0);
        r.set_name(2, "");
        r.set_number(2, "");
        assert_eq!(r.final_chapters()[1].name, "Chapter 2");
        assert_eq!(r.final_chapters()[1].number, None);
    }

    #[test]
    fn problems_stop_the_add() {
        let mut r = Review::new(draft());
        r.title = "   ".into();
        assert_eq!(r.problem(), Some("Give the book a title."));
        r.title = "X".into();
        r.set_number(1, "two");
        assert_eq!(r.problem(), Some("A chapter number is not a number."));
        assert!(!r.number_is_valid(1));
        r.toggle(1); // a left-out chapter's number does not matter
        assert_eq!(r.problem(), None);
        r.set_all(false);
        assert_eq!(r.problem(), Some("Keep at least one chapter."));
        r.set_all(true);
        assert_eq!(r.included_count(), 3);
    }

    #[test]
    fn out_of_range_edits_do_nothing() {
        let mut r = Review::new(draft());
        r.set_name(99, "x");
        r.set_number(99, "1");
        r.toggle(99);
        assert!(!r.number_is_valid(99));
        assert_eq!(r, Review::new(draft()));
    }

    #[test]
    fn the_engines_answer_deserializes() {
        let d: Draft = serde_json::from_str(
            r#"{"draft_id":"d3","title":"T","file_name":"t.cbz","kind":"zip","size":5,"fingerprint":"ff","page_count":2,
                "chapters":[{"name":"Chapter 1","number":null,"first":0,"count":2}],"cover_url":null,"existing":{"stored":"none"}}"#,
        )
        .unwrap();
        assert_eq!(d.chapters[0].number, None);
        assert_eq!(d.existing.unwrap().stored, "none");
    }
}
