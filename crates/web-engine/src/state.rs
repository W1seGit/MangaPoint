//! The reading-state file (`.mpstate`): where the visitor is in each book and how
//! they like to read, as one small JSON file the visitor keeps themselves.
//!
//! The website stores nothing on a server, so moving between devices means
//! moving this file (or an automatic copy in a folder the visitor chose). Two
//! devices that both changed can be reconciled with [`merge`].
//!
//! ```json
//! { "format": "mangapoint-state", "version": 1, "savedAt": "2026-10-19T12:00:00.000Z",
//!   "books": [ { "fingerprint": "<64 hex>", "size": 123, "fileName": "x.cbz", "title": "X",
//!                "chapters": 40,
//!                "position": { "chapter": 3, "chapterName": "Chapter 4", "page": 7, "scroll": 0.25 },
//!                "chaptersRead": [0, 1, 2],
//!                "prefs": { "mode": "Paged", "direction": "RightToLeft", "fit": "Width", "zoom": 100 },
//!                "lastReadAt": 1781000000000 } ],
//!   "settings": { "device": "xteink-x4", "preset": "manga" } }
//! ```
//!
//! The preference values are the desktop app's own ([`library::reading`]):
//! `mode` is `Paged` or `Scroll`, `direction` is `LeftToRight` or `RightToLeft`,
//! `fit` is `Width`, `Height` or `Original`, `zoom` is a percentage. A
//! preference that was never chosen is left out. `lastReadAt` is Unix
//! milliseconds.
//!
//! Reading a file never trusts it: unknown fields are ignored, wrong types fall
//! back to defaults, every number is clamped, a book without a valid
//! fingerprint is dropped, and nothing in here can panic on any input.

use crate::fingerprint::hex;
use library::reading::{ChapterRead, PageDirection, ReadFit, ReadMode, ReadProgress, ReaderPrefs};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The `format` field of the file.
pub const FORMAT: &str = "mangapoint-state";
/// The newest `version` this build reads and the one it writes.
pub const VERSION: u32 = 1;
/// The largest file that is read.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// The most books one file keeps (the ones read last win).
pub const MAX_BOOKS: usize = 5_000;
/// The most chapters a book may have.
pub const MAX_CHAPTERS: u32 = 100_000;
/// The most pages in a chapter the position may name.
pub const MAX_PAGE: u32 = 1_000_000;
/// The most characters of a name that are kept.
pub const MAX_TEXT: usize = 300;
/// Unix milliseconds of 2100-01-01: later times are not believed.
pub const MAX_TIME_MS: u64 = 4_102_444_800_000;
/// The largest file size a book may say it has (2^53, what JavaScript holds exactly).
pub const MAX_SIZE: u64 = 1 << 53;

/// Where the visitor is in a book.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    /// The chapter's index (0-based) in the book's chapter list.
    pub chapter: u32,
    pub chapter_name: String,
    /// The page in the chapter (0-based).
    pub page: u32,
    /// How far down the chapter in scroll reading, 0 to 1.
    pub scroll: f32,
}

/// How the visitor likes to read this book. A choice never made is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ReadMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<PageDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit: Option<ReadFit>,
    /// Zoom on top of the fit, as a percentage (100 = none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u16>,
}

impl Prefs {
    /// The desktop reader's preferences with the same choices.
    pub fn to_reader_prefs(self) -> ReaderPrefs {
        ReaderPrefs {
            mode: self.mode,
            direction: self.direction,
            fit: self.fit,
            zoom: self.zoom,
            hide_small_banner: false,
        }
    }

    pub fn from_reader_prefs(p: &ReaderPrefs) -> Self {
        Self {
            mode: p.mode,
            direction: p.direction,
            fit: p.fit,
            zoom: p.zoom.map(clamp_zoom),
        }
    }
}

/// One book the visitor has opened.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookState {
    /// Identity of the file: see [`crate::fingerprint`]. 64 lower-case hex digits.
    pub fingerprint: String,
    /// The file's size in bytes.
    pub size: u64,
    pub file_name: String,
    pub title: String,
    /// How many chapters the book has (0 when not known).
    pub chapters: u32,
    pub position: Position,
    /// Indices of the chapters read to the end, ascending, without repeats.
    pub chapters_read: Vec<u32>,
    pub prefs: Prefs,
    /// When the book was last read, in Unix milliseconds.
    pub last_read_at: u64,
}

