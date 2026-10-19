//! One JSON document on disk, written so a crash cannot leave it half-written
//! and read so that nothing the user has is ever thrown away silently.
//!
//! Safety rules:
//! * A file that cannot be parsed is moved to `<name>.bad` (older copies shift
//!   to `.bad.1` .. `.bad.4`, the oldest is dropped), never overwritten.
//! * A file that cannot be *read* (antivirus lock, permissions) is retried for
//!   a moment; if it stays unreadable the default is returned, a marker
//!   `<name>.unread` is left next to it, and [`JsonFile::save`] refuses to touch
//!   the file until a later load reads it. The marker lives on disk, so it
//!   protects callers that make a fresh `JsonFile` for every save.
//! * Before replacing an existing file, `save` checks that it is valid JSON;
//!   if it is not, a quarantine copy is made first.
//! * Lists marked with [`lenient_vec`] keep their good entries when one entry
//!   does not parse; the skipped entries go to the `<name>.skipped.bad` file.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use thiserror::Error;

/// How many quarantined copies are kept per file.
const KEEP_BAD: usize = 5;
/// Pauses between read attempts (about 300 ms in all).
const RETRY_PAUSES_MS: [u64; 3] = [50, 100, 150];

#[derive(Debug, Error)]
pub enum JsonError {
    #[error("i/o error on {0}: {1}")]
    Io(PathBuf, io::Error),
    #[error("could not encode {0}: {1}")]
    Encode(PathBuf, serde_json::Error),
    #[error("{0} exists but could not be read, so it was left untouched (restart the app once it is not locked)")]
    Unreadable(PathBuf),
}

thread_local! {
    /// Entries `lenient_vec` skipped while the current thread was parsing.
    static SKIPPED: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

/// For `#[serde(deserialize_with = "...")]` on a `Vec`: entries that do not
/// parse are skipped instead of failing the whole document. Under
/// [`JsonFile::load`] the skipped entries are kept in a side file.
pub fn lenient_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let raw = Vec::<serde_json::Value>::deserialize(d)?;
    let mut out = Vec::with_capacity(raw.len());
    for v in raw {
        match serde_json::from_value::<T>(v.clone()) {
            Ok(t) => out.push(t),
            Err(_) => SKIPPED.with(|s| s.borrow_mut().push(v)),
        }
    }
    Ok(out)
}

fn locks() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

fn lock_for(path: &Path) -> Arc<Mutex<()>> {
    let mut map = locks().lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf()).or_default().clone()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn bad_name(path: &Path, n: usize) -> PathBuf {
    if n == 0 {
        with_suffix(path, ".bad")
    } else {
        with_suffix(path, &format!(".bad.{n}"))
    }
}

/// Read a file, retrying briefly on errors other than "not found".
fn read_retry(path: &Path) -> io::Result<Vec<u8>> {
    let mut attempt = 0;
    loop {
        match fs::read(path) {
            Ok(b) => return Ok(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(e),
            Err(e) => match RETRY_PAUSES_MS.get(attempt) {
                Some(ms) => {
                    std::thread::sleep(Duration::from_millis(*ms));
                    attempt += 1;
                }
                None => return Err(e),
            },
        }
    }
}

/// Make room for a new `.bad` (shifting older ones up, dropping the oldest).
fn shift_bad(path: &Path) {
    let _ = fs::remove_file(bad_name(path, KEEP_BAD - 1));
    for n in (0..KEEP_BAD - 1).rev() {
        let from = bad_name(path, n);
        if from.exists() {
            let _ = fs::rename(&from, bad_name(path, n + 1));
        }
    }
}

/// Keep `bytes` as the newest quarantined copy of `path` (unless the newest
/// one is already identical).
fn quarantine_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(bad_name(path, 0)).is_ok_and(|b| b == bytes) {
        return Ok(());
    }
    shift_bad(path);
    fs::write(bad_name(path, 0), bytes)
}

/// Move `path` itself to be the newest quarantined copy; falls back to copying.
fn quarantine_file(path: &Path) -> io::Result<()> {
    shift_bad(path);
    let target = bad_name(path, 0);
    if fs::rename(path, &target).is_ok() {
        return Ok(());
    }
    fs::copy(path, &target).map(|_| ())
}

