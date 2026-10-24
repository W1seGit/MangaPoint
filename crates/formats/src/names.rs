//! What file and folder names say: the order pages and chapters go in, which
//! chapter a name is, and what a series is called.

use std::cmp::Ordering;

/// Compare names the way a person sorts them: a run of digits by its value
/// ("2" before "10", "007" with "7"), anything else without regard to case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (na, nb) = (digits(&mut a), digits(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let order = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (lower(x), lower(y));
                if lx != ly {
                    return lx.cmp(&ly);
                }
                a.next();
                b.next();
            }
        }
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut out = String::new();
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_digit() {
            break;
        }
        out.push(c);
        chars.next();
    }
    out
}

/// Words that introduce a chapter number, longest first so "chapter" is not
/// read as "c".
const CHAPTER_WORDS: [&str; 7] = ["chapter", "episode", "chap", "ep", "ch", "c", "#"];

/// Words that introduce a volume, whose number is not a chapter's.
const VOLUME_WORDS: [&str; 3] = ["volume", "vol", "v"];

/// The text of a file or folder name without its extension.
pub fn stem(name: &str) -> &str {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match name.rfind('.') {
        // An extension is letters and digits with at least one letter: ".cbz",
        // ".mp4", but not the ".5" of "ch 3.5".
        Some(dot)
            if dot > 0
                && name.len() - dot <= 6
                && name[dot + 1..].chars().all(|c| c.is_ascii_alphanumeric())
                && name[dot + 1..].chars().any(|c| c.is_ascii_alphabetic()) =>
        {
            &name[..dot]
        }
        _ => name,
    }
}

/// The chapter a name says it is: "Chapter 12", "ch.3.5", "c012", "#7", "第12話",
/// or a name that is only a number ("012", "12.5"). Failing those, a number
/// at the end ("One Piece 1050") or at the start ("01 - The Beginning"), but
/// never a year or a volume. `None` when it says none.
pub fn chapter_number(name: &str) -> Option<f64> {
    let text = stem(name).to_lowercase();
    keyword_number(&text)
        .or_else(|| cjk_number(&text))
        .or_else(|| text.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0))
        .or_else(|| loose_number(&text))
}

/// A number after a chapter word, which must start a word of its own.
fn keyword_number(text: &str) -> Option<f64> {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if i > 0 && chars[i - 1].is_alphabetic() {
            continue;
        }
        let rest: String = chars[i..].iter().collect();
        for word in CHAPTER_WORDS {
            let Some(after) = rest.strip_prefix(word) else {
                continue;
            };
            let after = after.trim_start_matches([' ', '.', ':', '_', '-', '#']);
            if let Some(n) = leading_number(after) {
                return Some(n);
            }
        }
    }
    None
}

/// 第12話, 第12章, 12話, 12话 (Japanese and Chinese chapter words).
fn cjk_number(text: &str) -> Option<f64> {
    for marker in ['話', '话', '章'] {
        if let Some(at) = text.find(marker) {
            let before = &text[..at];
            let digits: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(n) = digits.parse::<f64>() {
                return Some(n);
            }
        }
    }
    None
}

/// The number a text begins with: "12", "12.5", "012".
fn leading_number(text: &str) -> Option<f64> {
    let mut end = 0;
    let mut seen_dot = false;
    let bytes = text.as_bytes();
    while end < bytes.len() {
        match bytes[end] {
            b'0'..=b'9' => end += 1,
            b'.' if !seen_dot && end > 0 && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) => {
                seen_dot = true;
                end += 1;
            }
            _ => break,
        }
    }
    if end == 0 {
        return None;
    }
    text[..end].parse().ok()
}

/// The last number in a name, or failing that the first, unless it follows a
/// volume word or looks like a year.
fn loose_number(text: &str) -> Option<f64> {
    let mut found: Vec<(usize, f64)> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_alphanumeric() || chars[i - 1].is_ascii_digit()) {
            let rest: String = chars[i..].iter().collect();
            if let Some(n) = leading_number(&rest) {
                let len = format!("{n}").len().max(1);
                let word: String = chars[..i]
                    .iter()
                    .rev()
                    .skip_while(|c| matches!(c, ' ' | '.' | '_' | '-'))
                    .take_while(|c| c.is_alphabetic())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let is_volume = VOLUME_WORDS.contains(&word.as_str());
                let is_year = (1900.0..=2100.0).contains(&n) && n.fract() == 0.0 && len == 4;
                if !is_volume && !is_year {
                    found.push((i, n));
                }
                // Skip the digits just read.
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                continue;
            }
        }
        i += 1;
    }
    found.last().or(found.first()).map(|(_, n)| *n)
}