/// The visitor's choices for exports and the page. `device` and `preset` are
/// known; the page may keep any other plain value (text, number, flag) beside
/// them, which is carried along untouched.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub device: String,
    pub preset: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub format: String,
    pub version: u32,
    /// When the file was written, as an ISO 8601 text (`Date.toISOString()`).
    pub saved_at: String,
    pub books: Vec<BookState>,
    pub settings: Settings,
}

impl Default for State {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            saved_at: String::new(),
            books: Vec::new(),
            settings: Settings::default(),
        }
    }
}

// ---- Reading a file ----------------------------------------------------------

/// Read a state file from bytes (UTF-8 JSON, with or without a byte order mark).
pub fn parse(bytes: &[u8]) -> Result<State, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "This file is {} MB, too big to be a reading-state file (the limit is {} MB).",
            bytes.len().div_ceil(1 << 20),
            MAX_BYTES >> 20
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "This is not a reading-state file (it is not text).".to_string())?;
    parse_str(text)
}

/// Read a state file from text.
pub fn parse_str(text: &str) -> Result<State, String> {
    if text.len() > MAX_BYTES {
        return Err(format!(
            "This file is too big to be a reading-state file (the limit is {} MB).",
            MAX_BYTES >> 20
        ));
    }
    let text = text.trim_start_matches('\u{feff}');
    let value: Value = serde_json::from_str(text)
        .map_err(|_| "This is not a reading-state file (it is not valid JSON).".to_string())?;
    from_value(&value)
}

/// Check and clean a state given as JSON. The same rules as [`parse`].
pub fn from_value(value: &Value) -> Result<State, String> {
    let root = value
        .as_object()
        .ok_or_else(|| "This is not a reading-state file (it is not a JSON object).".to_string())?;
    if root.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("This is not a MangaPoint reading-state file.".into());
    }
    let version = root.get("version").and_then(num).filter(|v| *v >= 1.0).map(|v| v as u64);
    match version {
        None => return Err("This reading-state file has no valid version number.".into()),
        Some(v) if v > u64::from(VERSION) => {
            return Err(format!(
                "This reading-state file was saved by a newer MangaPoint (file version {v}; this one reads up to {VERSION}). Open it in the newest version of the site."
            ))
        }
        Some(_) => {}
    }
    let mut state = State {
        saved_at: text_of(root.get("savedAt"), 40),
        ..State::default()
    };
    let mut books = Vec::new();
    if let Some(list) = root.get("books").and_then(Value::as_array) {
        for item in list.iter().take(MAX_BOOKS * 2) {
            if let Some(book) = clean_book(item) {
                books.push(book);
            }
        }
    }
    state.books = normalize_books(books);
    if let Some(s) = root.get("settings").and_then(Value::as_object) {
        state.settings = clean_settings(s);
    }
    Ok(state)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// A whole number held to `min..=max`; anything that is not a number is `default`.
fn int(v: Option<&Value>, min: u64, max: u64, default: u64) -> u64 {
    match v.and_then(num) {
        Some(n) if n <= min as f64 => min,
        Some(n) if n >= max as f64 => max,
        Some(n) => n as u64,
        None => default,
    }
}