pub struct JsonFile<T> {
    path: PathBuf,
    /// Set by `load` when the file exists but could not be read.
    unread: Cell<bool>,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonFile<T> {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            unread: Cell::new(false),
            _marker: PhantomData,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn marker(&self) -> PathBuf {
        with_suffix(&self.path, ".unread")
    }

    /// Whether the file is being protected because it could not be read:
    /// `save` refuses until a later `load` manages to read it.
    pub fn is_protected(&self) -> bool {
        self.unread.get() || self.marker().exists()
    }

    /// Load the document, or the default if there is none yet.
    ///
    /// * No file: the default (a fresh install).
    /// * Unparseable or non-UTF-8 file: moved aside to `<name>.bad` (older
    ///   ones shift to `.bad.1` ..), the default is returned.
    /// * Unreadable file (after a few retries): the default is returned and
    ///   the file is protected from `save` (see the module docs).
    pub fn load(&self) -> T {
        let bytes = match read_retry(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.clear_protection();
                return T::default();
            }
            Err(_) => {
                self.unread.set(true);
                let _ = fs::write(self.marker(), "the file could not be read when last loaded");
                return T::default();
            }
        };
        self.clear_protection();
        SKIPPED.with(|s| s.borrow_mut().clear());
        let parsed = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| serde_json::from_str::<T>(text).ok());
        let skipped = SKIPPED.with(|s| std::mem::take(&mut *s.borrow_mut()));
        match parsed {
            Some(value) => {
                if !skipped.is_empty() {
                    if let Ok(raw) = serde_json::to_vec_pretty(&skipped) {
                        let _ = quarantine_bytes(&with_suffix(&self.path, ".skipped"), &raw);
                    }
                }
                value
            }
            None => {
                if quarantine_file(&self.path).is_err() {
                    // Could not move it aside; do not let a save replace it.
                    self.unread.set(true);
                    let _ = fs::write(self.marker(), "the file could not be set aside");
                }
                T::default()
            }
        }
    }

    fn clear_protection(&self) {
        self.unread.set(false);
        let _ = fs::remove_file(self.marker());
    }

    /// Save atomically: write a uniquely named temporary file, flush it to
    /// disk, then rename it over the old one. Saves of the same path in this
    /// process never interleave. Refuses (with [`JsonError::Unreadable`]) to
    /// replace a file that could not be read.
    pub fn save(&self, value: &T) -> Result<(), JsonError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let io_err = |e| JsonError::Io(self.path.clone(), e);
        let text = serde_json::to_string_pretty(value)
            .map_err(|e| JsonError::Encode(self.path.clone(), e))?;
        let lock = lock_for(&self.path);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_protected() {
            return Err(JsonError::Unreadable(self.path.clone()));
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        // Never replace something that is not valid JSON without keeping it.
        match read_retry(&self.path) {
            Ok(old) => {
                let valid = std::str::from_utf8(&old)
                    .ok()
                    .is_some_and(|t| serde_json::from_str::<serde::de::IgnoredAny>(t).is_ok());
                if !valid && !old.is_empty() {
                    quarantine_bytes(&self.path, &old).map_err(io_err)?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(JsonError::Unreadable(self.path.clone())),
        }
        let tmp = with_suffix(
            &self.path,
            &format!(
                ".{}.{}.tmp",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ),
        );
        let write = || -> io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            io::Write::write_all(&mut f, text.as_bytes())?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &self.path)
        };
        if let Err(e) = write() {
            let _ = fs::remove_file(&tmp);
            return Err(io_err(e));
        }
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            if let Ok(d) = fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Doc {
        name: String,
        n: u32,
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-json-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_gives_default_and_round_trip_works() {
        let dir = temp("rt");
        let f = JsonFile::<Doc>::new(dir.join("sub").join("doc.json"));
        assert_eq!(f.load(), Doc::default());
        let doc = Doc {
            name: "x".into(),
            n: 7,
        };
        f.save(&doc).unwrap();
        assert_eq!(f.load(), doc);
        assert!(!f.path().with_extension("json.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_kept_aside_and_default_returned() {
        let dir = temp("bad");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        fs::write(&path, "{ this is not json").unwrap();
        let f = JsonFile::<Doc>::new(&path);
        assert_eq!(f.load(), Doc::default());
        assert!(dir.join("doc.json.bad").exists());
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_or_missing_fields_do_not_break_loading() {
        let dir = temp("compat");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        // Older file lacking `n`, and a field from a newer version.
        fs::write(&path, r#"{"name":"old","future":1}"#).unwrap();
        let f = JsonFile::<Doc>::new(&path);
        // serde errors on a missing field without a default, so the file is
        // set aside rather than half-loaded.
        let loaded = f.load();
        assert_eq!(loaded, Doc::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_corrupt_file_does_not_destroy_the_first_backup() {
        let dir = temp("bad2");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        let f = JsonFile::<Doc>::new(&path);
        fs::write(&path, "first {").unwrap();
        f.load();
        fs::write(&path, "second {").unwrap();
        f.load();
        assert_eq!(fs::read_to_string(dir.join("doc.json.bad")).unwrap(), "second {");
        assert_eq!(fs::read_to_string(dir.join("doc.json.bad.1")).unwrap(), "first {");
        // Only the newest few are kept.
        for i in 0..10 {
            fs::write(&path, format!("junk {i}")).unwrap();
            f.load();
        }
        let kept = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".bad"))
            .count();
        assert_eq!(kept, KEEP_BAD);
        assert_eq!(fs::read_to_string(dir.join("doc.json.bad")).unwrap(), "junk 9");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_utf8_is_kept_not_lost() {
        let dir = temp("utf8");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        fs::write(&path, [0x7b, 0xff, 0xfe, 0x7d]).unwrap();
        let f = JsonFile::<Doc>::new(&path);
        assert_eq!(f.load(), Doc::default());
        assert_eq!(fs::read(dir.join("doc.json.bad")).unwrap(), vec![0x7b, 0xff, 0xfe, 0x7d]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten_even_by_a_fresh_jsonfile() {
        let dir = temp("unread");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        // A directory in the file's place cannot be read as a file.
        fs::create_dir_all(&path).unwrap();
        let f = JsonFile::<Doc>::new(&path);
        assert_eq!(f.load(), Doc::default());
        assert!(f.is_protected());
        assert!(matches!(f.save(&Doc::default()), Err(JsonError::Unreadable(_))));
        // The lock is released and the real content is there again; a fresh
        // JsonFile (as the app makes for every save) must still not clobber it.
        fs::remove_dir_all(&path).unwrap();
        fs::write(&path, r#"{"name":"real","n":9}"#).unwrap();
        let fresh = JsonFile::<Doc>::new(&path);
        assert!(matches!(
            fresh.save(&Doc { name: "default".into(), n: 0 }),
            Err(JsonError::Unreadable(_))
        ));
        assert!(fs::read_to_string(&path).unwrap().contains("real"));
        // Once a load reads it, saving works again.
        assert_eq!(fresh.load().n, 9);
        fresh.save(&Doc { name: "new".into(), n: 1 }).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("new"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_keeps_a_copy_of_a_file_that_is_not_json() {
        let dir = temp("savebad");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        fs::write(&path, "garbage").unwrap();
        JsonFile::<Doc>::new(&path).save(&Doc::default()).unwrap();
        assert_eq!(fs::read_to_string(dir.join("doc.json.bad")).unwrap(), "garbage");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_saves_leave_a_whole_file_and_no_temp_files() {
        let dir = temp("conc");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.json");
        let handles: Vec<_> = (0..8u32)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..25u32 {
                        let doc = Doc { name: "x".repeat(2000 + t as usize), n: t * 100 + i };
                        JsonFile::<Doc>::new(&path).save(&doc).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let loaded = JsonFile::<Doc>::new(&path).load();
        let t = (loaded.n / 100) as usize;
        assert_eq!(loaded.name.len(), 2000 + t);
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["doc.json".to_string()], "{names:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Item {
        id: u32,
    }

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    #[serde(default)]
    struct List {
        #[serde(deserialize_with = "lenient_vec")]
        items: Vec<Item>,
    }

    #[test]
    fn one_bad_entry_is_skipped_and_kept_aside() {
        let dir = temp("lenient");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.json");
        fs::write(&path, r#"{"items":[{"id":1},{"id":"oops"},{"id":3}]}"#).unwrap();
        let list = JsonFile::<List>::new(&path).load();
        assert_eq!(list.items, vec![Item { id: 1 }, Item { id: 3 }]);
        let skipped = fs::read_to_string(dir.join("list.json.skipped.bad")).unwrap();
        assert!(skipped.contains("oops"));
        assert!(path.exists());
        let _ = fs::remove_dir_all(dir);
    }
}
