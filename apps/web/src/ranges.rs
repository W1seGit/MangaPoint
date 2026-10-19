//! Which chapters a typed range selects ("1-20, 25, 30-"): the desktop app's own rule
//! (`logic/chapters.rs`), so both screens read a range the same way.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_selects_by_chapter_number() {
        let numbers = [Some(1.0), Some(2.0), Some(2.5), None, Some(10.0)];
        assert_eq!(select_by_text("1-2", &numbers).unwrap(), vec![0, 1]);
        assert_eq!(select_by_text("2.5, 10", &numbers).unwrap(), vec![2, 4]);
        assert_eq!(select_by_text("2-", &numbers).unwrap(), vec![1, 2, 4]);
        assert!(select_by_text("5-1", &numbers).is_err());
        assert!(select_by_text("x", &numbers).is_err());
        assert!(select_by_text("", &numbers).is_err());
    }
}