fn text_of(v: Option<&Value>, max_chars: usize) -> String {
    let Some(s) = v.and_then(Value::as_str) else {
        return String::new();
    };
    s.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn clamp_zoom(z: u16) -> u16 {
    z.clamp(10, 800)
}

fn clean_book(v: &Value) -> Option<BookState> {
    let o = v.as_object()?;
    let fingerprint = o.get("fingerprint")?.as_str()?.trim().to_ascii_lowercase();
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let chapters = int(o.get("chapters"), 0, u64::from(MAX_CHAPTERS), 0) as u32;
    let pos = o.get("position").and_then(Value::as_object);
    let field = |name: &str| pos.and_then(|p| p.get(name));
    let position = Position {
        chapter: int(field("chapter"), 0, u64::from(MAX_CHAPTERS), 0) as u32,
        chapter_name: text_of(field("chapterName"), MAX_TEXT),
        page: int(field("page"), 0, u64::from(MAX_PAGE), 0) as u32,
        scroll: match field("scroll").and_then(num) {
            Some(s) => s.clamp(0.0, 1.0) as f32,
            None => 0.0,
        },
    };
    let mut read: Vec<u32> = o
        .get("chaptersRead")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .take(MAX_CHAPTERS as usize)
                .filter_map(num)
                .filter(|n| *n >= 0.0 && *n < f64::from(MAX_CHAPTERS))
                .map(|n| n as u32)
                .collect()
        })
        .unwrap_or_default();
    read.sort_unstable();
    read.dedup();
    let prefs = o.get("prefs").and_then(Value::as_object);
    let pref = |name: &str| prefs.and_then(|p| p.get(name));
    let prefs = Prefs {
        mode: pref("mode").and_then(|v| serde_json::from_value(v.clone()).ok()),
        direction: pref("direction").and_then(|v| serde_json::from_value(v.clone()).ok()),
        fit: pref("fit").and_then(|v| serde_json::from_value(v.clone()).ok()),
        zoom: pref("zoom")
            .and_then(num)
            .map(|z| clamp_zoom(z.clamp(0.0, 65_535.0) as u16)),
    };
    let mut book = BookState {
        fingerprint,
        size: int(o.get("size"), 0, MAX_SIZE, 0),
        file_name: text_of(o.get("fileName"), MAX_TEXT),
        title: text_of(o.get("title"), MAX_TEXT),
        chapters,
        position,
        chapters_read: read,
        prefs,
        last_read_at: int(o.get("lastReadAt"), 0, MAX_TIME_MS, 0),
    };
    tidy(&mut book);
    Some(book)
}

/// Make a book agree with itself: with a known chapter count, no chapter beyond it.
fn tidy(book: &mut BookState) {
    if book.chapters > 0 {
        book.chapters_read.retain(|c| *c < book.chapters);
        book.position.chapter = book.position.chapter.min(book.chapters - 1);
    }
}

fn clean_settings(o: &serde_json::Map<String, Value>) -> Settings {
    let mut s = Settings {
        device: text_of(o.get("device"), 100),
        preset: text_of(o.get("preset"), 100),
        extra: BTreeMap::new(),
    };
    for (key, value) in o {
        if key == "device" || key == "preset" || s.extra.len() >= 32 || key.chars().count() > 60 {
            continue;
        }
        let kept = match value {
            Value::Bool(_) => Some(value.clone()),
            Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => Some(value.clone()),
            Value::String(t) => Some(Value::String(
                t.chars().filter(|c| !c.is_control()).take(200).collect(),
            )),
            _ => None,
        };
        if let Some(v) = kept {
            s.extra.insert(key.clone(), v);
        }
    }
    s
}

/// One entry per fingerprint (duplicates merged as [`merge`] does), the books
/// read last first, the newest [`MAX_BOOKS`] kept.
fn normalize_books(books: Vec<BookState>) -> Vec<BookState> {
    let mut by_print: BTreeMap<String, BookState> = BTreeMap::new();
    for book in books {
        match by_print.remove(&book.fingerprint) {
            Some(old) => {
                let merged = merge_book(&old, &book);
                by_print.insert(merged.fingerprint.clone(), merged);
            }
            None => {
                by_print.insert(book.fingerprint.clone(), book);
            }
        }
    }
    let mut out: Vec<BookState> = by_print.into_values().collect();
    out.sort_by(|a, b| {
        b.last_read_at
            .cmp(&a.last_read_at)
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });
    out.truncate(MAX_BOOKS);
    out
}

// ---- Writing and merging -----------------------------------------------------

/// The file as pretty-printed JSON, ready to save.
pub fn to_json_pretty(state: &State) -> String {
    serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into())
}

/// Make a state from the page's own records: `books` as the page keeps them,
/// the visitor's `settings` and the time to put in `savedAt`. Everything is
/// cleaned as if it had been read from a file.
pub fn from_records(books: Vec<BookState>, settings: Settings, saved_at: &str) -> State {
    let state = State {
        format: FORMAT.into(),
        version: VERSION,
        saved_at: saved_at.chars().take(40).collect(),
        books,
        settings,
    };
    // Going through the reader applies every rule of a file once, in one place.
    serde_json::to_value(&state)
        .ok()
        .and_then(|v| from_value(&v).ok())
        .unwrap_or_default()
}

