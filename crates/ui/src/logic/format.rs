//! Numbers, dates, sizes and counts as the screens word them.

use library::Verdict;

/// `12`, `12.5`: whole numbers without a decimal point.
pub fn format_number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `2024-05-03` from milliseconds since the Unix epoch; empty if unknown.
/// (UTC; computed directly so no date library is needed.)
pub fn format_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let days = ms.div_euclid(86_400_000);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "Working",
        Verdict::NeedsBrowser => "Needs browser support",
        Verdict::NeedsFlareSolverr => "Blocked by Cloudflare",
        Verdict::Failed => "Not working",
    }
}

/// A length of time in words: "45 s", "2 min 14 s", "1 h 05 min".
pub fn human_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min {:02} s", secs / 60, secs % 60),
        _ => format!("{} h {:02} min", secs / 3600, secs % 3600 / 60),
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} bytes")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// "just now", "5 minutes ago", "3 days ago".
pub fn ago(now_secs: u64, then_secs: u64) -> String {
    let s = now_secs.saturating_sub(then_secs);
    let (n, unit) = match s {
        0..=59 => return "just now".into(),
        60..=3599 => (s / 60, "minute"),
        3600..=86_399 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// "1 chapter", "2 chapters".
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_dates_are_shown_plainly() {
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(0.01), "0.01");
        assert_eq!(format_number(3.10), "3.1");
        assert_eq!(format_date(0), "");
        assert_eq!(format_date(-5), "");
        assert_eq!(format_date(1_700_000_000_000), "2023-11-14");
        assert_eq!(format_date(951_782_400_000), "2000-02-29", "leap day");
        assert_eq!(format_date(1_000_000_000_000), "2001-09-09");
    }

    #[test]
    fn wording_helpers() {
        assert_eq!(
            verdict_label(Verdict::NeedsBrowser),
            "Needs browser support"
        );
        assert_eq!(human_bytes(999), "999 bytes");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(268_200_000), "268 MB");
        assert_eq!(human_bytes(1_200_000_000), "1.2 GB");
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1000, 940), "1 minute ago");
        assert_eq!(ago(10_000, 2_800), "2 hours ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400 * 3), "3 days ago");
        assert_eq!(
            ago(5, 50),
            "just now",
            "a clock set back is not a negative time"
        );
    }

    #[test]
    fn counts_are_worded_in_the_singular_and_plural() {
        assert_eq!(plural(0, "chapter"), "0 chapters");
        assert_eq!(plural(1, "chapter"), "1 chapter");
        assert_eq!(plural(235, "page"), "235 pages");
    }

    #[test]
    fn durations_read_in_words() {
        assert_eq!(human_duration(7), "7 s");
        assert_eq!(human_duration(134), "2 min 14 s");
        assert_eq!(human_duration(3900), "1 h 05 min");
    }
}