/// A name made readable: no extension, underscores as spaces, no [group] or
/// (year) tags, spaces squeezed.
pub fn clean_name(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in stem(name).chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '_' => out.push(' '),
            c => out.push(c),
        }
    }
    let squeezed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = squeezed.trim_matches(|c: char| c == '-' || c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        stem(name).trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// What a series is called by the name of its file or folder: the cleaned
/// name without its chapter or volume part ("One Piece - c1050" is "One Piece").
pub fn series_title(name: &str) -> String {
    let cleaned = clean_name(name);
    // ASCII-only lowercasing keeps every byte offset the same as in `cleaned`
    // (a full lowercase turns 'İ' into two characters and shifts them).
    let lower = cleaned.to_ascii_lowercase();
    let chars: Vec<(usize, char)> = lower.char_indices().collect();
    for (k, &(i, _)) in chars.iter().enumerate() {
        if k > 0 && chars[k - 1].1.is_alphabetic() {
            continue;
        }
        for word in CHAPTER_WORDS.iter().chain(VOLUME_WORDS.iter()) {
            let Some(after) = lower[i..].strip_prefix(word) else {
                continue;
            };
            let after = after.trim_start_matches([' ', '.', ':', '_', '-', '#']);
            if after.starts_with(|c: char| c.is_ascii_digit()) && i > 0 {
                let head = cleaned[..i].trim_end_matches(|c: char| c == '-' || c == ':' || c.is_whitespace());
                if !head.is_empty() {
                    return head.to_string();
                }
            }
        }
    }
    // A trailing number with nothing but a series name before it.
    if let Some(n) = loose_number(&lower) {
        let digits = format!("{n}");
        if let Some(at) = cleaned.rfind(&digits) {
            let head = cleaned[..at].trim_end_matches(|c: char| c == '-' || c == '#' || c.is_whitespace());
            if !head.is_empty() && at + digits.len() == cleaned.len() {
                return head.to_string();
            }
        }
    }
    cleaned
}

/// What a chapter is called: its cleaned name, or "Chapter 12" when the name
/// is only the number (or there is no name).
pub fn chapter_name(name: &str, number: Option<f64>) -> String {
    let cleaned = clean_name(name);
    let only_number = cleaned.trim().parse::<f64>().is_ok();
    match number {
        Some(n) if cleaned.is_empty() || only_number => format!("Chapter {}", plain(n)),
        _ if cleaned.is_empty() => "Chapter".to_string(),
        _ => cleaned,
    }
}

/// A chapter number as it reads: "12", "3.5".
pub fn plain(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_the_way_a_person_counts() {
        let mut names = vec!["10.jpg", "2.jpg", "1.jpg", "007.jpg", "a 3", "A 20", "a 100"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["1.jpg", "2.jpg", "007.jpg", "10.jpg", "a 3", "A 20", "a 100"]);
        assert_eq!(natural_cmp("p07.png", "p7.png"), Ordering::Equal);
        assert_eq!(natural_cmp("Chapter 2/1.png", "chapter 10/1.png"), Ordering::Less);
    }

    #[test]
    fn a_chapter_is_read_from_the_words_in_its_name() {
        for (name, want) in [
            ("Chapter 12", Some(12.0)),
            ("One Piece - Chapter 1050.cbz", Some(1050.0)),
            ("ch.3.5", Some(3.5)),
            ("c012", Some(12.0)),
            ("[Group] Berserk c041 (v12).cbz", Some(41.0)),
            ("#7", Some(7.0)),
            ("Episode 4: Return", Some(4.0)),
            ("第12話", Some(12.0)),
            ("12章", Some(12.0)),
            ("012", Some(12.0)),
            ("12.5", Some(12.5)),
            ("One Piece 1050.cbz", Some(1050.0)),
            ("01 - The Beginning", Some(1.0)),
            ("Vol 3", None),
            ("Vol. 03 Special", None),
            ("Church", None),
            ("Title (2019)", None),
            ("cover", None),
        ] {
            assert_eq!(chapter_number(name), want, "{name}");
        }
    }

    #[test]
    fn a_series_is_named_by_what_comes_before_its_chapter() {
        assert_eq!(series_title("One Piece - c1050.cbz"), "One Piece");
        assert_eq!(series_title("[Group] Berserk_ch_41 (Digital).cbz"), "Berserk");
        assert_eq!(series_title("Solo Leveling Vol 2"), "Solo Leveling");
        assert_eq!(series_title("Naruto 700.cbz"), "Naruto");
        assert_eq!(series_title("My Own Comic.pdf"), "My Own Comic");
        assert_eq!(series_title("2001 Nights"), "2001 Nights");
        assert_eq!(clean_name("[Scans] Some_Title (2020).zip"), "Some Title");
    }

    #[test]
    fn a_chapter_is_called_by_its_name_or_its_number() {
        assert_eq!(chapter_name("012", Some(12.0)), "Chapter 12");
        assert_eq!(chapter_name("", Some(3.5)), "Chapter 3.5");
        assert_eq!(chapter_name("The Fall", Some(4.0)), "The Fall");
        assert_eq!(chapter_name("", None), "Chapter");
        assert_eq!(plain(7.0), "7");
    }
}