impl State {
    /// The book with this fingerprint.
    pub fn find(&self, fingerprint: &str) -> Option<&BookState> {
        let wanted = fingerprint.to_ascii_lowercase();
        self.books.iter().find(|b| b.fingerprint == wanted)
    }

    /// Add a book, or combine it with the one that has the same fingerprint
    /// (the more recently read wins; chapters read are united).
    pub fn upsert(&mut self, book: BookState) {
        let mut all = std::mem::take(&mut self.books);
        all.push(book);
        for b in &mut all {
            tidy(b);
        }
        self.books = normalize_books(all);
    }
}

fn merge_book(a: &BookState, b: &BookState) -> BookState {
    let a_wins = match a.last_read_at.cmp(&b.last_read_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        // The same moment: take the one that sorts last as text, so that the
        // result does not depend on which file is called a and which b.
        std::cmp::Ordering::Equal => json_key(a) >= json_key(b),
    };
    let (win, lose) = if a_wins { (a, b) } else { (b, a) };
    let mut out = win.clone();
    out.chapters_read = win
        .chapters_read
        .iter()
        .chain(&lose.chapters_read)
        .copied()
        .collect();
    out.chapters_read.sort_unstable();
    out.chapters_read.dedup();
    // A stale record that never knew the size or names must not erase them.
    if out.title.is_empty() {
        out.title = lose.title.clone();
    }
    if out.file_name.is_empty() {
        out.file_name = lose.file_name.clone();
    }
    if out.size == 0 {
        out.size = lose.size;
    }
    if out.chapters == 0 {
        out.chapters = lose.chapters;
    }
    tidy(&mut out);
    out
}

fn json_key(b: &BookState) -> String {
    serde_json::to_string(b).unwrap_or_default()
}

/// Milliseconds since the Unix epoch of an ISO 8601 text like
/// `2026-10-19T12:00:00.000Z` (an offset such as `+02:00` is honoured); 0 when it
/// cannot be read.
pub fn iso_to_ms(text: &str) -> u64 {
    fn digits(s: &str, from: usize, len: usize) -> Option<i64> {
        let part = s.get(from..from + len)?;
        if part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    }
    let parse = || -> Option<i64> {
        let t = text.trim();
        let (year, month, day) = (digits(t, 0, 4)?, digits(t, 5, 2)?, digits(t, 8, 2)?);
        if t.as_bytes().get(4) != Some(&b'-') || t.as_bytes().get(7) != Some(&b'-') {
            return None;
        }
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let (mut hour, mut min, mut sec, mut ms) = (0, 0, 0, 0);
        let mut rest = "";
        if t.len() > 10 {
            if !matches!(t.as_bytes()[10], b'T' | b't' | b' ') {
                return None;
            }
            hour = digits(t, 11, 2)?;
            min = digits(t, 14, 2)?;
            rest = t.get(16..)?;
            if let Some(r) = rest.strip_prefix(':') {
                sec = digits(r, 0, 2)?;
                rest = r.get(2..)?;
                if let Some(frac) = rest.strip_prefix('.') {
                    let n = frac.bytes().take_while(u8::is_ascii_digit).count();
                    let head: String = frac[..n].chars().chain("000".chars()).take(3).collect();
                    ms = head.parse().ok()?;
                    rest = &frac[n..];
                }
            }
        }
        if hour > 23 || min > 59 || sec > 60 {
            return None;
        }
        let offset_min = match rest {
            "" | "Z" | "z" => 0,
            r if r.len() == 6
                && matches!(r.as_bytes()[0], b'+' | b'-')
                && r.as_bytes()[3] == b':' =>
            {
                let sign = if r.starts_with('-') { -1 } else { 1 };
                sign * (digits(r, 1, 2)? * 60 + digits(r, 4, 2)?)
            }
            _ => return None,
        };
        // Days since 1970-01-01 (Howard Hinnant's algorithm).
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(((days * 24 + hour) * 60 + min - offset_min) * 60_000 + sec * 1000 + ms)
    };
    parse().filter(|ms| *ms >= 0).map_or(0, |ms| ms as u64)
}

