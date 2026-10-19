//! A history of the files the app has exported, so the Library page can list
//! them, open their folder, and delete them.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportedFileRecord {
    /// A path on this computer, or a URL for files sent over Wi-Fi.
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportRecord {
    pub id: String,
    pub series_id: String,
    pub title: String,
    pub chapters: usize,
    pub pages: usize,
    /// "xtc" or "xtch".
    pub format: String,
    pub files: Vec<ExportedFileRecord>,
    /// Where it was sent, in words ("folder C:\\…", "the reader").
    pub target: String,
    /// How the pages were converted, and the preset that started from, so the
    /// next export of the series can go back to the same.
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub settings: Option<crate::ConversionSettings>,
    /// The chapters (by library key) that went into it. Empty for exports
    /// made before these were kept.
    #[serde(default)]
    pub chapter_keys: Vec<String>,
    /// Seconds since the Unix epoch.
    pub at: u64,
}

const KEEP: usize = 300;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportsDb {
    /// Oldest first.
    #[serde(deserialize_with = "crate::json::lenient_vec")]
    pub records: Vec<ExportRecord>,
}

impl ExportsDb {
    /// The chapters of a series that have been exported, in any export.
    pub fn exported_keys(&self, series_id: &str) -> std::collections::HashSet<String> {
        self.records
            .iter()
            .filter(|r| r.series_id == series_id)
            .flat_map(|r| r.chapter_keys.iter().cloned())
            .collect()
    }

    pub fn add(&mut self, record: ExportRecord) {
        self.records.retain(|r| r.id != record.id);
        self.records.push(record);
        let excess = self.records.len().saturating_sub(KEEP);
        self.records.drain(..excess);
    }

    pub fn remove_record(&mut self, id: &str) {
        self.records.retain(|r| r.id != id);
    }

    /// Forget one file of a record; the record goes when its last file does.
    pub fn forget_file(&mut self, id: &str, path: &str) {
        if let Some(r) = self.records.iter_mut().find(|r| r.id == id) {
            r.files.retain(|f| f.path != path);
        }
        self.records.retain(|r| !r.files.is_empty());
    }
}

/// Delete an exported file from disk. Only `.xtc` and `.xtch` files are ever
/// deleted, because the history file could be edited by hand. Returns whether
/// a file was actually removed (a missing file is not an error).
pub fn delete_exported_file(path: &Path) -> io::Result<bool> {
    let is_ours = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xtc") || e.eq_ignore_ascii_case("xtch"));
    if !is_ours {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not an .xtc or .xtch file", path.display()),
        ));
    }
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(meta) if !meta.is_file() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not a regular file", path.display()),
        )),
        Ok(_) => fs::remove_file(path).map(|()| true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chapters_exported_for_a_series_are_known_from_all_its_exports() {
        let mut db = ExportsDb::default();
        let mut first = record("a", &["a.xtc"]);
        first.chapter_keys = vec!["c1".into(), "c2".into()];
        let mut second = record("b", &["b.xtc"]);
        second.chapter_keys = vec!["c2".into(), "c3".into()];
        let mut other = record("c", &["c.xtc"]);
        other.series_id = "other".into();
        other.chapter_keys = vec!["c9".into()];
        db.add(first);
        db.add(second);
        db.add(other);
        let keys = db.exported_keys("s");
        assert_eq!(keys.len(), 3);
        assert!(keys.contains("c1") && keys.contains("c3") && !keys.contains("c9"));
        assert!(db.exported_keys("nobody").is_empty());
    }

    #[test]
    fn old_or_partial_files_still_load_and_one_bad_record_is_skipped() {
        let dir = std::env::temp_dir().join(format!("mangapoint-exports-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("exports.json");
        fs::write(
            &path,
            r#"{"records":[{"id":"a","title":"T"},{"id":7,"files":"no"},{"id":"b","files":[{"path":"x.xtc"}]}]}"#,
        )
        .unwrap();
        let db = crate::JsonFile::<ExportsDb>::new(&path).load();
        assert_eq!(db.records.len(), 2);
        assert_eq!(db.records[1].files[0].bytes, 0);
        assert!(path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    fn record(id: &str, files: &[&str]) -> ExportRecord {
        ExportRecord {
            id: id.into(),
            series_id: "s".into(),
            title: "T".into(),
            chapters: 2,
            pages: 40,
            format: "xtch".into(),
            files: files
                .iter()
                .map(|p| ExportedFileRecord {
                    path: p.to_string(),
                    bytes: 10,
                })
                .collect(),
            target: "folder".into(),
            preset_id: None,
            settings: None,
            chapter_keys: Vec::new(),
            at: 1,
        }
    }

    #[test]
    fn records_are_added_replaced_capped_and_removed() {
        let mut db = ExportsDb::default();
        db.add(record("a", &["a.xtch"]));
        db.add(record("a", &["a2.xtch"]));
        assert_eq!(db.records.len(), 1);
        assert_eq!(db.records[0].files[0].path, "a2.xtch");
        for i in 0..400 {
            db.add(record(&format!("r{i}"), &["x.xtch"]));
        }
        assert_eq!(db.records.len(), KEEP);
        assert_eq!(db.records.last().unwrap().id, "r399");
        db.remove_record("r399");
        assert!(db.records.iter().all(|r| r.id != "r399"));
    }

    #[test]
    fn forgetting_the_last_file_forgets_the_record() {
        let mut db = ExportsDb::default();
        db.add(record("a", &["one.xtch", "two.xtch"]));
        db.forget_file("a", "one.xtch");
        assert_eq!(db.records[0].files.len(), 1);
        db.forget_file("a", "two.xtch");
        assert!(db.records.is_empty());
    }

    #[test]
    fn only_reader_files_can_be_deleted_from_the_history() {
        let dir = std::env::temp_dir().join(format!("mangapoint-exp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let book = dir.join("Book.xtch");
        let other = dir.join("passwords.txt");
        fs::write(&book, b"x").unwrap();
        fs::write(&other, b"secret").unwrap();

        assert!(delete_exported_file(&other).is_err(), "not an XTC file");
        assert!(other.exists());
        assert!(delete_exported_file(&dir).is_err(), "not a file");
        assert!(delete_exported_file(&book).unwrap());
        assert!(!book.exists());
        assert!(
            !delete_exported_file(&book).unwrap(),
            "already gone is fine"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