/// Combine two states (two devices, or a file and what the page holds).
///
/// * Per book (same fingerprint) the record with the newer `lastReadAt` wins
///   whole; the chapters read are the union of both.
/// * Books present in only one state are kept.
/// * `settings` and `savedAt` come from the state saved later (`a` when equal).
/// * Books are ordered by `lastReadAt`, newest first, then by fingerprint, so
///   `merge(a, b)` and `merge(b, a)` list the same books in the same order.
pub fn merge(a: &State, b: &State) -> State {
    let b_newer = iso_to_ms(&b.saved_at) > iso_to_ms(&a.saved_at);
    let newer = if b_newer { b } else { a };
    let mut books = a.books.clone();
    books.extend(b.books.iter().cloned());
    State {
        format: FORMAT.into(),
        version: VERSION,
        saved_at: newer.saved_at.clone(),
        books: normalize_books(books),
        settings: newer.settings.clone(),
    }
}

// ---- The desktop reader's progress -------------------------------------------

impl BookState {
    /// The same place as the desktop reader keeps it ([`ReadProgress`]), with a
    /// chapter's index as its key and `lastReadAt` in seconds.
    pub fn to_progress(&self) -> ReadProgress {
        let mut p = ReadProgress {
            chapter: Some(self.position.chapter.to_string()),
            page: self.position.page as usize,
            scroll: self.position.scroll,
            last_read_at: self.last_read_at / 1000,
            prefs: self.prefs.to_reader_prefs(),
            ..ReadProgress::default()
        };
        for c in &self.chapters_read {
            p.chapters
                .insert(c.to_string(), ChapterRead { furthest: 0, read: true });
        }
        p
    }

    /// Take the place and the chapters read from a desktop-style progress whose
    /// chapter keys are indices (others are ignored).
    pub fn set_progress(&mut self, p: &ReadProgress) {
        if let Some(c) = p.chapter.as_deref().and_then(|k| k.parse::<u32>().ok()) {
            self.position.chapter = c.min(MAX_CHAPTERS);
        }
        self.position.page = (p.page as u64).min(u64::from(MAX_PAGE)) as u32;
        self.position.scroll = if p.scroll.is_finite() {
            p.scroll.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.prefs = Prefs::from_reader_prefs(&p.prefs);
        let mut read: Vec<u32> = p
            .chapters
            .iter()
            .filter(|(_, c)| c.read)
            .filter_map(|(k, _)| k.parse::<u32>().ok())
            .chain(self.chapters_read.iter().copied())
            .collect();
        read.sort_unstable();
        read.dedup();
        self.chapters_read = read;
        tidy(self);
    }
}

// ---- String in, string out (what the JavaScript names call) --------------------

/// `state_parse_json`: check a file and give it back cleaned, as pretty JSON.
pub fn parse_json(bytes: &[u8]) -> Result<String, String> {
    parse(bytes).map(|s| to_json_pretty(&s))
}

/// `state_merge_json`: merge two files (as text) and give the result as pretty JSON.
pub fn merge_json(a: &str, b: &str) -> Result<String, String> {
    let a = parse_str(a).map_err(|e| format!("First file: {e}"))?;
    let b = parse_str(b).map_err(|e| format!("Second file: {e}"))?;
    Ok(to_json_pretty(&merge(&a, &b)))
}

/// `state_to_json`: from the page's records, `{ "savedAt": "...", "books": [...],
/// "settings": {...} }` (`format` and `version` may be left out), the file to
/// save, cleaned and pretty-printed.
pub fn to_json(records: &str) -> Result<String, String> {
    let mut value: Value =
        serde_json::from_str(records).map_err(|_| "The records are not valid JSON.".to_string())?;
    if let Some(o) = value.as_object_mut() {
        o.insert("format".into(), FORMAT.into());
        o.insert("version".into(), VERSION.into());
    }
    from_value(&value).map(|s| to_json_pretty(&s))
}

/// A fingerprint-shaped text for tests and examples.
#[doc(hidden)]
pub fn sample_fingerprint(n: u8) -> String {
    hex(&[n; 32])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(n: u8, at: u64, read: &[u32]) -> BookState {
        BookState {
            fingerprint: sample_fingerprint(n),
            size: 1000,
            file_name: format!("b{n}.cbz"),
            title: format!("Book {n}"),
            chapters: 10,
            position: Position {
                chapter: 2,
                chapter_name: "Chapter 3".into(),
                page: 4,
                scroll: 0.5,
            },
            chapters_read: read.to_vec(),
            prefs: Prefs {
                mode: Some(ReadMode::Paged),
                direction: Some(PageDirection::RightToLeft),
                fit: Some(ReadFit::Width),
                zoom: Some(100),
            },
            last_read_at: at,
        }
    }

    fn state(saved: &str, books: Vec<BookState>, device: &str) -> State {
        State {
            saved_at: saved.into(),
            books,
            settings: Settings {
                device: device.into(),
                preset: "manga".into(),
                extra: BTreeMap::new(),
            },
            ..State::default()
        }
    }

    #[test]
    fn a_file_survives_writing_and_reading() {
        let s = state(
            "2026-10-19T12:00:00.000Z",
            vec![book(1, 5000, &[0, 1]), book(2, 9000, &[])],
            "xteink-x4",
        );
        let text = to_json_pretty(&s);
        assert!(text.contains("\"format\": \"mangapoint-state\""));
        assert!(text.contains("\"lastReadAt\": 9000"));
        let back = parse(text.as_bytes()).unwrap();
        assert_eq!(back.books.len(), 2);
        assert_eq!(back.books[0].title, "Book 2", "newest first");
        assert_eq!(back.settings, s.settings);
        let again = to_json_pretty(&back);
        assert_eq!(to_json_pretty(&parse(again.as_bytes()).unwrap()), again, "reading it again changes nothing");
        assert_eq!(
            back.find(&sample_fingerprint(1).to_uppercase()).unwrap().chapters_read,
            vec![0, 1]
        );
    }

    #[test]
    fn a_newer_version_is_refused_with_a_sentence_and_other_files_are_not_ours() {
        let e = parse_str(r#"{"format":"mangapoint-state","version":2,"books":[]}"#).unwrap_err();
        assert!(e.contains("newer MangaPoint") && e.contains("version 2"), "{e}");
        assert!(parse_str(r#"{"format":"other","version":1}"#)
            .unwrap_err()
            .contains("not a MangaPoint"));
        assert!(parse_str(r#"{"format":"mangapoint-state"}"#).unwrap_err().contains("version"));
        assert!(parse_str(r#"{"format":"mangapoint-state","version":0}"#).is_err());
        assert!(parse_str("[1,2]").unwrap_err().contains("JSON object"));
        assert!(parse_str("{oops").unwrap_err().contains("JSON"));
        assert!(parse(&[0xff, 0xfe, 0x00]).is_err());
        let ok = parse_str("\u{feff}{\"format\":\"mangapoint-state\",\"version\":1}").unwrap();
        assert!(ok.books.is_empty());
    }

    #[test]
    fn a_file_over_five_megabytes_is_refused_before_it_is_read() {
        let big = vec![b' '; MAX_BYTES + 1];
        let e = parse(&big).unwrap_err();
        assert!(e.contains("too big"), "{e}");
        assert!(parse(&big[..MAX_BYTES]).is_err(), "all spaces is not a file either");
    }

    #[test]
    fn unknown_fields_are_ignored_and_every_number_is_held_to_its_range() {
        let fp = sample_fingerprint(7);
        let text = format!(
            r#"{{"format":"mangapoint-state","version":1,"future":{{"x":1}},"books":[
              {{"fingerprint":"{fp}","size":-5,"fileName":"a\u0000b.cbz","title":"T","chapters":1e30,"extra":true,
                "position":{{"chapter":99999999999,"chapterName":5,"page":-3,"scroll":7.5}},
                "chaptersRead":[3,1,1,-2,"x",1e99,2.9,null],
                "prefs":{{"mode":"Sideways","direction":"LeftToRight","fit":42,"zoom":1e9}},
                "lastReadAt":1e30}},
              {{"fingerprint":"nothex","title":"dropped"}},
              {{"title":"no fingerprint"}}, 5, null, "x"]}}"#
        );
        let s = parse_str(&text).unwrap();
        assert_eq!(s.books.len(), 1);
        let b = &s.books[0];
        assert_eq!((b.size, b.chapters), (0, MAX_CHAPTERS));
        assert_eq!(b.file_name, "ab.cbz", "control characters are dropped");
        assert_eq!(b.position.chapter, MAX_CHAPTERS - 1, "the position stays inside the chapters");
        assert_eq!((b.position.page, b.position.scroll), (0, 1.0));
        assert_eq!(b.position.chapter_name, "");
        assert_eq!(b.chapters_read, vec![1, 2, 3]);
        assert_eq!(b.prefs.mode, None, "an unknown mode is a choice never made");
        assert_eq!(b.prefs.direction, Some(PageDirection::LeftToRight));
        assert_eq!(b.prefs.fit, None);
        assert_eq!(b.prefs.zoom, Some(800));
        assert_eq!(b.last_read_at, MAX_TIME_MS);
    }

    #[test]
    fn a_chapter_beyond_the_count_is_not_kept() {
        let mut b = book(1, 1, &[0, 5, 9, 10, 11]);
        b.chapters = 10;
        b.position.chapter = 50;
        let s = from_records(vec![b], Settings::default(), "2026-10-19T12:00:00Z");
        assert_eq!(s.books[0].chapters_read, vec![0, 5, 9]);
        assert_eq!(s.books[0].position.chapter, 9);
    }

    #[test]
    fn hostile_input_never_panics() {
        let samples = [
            "",
            "null",
            "{}",
            "[]",
            "\"x\"",
            "{\"format\":\"mangapoint-state\",\"version\":1,\"books\":\"x\",\"settings\":[1]}",
            "{\"format\":\"mangapoint-state\",\"version\":1,\"books\":[[],{},{\"fingerprint\":[1]}],\"settings\":{\"a\":[1],\"b\":{}}}",
            "{\"format\":\"mangapoint-state\",\"version\":1e400}",
            "{\"format\":\"mangapoint-state\",\"version\":\"1\"}",
            "{\"format\":\"mangapoint-state\",\"version\":1,\"savedAt\":\"\u{1F600}\u{1F600}\"}",
            "\u{0}\u{0}\u{0}",
        ];
        for s in samples {
            let _ = parse_str(s);
            let _ = merge_json(s, s);
            let _ = to_json(s);
        }
        // Deeply nested JSON is refused by the JSON reader, not by a stack overflow.
        let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
        assert!(parse_str(&deep).is_err());
        // Random-looking bytes.
        let mut x = 12345u32;
        for len in [1usize, 7, 64, 500] {
            let junk: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                    (x >> 24) as u8
                })
                .collect();
            let _ = parse(&junk);
        }
        // Odd dates cannot panic either.
        for d in ["2026-06-09T1", "2026-06-09T10:1", "\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}", "2026-06-09T10:13:20.", "9999-12-31T23:59:59.999Z"] {
            let _ = iso_to_ms(d);
        }
    }

    #[test]
    fn settings_keep_plain_extra_values_only() {
        let s = parse_str(
            r#"{"format":"mangapoint-state","version":1,"settings":{"device":"d","preset":"manhwa","theme":"dark","n":3,"on":true,"obj":{"a":1},"list":[1],"nothing":null}}"#,
        )
        .unwrap();
        assert_eq!(s.settings.device, "d");
        assert_eq!(s.settings.extra.len(), 3);
        assert_eq!(s.settings.extra["theme"], "dark");
        let text = to_json_pretty(&s);
        assert!(text.contains("\"theme\": \"dark\""));
        assert_eq!(parse_str(&text).unwrap(), s);
    }

    #[test]
    fn merging_takes_the_newest_record_unions_the_chapters_read_and_keeps_both_sides_books() {
        let mut a1 = book(1, 5000, &[0, 1]);
        a1.position.page = 1;
        let mut b1 = book(1, 9000, &[1, 2]);
        b1.position.page = 8;
        let a = state("2026-10-19T10:00:00.000Z", vec![a1, book(2, 100, &[])], "device-a");
        let b = state("2026-10-19T11:00:00.000Z", vec![b1, book(3, 700, &[4])], "device-b");
        let m = merge(&a, &b);
        assert_eq!(
            m.books.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(),
            ["Book 1", "Book 3", "Book 2"]
        );
        assert_eq!(m.books[0].position.page, 8, "the newer record wins");
        assert_eq!(m.books[0].chapters_read, vec![0, 1, 2], "union");
        assert_eq!(m.settings.device, "device-b", "settings from the later file");
        assert_eq!(m.saved_at, "2026-10-19T11:00:00.000Z");
        // Order of arguments changes nothing (including ties).
        assert_eq!(merge(&b, &a), m);
        let t1 = state("2026-10-19T11:00:00Z", vec![book(1, 50, &[1])], "x");
        let mut other = book(1, 50, &[2]);
        other.position.page = 99;
        let t2 = state("2026-10-19T11:00:00Z", vec![other], "x");
        assert_eq!(merge(&t1, &t2).books, merge(&t2, &t1).books);
        assert_eq!(merge(&t1, &t2).books[0].chapters_read, vec![1, 2]);
    }

    #[test]
    fn merging_files_as_text_names_the_bad_one() {
        let good = to_json_pretty(&State::default());
        assert!(merge_json(&good, &good).is_ok());
        assert!(merge_json("nope", &good).unwrap_err().starts_with("First file:"));
        assert!(merge_json(&good, "nope").unwrap_err().starts_with("Second file:"));
    }

    #[test]
    fn records_from_the_page_become_a_clean_file() {
        let fp = sample_fingerprint(9);
        let text = to_json(&format!(
            r#"{{"savedAt":"2026-10-19T12:00:00.000Z","books":[{{"fingerprint":"{fp}","title":"X","lastReadAt":5,"position":{{"page":3}}}}],"settings":{{"device":"kobo-libra-2"}}}}"#
        ))
        .unwrap();
        let s = parse_str(&text).unwrap();
        assert_eq!((s.format.as_str(), s.version), (FORMAT, VERSION));
        assert_eq!(s.books[0].position.page, 3);
        assert!(to_json("[").is_err());
        let again = from_records(s.books.clone(), s.settings.clone(), &s.saved_at);
        assert_eq!(again, s);
    }

    #[test]
    fn upsert_combines_by_fingerprint() {
        let mut s = State::default();
        s.upsert(book(1, 10, &[0]));
        s.upsert(book(1, 20, &[3]));
        s.upsert(book(2, 5, &[]));
        assert_eq!(s.books.len(), 2);
        assert_eq!(s.find(&sample_fingerprint(1)).unwrap().last_read_at, 20);
        assert_eq!(s.find(&sample_fingerprint(1)).unwrap().chapters_read, vec![0, 3]);
        assert!(s.find("nope").is_none());
    }

    #[test]
    fn dates_are_read_the_way_javascript_writes_them() {
        assert_eq!(iso_to_ms("1970-01-01T00:00:00.000Z"), 0);
        assert_eq!(iso_to_ms("1970-01-02T00:00:00Z"), 86_400_000);
        assert_eq!(iso_to_ms("2026-06-09T10:13:20.250Z"), 1_781_000_000_250);
        assert_eq!(iso_to_ms("2026-06-09T12:13:20+02:00"), 1_781_000_000_000);
        assert_eq!(iso_to_ms("2026-06-09"), 1_780_963_200_000);
        for bad in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",
            "2026-06-09T25:00:00Z",
            "2026-06-09T10:13:20X",
        ] {
            assert_eq!(iso_to_ms(bad), 0, "{bad}");
        }
    }

    #[test]
    fn it_converts_to_and_from_the_desktop_readers_progress() {
        let b = book(1, 7_000, &[0, 2]);
        let p = b.to_progress();
        assert!(p.is_read("0") && p.is_read("2") && !p.is_read("1"));
        assert_eq!((p.chapter.as_deref(), p.page, p.last_read_at), (Some("2"), 4, 7));
        assert_eq!(p.prefs.fit, Some(ReadFit::Width));
        let mut q = BookState { chapters: 10, ..BookState::default() };
        q.set_progress(&p);
        assert_eq!(
            (q.position.chapter, q.position.page, q.chapters_read.clone()),
            (2, 4, vec![0, 2])
        );
        assert_eq!(q.prefs, b.prefs);
    }
}
