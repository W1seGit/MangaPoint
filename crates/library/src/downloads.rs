//! Chapters the user has downloaded. The pages stay on disk until the user
//! deletes them, so previewing and exporting never need the network.
//!
//! Layout under the library folder:
//!
//! ```text
//! <library>/<series>/<chapter>/00001.img   the page as downloaded
//!                              00002.img
//!                              source.txt  which source copy these pages came from
//! ```
//!
//! This module also owns deletion, and every delete goes through
//! [`remove_inside`], which refuses to touch anything but a folder strictly
//! inside the library.

use crate::presets::PresetsDb;
use crate::settings::{ConversionField, ConversionSettings};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// ---- Identity --------------------------------------------------------------

/// A stable id for a series, from its title: the same title gives the same id
/// whatever the capitalisation or punctuation, so chapters downloaded on
/// different days from different sources land in one place. Also usable as a
/// folder name: `solo-leveling-3fa9c2e1`.
pub fn series_id(title: &str) -> String {
    let squashed: String = title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in squashed.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let words: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let mut slug = words.join("-");
    slug.truncate(40);
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "series" } else { slug };
    format!("{slug}-{:08x}", hash & 0xffff_ffff)
}

/// A stable name for a chapter within its series: `c12`, `c12_5`, or
/// `u<chapter id>` for chapters without a number.
pub fn chapter_key(number: Option<f64>, chapter_id: i64) -> String {
    match number {
        Some(n) if n.fract().abs() < 0.0005 => format!("c{}", n.round() as i64),
        Some(n) => format!(
            "c{}",
            format!("{n:.3}").trim_end_matches('0').replace('.', "_")
        ),
        None => format!("u{chapter_id}"),
    }
}

// ---- The index -------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadedChapter {
    pub key: String,
    pub number: Option<f64>,
    pub name: String,
    /// Where these pages came from (kept so a download can be repeated).
    pub source_id: String,
    pub chapter_id: i64,
    pub pages: usize,
    /// Size of the saved pages on disk.
    pub bytes: u64,
    pub downloaded_at: u64,
}

/// A source's copy of a series, saved with it so the series can be opened and
/// its chapters listed later without searching again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSource {
    pub source_id: String,
    pub source_name: String,
    pub lang: String,
    /// The series' id on the catalog server for this source.
    pub manga_id: i64,
    /// Where the cover is on the server, if it said.
    pub thumbnail: Option<String>,
}

/// What the sources say about a series, saved so the Library can describe it
/// long after the search that found it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeriesInfo {
    pub description: String,
    pub author: String,
    pub artist: String,
    /// "Ongoing", "Completed" and so on; empty when the source does not say.
    pub status: String,
    pub genres: Vec<String>,
    /// Source id and name, for the sources chapters came from.
    pub source_names: Vec<(String, String)>,
    /// A cover is saved next to the chapters (see [`LibraryFiles::cover_path`]).
    pub has_cover: bool,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
}

impl SeriesInfo {
    /// The name of a source, if it was recorded.
    pub fn source_name(&self, source_id: &str) -> Option<&str> {
        self.source_names
            .iter()
            .find(|(id, _)| id == source_id)
            .map(|(_, name)| name.as_str())
    }
}

/// A source's status word, as a person would say it.
pub fn status_label(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "ONGOING" => "Ongoing",
        "COMPLETED" => "Completed",
        "ON_HIATUS" => "On hiatus",
        "CANCELLED" => "Cancelled",
        "LICENSED" => "Licensed",
        "PUBLISHING_FINISHED" => "Publishing finished",
        _ => "",
    }
    .to_string()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadedSeries {
    #[serde(default)]
    pub crops: crate::CropOverrides,
    pub id: String,
    pub title: String,
    pub chapters: Vec<DownloadedChapter>,
    /// How pages of this series are converted (set while previewing).
    pub settings: ConversionSettings,
    /// The preset these settings started from, if any. The settings may have
    /// been changed since.
    pub preset_id: Option<String>,
    /// Settings this series changed from its preset. They keep the series'
    /// own value; everything else follows the preset, so editing a preset
    /// updates every series that uses it.
    pub overridden: Vec<ConversionField>,
    /// Description, author, genres and so on, once they have been fetched.
    pub info: Option<SeriesInfo>,
    /// Where to find the series again: the sources that carry it. A series can
    /// be in the library with these and no downloaded chapters.
    pub sources: Vec<SavedSource>,
    /// How many chapters the sources list in all, as last seen; `None` until
    /// they have been looked up.
    #[serde(default)]
    pub chapter_total: Option<usize>,
    /// When it was added to the library (seconds since the Unix epoch).
    pub added_at: u64,
    pub updated_at: u64,
}

/// The title of the made-up series the Export screen shows when nothing has
/// been chosen to export. It is kept out of the library's lists.
pub const SAMPLE_TITLE: &str = "Sample pages";

impl DownloadedSeries {
    /// The made-up series of sample pages.
    pub fn is_sample(&self) -> bool {
        self.title == SAMPLE_TITLE
    }

    pub fn bytes(&self) -> u64 {
        self.chapters.iter().map(|c| c.bytes).sum()
    }

    pub fn page_count(&self) -> usize {
        self.chapters.iter().map(|c| c.pages).sum()
    }

    pub fn chapter(&self, key: &str) -> Option<&DownloadedChapter> {
        self.chapters.iter().find(|c| c.key == key)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryDb {
    #[serde(deserialize_with = "crate::json::lenient_vec")]
    pub series: Vec<DownloadedSeries>,
}

impl LibraryDb {
    pub fn find(&self, series_id: &str) -> Option<&DownloadedSeries> {
        self.series.iter().find(|s| s.id == series_id)
    }

    /// Add or replace a downloaded chapter, creating the series if needed
    /// (with `defaults` as its conversion settings). Chapters stay in reading
    /// order, unnumbered ones last.
    pub fn record_chapter(
        &mut self,
        title: &str,
        defaults: &ConversionSettings,
        chapter: DownloadedChapter,
        now: u64,
    ) -> String {
        let id = series_id(title);
        let index = match self.series.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => {
                self.series.push(DownloadedSeries {
                    crops: Default::default(),
                    id: id.clone(),
                    title: title.to_string(),
                    chapters: Vec::new(),
                    settings: defaults.clone(),
                    preset_id: None,
                    overridden: Vec::new(),
                    info: None,
                    sources: Vec::new(),
                    chapter_total: None,
                    added_at: now,
                    updated_at: now,
                });
                self.series.len() - 1
            }
        };
        Self::insert_chapter(&mut self.series[index], chapter, now);
        id
    }

    /// Like [`LibraryDb::record_chapter`], but only when the series is still
    /// in the library: a series the user deleted while a download was running
    /// is not brought back. Returns whether the chapter was recorded.
    pub fn record_chapter_if_present(
        &mut self,
        title: &str,
        chapter: DownloadedChapter,
        now: u64,
    ) -> bool {
        let id = series_id(title);
        match self.series.iter_mut().find(|s| s.id == id) {
            Some(series) => {
                Self::insert_chapter(series, chapter, now);
                true
            }
            None => false,
        }
    }

    fn insert_chapter(series: &mut DownloadedSeries, chapter: DownloadedChapter, now: u64) {
        series.chapters.retain(|c| c.key != chapter.key);
        series.chapters.push(chapter);
        series.chapters.sort_by(|a, b| {
            a.number
                .unwrap_or(f64::MAX)
                .total_cmp(&b.number.unwrap_or(f64::MAX))
                .then_with(|| a.name.cmp(&b.name))
        });
        series.updated_at = now;
    }

    /// Forget one chapter; the series goes too when its last chapter does.
    /// Forget one downloaded chapter. The series stays in the library even
    /// when its last chapter goes; only [`LibraryDb::remove_series`] takes it out.
    pub fn remove_chapter(&mut self, series_id: &str, key: &str) -> bool {
        let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) else {
            return false;
        };
        let before = s.chapters.len();
        s.chapters.retain(|c| c.key != key);
        s.chapters.len() != before
    }

    /// Put a series in the library without downloading anything. If it is
    /// already there its sources are brought up to date. Returns its id.
    pub fn add_saved(
        &mut self,
        title: &str,
        sources: Vec<SavedSource>,
        defaults: &ConversionSettings,
        now: u64,
    ) -> String {
        let id = series_id(title);
        match self.series.iter_mut().find(|s| s.id == id) {
            Some(existing) => merge_sources(&mut existing.sources, sources),
            None => self.series.push(DownloadedSeries {
                crops: Default::default(),
                id: id.clone(),
                title: title.to_string(),
                chapters: Vec::new(),
                settings: defaults.clone(),
                preset_id: None,
                overridden: Vec::new(),
                info: None,
                sources,
                chapter_total: None,
                added_at: now,
                updated_at: now,
            }),
        }
        id
    }

    /// Note more sources a series can be found at.
    pub fn add_sources(&mut self, series_id: &str, sources: Vec<SavedSource>) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            merge_sources(&mut s.sources, sources);
        }
    }

    pub fn remove_series(&mut self, series_id: &str) -> Option<DownloadedSeries> {
        let i = self.series.iter().position(|s| s.id == series_id)?;
        Some(self.series.remove(i))
    }

    /// Give a series these settings. `preset` is the settings of the preset it
    /// follows, if any: whatever differs from it is remembered as the series'
    /// own.
    pub fn set_settings(
        &mut self,
        series_id: &str,
        settings: ConversionSettings,
        preset: Option<&ConversionSettings>,
    ) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            s.overridden = preset.map(|p| p.differing(&settings)).unwrap_or_default();
            s.settings = settings;
        }
    }

    /// Bring every series up to date with its preset after presets changed:
    /// a series takes the preset's settings except the ones it overrides. A
    /// series whose preset is gone keeps what it has and loses the link.
    /// Returns whether anything changed.
    pub fn follow_presets(&mut self, presets: &PresetsDb) -> bool {
        let mut changed = false;
        for s in &mut self.series {
            let Some(id) = s.preset_id.clone() else {
                continue;
            };
            let Some(entry) = presets.find(&id) else {
                s.preset_id = None;
                s.overridden.clear();
                changed = true;
                continue;
            };
            let mut next = entry.preset.settings.clone();
            for &field in &s.overridden {
                next.copy_field(&s.settings, field);
            }
            if next != s.settings {
                s.settings = next;
                changed = true;
            }
        }
        changed
    }

    /// A series still on the default settings, with no preset of its own yet,
    /// takes the preset those defaults came from.
    pub fn adopt_default_preset(
        &mut self,
        series_id: &str,
        defaults: &ConversionSettings,
        preset_id: &Option<String>,
    ) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            if s.preset_id.is_none() && s.settings == *defaults {
                s.preset_id = preset_id.clone();
                s.overridden.clear();
            }
        }
    }

    /// Remember how many chapters a series has in all. True if that changed.
    pub fn set_chapter_total(&mut self, series_id: &str, total: usize) -> bool {
        match self.series.iter_mut().find(|s| s.id == series_id) {
            Some(s) if s.chapter_total != Some(total) => {
                s.chapter_total = Some(total);
                true
            }
            _ => false,
        }
    }

    pub fn set_info(&mut self, series_id: &str, info: SeriesInfo) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            s.info = Some(info);
        }
    }

    /// Remember which preset a series started from.
    pub fn set_preset(&mut self, series_id: &str, preset_id: Option<String>) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            s.preset_id = preset_id;
            s.overridden.clear();
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.series.iter().map(DownloadedSeries::bytes).sum()
    }
}

// ---- Files on disk ---------------------------------------------------------

/// Where downloaded pages live, and the only code that deletes them.
#[derive(Clone, Debug)]
pub struct LibraryFiles {
    /// Where new series are kept.
    root: PathBuf,
    /// Other folders earlier downloads were kept in. A series stays in the
    /// folder it was first downloaded to.
    others: Vec<PathBuf>,
}

/// Add the sources that are new (same source and series id counts as known).
fn merge_sources(known: &mut Vec<SavedSource>, more: Vec<SavedSource>) {
    for source in more {
        match known
            .iter_mut()
            .find(|k| k.source_id == source.source_id && k.manga_id == source.manga_id)
        {
            // Keep what was there but fill in a cover path that was missing.
            Some(k) => {
                if k.thumbnail.is_none() {
                    k.thumbnail = source.thumbnail;
                }
            }
            None => known.push(source),
        }
    }
}

/// Make a value safe to use as one folder or file name, whatever the index
/// file says: separators, `..` and the like become underscores.
fn component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".into()
    } else {
        cleaned
    }
}

impl LibraryFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            others: Vec::new(),
        }
    }

    /// Also look for series in these folders.
    pub fn with_other_roots(mut self, others: impl IntoIterator<Item = PathBuf>) -> Self {
        self.others = others.into_iter().filter(|o| *o != self.root).collect();
        self
    }

    /// The folder new series are kept in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder a series is kept in: the one it is already in, or else the
    /// one new series go to.
    fn series_root(&self, series_id: &str) -> &Path {
        let name = component(series_id);
        if self.root.join(&name).exists() {
            return &self.root;
        }
        self.others
            .iter()
            .find(|other| other.join(&name).exists())
            .map_or(self.root.as_path(), PathBuf::as_path)
    }

    pub fn series_dir(&self, series_id: &str) -> PathBuf {
        self.series_root(series_id).join(component(series_id))
    }

    /// Where the series' cover image is kept: in its folder, beside the
    /// chapters, so deleting the series deletes it too.
    pub fn cover_path(&self, series_id: &str) -> PathBuf {
        self.series_dir(series_id).join("cover.img")
    }

    pub fn chapter_dir(&self, series_id: &str, key: &str) -> PathBuf {
        self.series_dir(series_id).join(component(key))
    }

    /// Path of page `index` (0-based), named so that sorting by name gives
    /// reading order.
    pub fn page_path(&self, series_id: &str, key: &str, index: usize) -> PathBuf {
        self.chapter_dir(series_id, key)
            .join(format!("{:05}.img", index + 1))
    }

    /// The saved pages of a chapter, in reading order. Empty files (left by
    /// an interrupted write) are ignored.
    pub fn page_files(&self, series_id: &str, key: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.chapter_dir(series_id, key)) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.path().extension().is_some_and(|x| x == "img")
                    && e.metadata().is_ok_and(|m| m.len() > 0)
            })
            .map(|e| e.path())
            .collect();
        files.sort();
        files
    }

    /// Total size of a chapter's saved pages.
    pub fn chapter_bytes(&self, series_id: &str, key: &str) -> u64 {
        self.page_files(series_id, key)
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn remove_chapter(&self, series_id: &str, key: &str) -> io::Result<()> {
        remove_inside(
            self.series_root(series_id),
            &self.chapter_dir(series_id, key),
        )
    }

    pub fn remove_series(&self, series_id: &str) -> io::Result<()> {
        remove_inside(self.series_root(series_id), &self.series_dir(series_id))
    }
}

/// Delete `target` and everything in it, but only if it is a folder strictly
/// inside `root`. Refuses the root itself, anything outside it, and symbolic
/// links. A folder that is already gone counts as success.
pub fn remove_inside(root: &Path, target: &Path) -> io::Result<()> {
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to delete {}: {why}", target.display()),
        ))
    };
    if target == root || !target.starts_with(root) {
        return refuse("it is not inside the library folder");
    }
    if target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return refuse("the path contains '..'");
    }
    match fs::symlink_metadata(target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.file_type().is_symlink() => refuse("it is a symbolic link"),
        Ok(meta) if !meta.is_dir() => refuse("it is not a folder"),
        Ok(_) => fs::remove_dir_all(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-lib-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn chapter(number: Option<f64>, id: i64, name: &str) -> DownloadedChapter {
        DownloadedChapter {
            key: chapter_key(number, id),
            number,
            name: name.into(),
            source_id: "s".into(),
            chapter_id: id,
            pages: 10,
            bytes: 1000,
            downloaded_at: 5,
        }
    }

    #[test]
    fn a_deleted_series_is_not_resurrected_by_a_late_chapter() {
        let mut db = LibraryDb::default();
        let defaults = ConversionSettings::default();
        let id = db.record_chapter("Solo", &defaults, chapter(Some(1.0), 1, "a"), 1);
        assert!(db.record_chapter_if_present("Solo", chapter(Some(2.0), 2, "b"), 2));
        assert_eq!(db.find(&id).unwrap().chapters.len(), 2);
        db.remove_series(&id);
        assert!(!db.record_chapter_if_present("Solo", chapter(Some(3.0), 3, "c"), 3));
        assert!(db.series.is_empty());
    }

    #[test]
    fn a_library_with_one_bad_series_and_old_chapters_still_loads() {
        let dir = temp("tolerant");
        let path = dir.join("library.json");
        // Series 1 has a chapter lacking most fields; series 2 is garbage.
        fs::write(
            &path,
            r#"{"series":[{"id":"a","title":"A","chapters":[{"key":"c1"}]},{"id":5,"title":[]},{"id":"b","title":"B"}]}"#,
        )
        .unwrap();
        let db = crate::JsonFile::<LibraryDb>::new(&path).load();
        assert_eq!(db.series.len(), 2);
        assert_eq!(db.series[0].chapters[0].key, "c1");
        assert!(path.exists(), "the file is kept, not set aside");
        assert!(dir.join("library.json.skipped.bad").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn status_words_are_readable_and_unknown_ones_are_left_out() {
        assert_eq!(status_label("ONGOING"), "Ongoing");
        assert_eq!(status_label("on_hiatus"), "On hiatus");
        assert_eq!(status_label("PUBLISHING_FINISHED"), "Publishing finished");
        assert_eq!(status_label("UNKNOWN"), "");
        assert_eq!(status_label(""), "");
    }

    #[test]
    fn a_series_keeps_its_details_and_the_cover_lives_in_its_own_folder() {
        let mut db = LibraryDb::default();
        let id = db.record_chapter(
            "X",
            &ConversionSettings::default(),
            chapter(Some(1.0), 1, "a"),
            1,
        );
        assert!(db.find(&id).unwrap().info.is_none());
        db.set_info(
            &id,
            SeriesInfo {
                description: "A story.".into(),
                genres: vec!["Action".into()],
                source_names: vec![("42".into(), "Some Source".into())],
                has_cover: true,
                ..Default::default()
            },
        );
        let info = db.find(&id).unwrap().info.clone().unwrap();
        assert_eq!(info.source_name("42"), Some("Some Source"));
        assert_eq!(info.source_name("nope"), None);

        let files = LibraryFiles::new("/lib");
        assert_eq!(
            files.cover_path(&id).parent().unwrap(),
            files.series_dir(&id)
        );
        // Even a hostile id cannot put the cover outside the library.
        let evil = files.cover_path("../../etc");
        assert!(evil.starts_with("/lib"), "{evil:?}");

        // Old library files without details still load.
        let old = r#"{"series":[{"id":"a","title":"A","chapters":[]}]}"#;
        let db: LibraryDb = serde_json::from_str(old).unwrap();
        assert!(db.series[0].info.is_none());
    }

    #[test]
    fn ids_ignore_case_and_punctuation_but_not_the_title() {
        assert_eq!(series_id("Solo Leveling"), series_id("SOLO-leveling!"));
        assert_ne!(
            series_id("Solo Leveling"),
            series_id("Solo Leveling: Ragnarok")
        );
        let id = series_id("Solo Leveling");
        assert!(id.starts_with("solo-leveling-"), "{id}");
        assert_eq!(id.len(), "solo-leveling-".len() + 8);
        // Titles with no ASCII letters still get a usable, distinct id.
        assert!(series_id("進撃の巨人").starts_with("series-"));
        assert_ne!(series_id("進撃の巨人"), series_id("鬼滅の刃"));
    }

    #[test]
    fn chapter_keys_are_stable_and_filename_safe() {
        assert_eq!(chapter_key(Some(12.0), 99), "c12");
        assert_eq!(chapter_key(Some(12.5), 99), "c12_5");
        assert_eq!(chapter_key(Some(0.01), 99), "c0_01");
        assert_eq!(chapter_key(None, 99), "u99");
    }

    #[test]
    fn chapters_are_kept_in_reading_order_and_replaced_not_duplicated() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        db.record_chapter("Berserk", &d, chapter(Some(2.0), 2, "Two"), 1);
        db.record_chapter("berserk", &d, chapter(None, 7, "Note"), 2);
        let id = db.record_chapter("Berserk", &d, chapter(Some(1.0), 1, "One"), 3);
        db.record_chapter("Berserk", &d, chapter(Some(2.0), 22, "Two again"), 4);

        assert_eq!(
            db.series.len(),
            1,
            "same title, different spelling: one series"
        );
        let s = db.find(&id).unwrap();
        let names: Vec<_> = s.chapters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["One", "Two again", "Note"]);
        assert_eq!(s.updated_at, 4);
        assert_eq!(s.bytes(), 3000);
        assert_eq!(s.page_count(), 30);
    }

    #[test]
    fn a_new_series_starts_with_the_users_default_settings() {
        let mut db = LibraryDb::default();
        let defaults = ConversionSettings {
            contrast: 70,
            ..Default::default()
        };
        let id = db.record_chapter("X", &defaults, chapter(Some(1.0), 1, "a"), 1);
        assert_eq!(db.find(&id).unwrap().settings.contrast, 70);
        // Later chapters do not overwrite settings the user tuned for the series.
        let mut tuned = defaults.clone();
        tuned.contrast = 40;
        db.set_settings(&id, tuned, None);
        db.record_chapter("X", &defaults, chapter(Some(2.0), 2, "b"), 2);
        assert_eq!(db.find(&id).unwrap().settings.contrast, 40);
    }

    #[test]
    fn series_follow_their_preset_except_for_what_they_changed() {
        let mut presets = PresetsDb::default();
        let manga = presets.find("manga").unwrap().preset.settings;
        let mut db = LibraryDb::default();
        let id = db.record_chapter("X", &manga, chapter(Some(1.0), 1, "a"), 1);
        db.set_preset(&id, Some("manga".into()));

        // The user raises this series' contrast: that one setting is its own.
        let mut mine = manga.clone();
        mine.contrast = 90;
        db.set_settings(&id, mine, Some(&manga));
        assert_eq!(
            db.find(&id).unwrap().overridden,
            vec![ConversionField::Contrast]
        );

        // Editing the preset changes the series' other settings, not its own.
        let mut edited = manga.clone();
        edited.contrast = 20;
        edited.overlap_percent = 30;
        presets.overwrite("manga", edited).unwrap();
        assert!(db.follow_presets(&presets));
        let s = &db.find(&id).unwrap().settings;
        assert_eq!(s.contrast, 90, "overridden");
        assert_eq!(s.overlap_percent, 30, "followed the preset");
        assert!(!db.follow_presets(&presets), "nothing left to change");

        // Picking a preset again drops the series' own changes.
        db.set_preset(&id, Some("manga".into()));
        assert!(db.find(&id).unwrap().overridden.is_empty());

        // A deleted preset leaves the settings and cuts the link.
        db.series[0].preset_id = Some("gone".into());
        assert!(db.follow_presets(&presets));
        assert_eq!(db.series[0].preset_id, None);
    }

    #[test]
    fn removing_the_last_chapter_keeps_the_series_in_the_library() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        let id = db.record_chapter("X", &d, chapter(Some(1.0), 1, "a"), 1);
        db.record_chapter("X", &d, chapter(Some(2.0), 2, "b"), 1);
        assert!(db.remove_chapter(&id, "c1"));
        assert!(!db.remove_chapter(&id, "c1"), "already gone");
        assert!(db.remove_chapter(&id, "c2"));
        let series = db.find(&id).expect("the series is still in the library");
        assert!(series.chapters.is_empty());
        assert_eq!(db.total_bytes(), 0);
        // Only an explicit removal takes it out.
        assert!(db.remove_series(&id).is_some());
        assert!(db.find(&id).is_none());
    }

    fn source(id: &str, manga: i64) -> SavedSource {
        SavedSource {
            source_id: id.into(),
            source_name: format!("Source {id}"),
            lang: "en".into(),
            manga_id: manga,
            thumbnail: None,
        }
    }

    #[test]
    fn a_series_can_be_saved_without_downloading_anything() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        let id = db.add_saved("Berserk", vec![source("a", 1), source("b", 2)], &d, 50);
        let series = db.find(&id).unwrap();
        assert!(series.chapters.is_empty());
        assert_eq!(series.sources.len(), 2);
        assert_eq!(series.added_at, 50);
        assert_eq!(db.total_bytes(), 0);

        // Saving it again does not duplicate it, and learns of new sources.
        let again = db.add_saved("berserk", vec![source("a", 1), source("c", 3)], &d, 99);
        assert_eq!(again, id);
        assert_eq!(db.series.len(), 1);
        let series = db.find(&id).unwrap();
        assert_eq!(series.sources.len(), 3, "a, b and the new c");
        assert_eq!(series.added_at, 50, "the original date is kept");

        // Downloading a chapter later puts it on the same series.
        let same = db.record_chapter("Berserk", &d, chapter(Some(1.0), 1, "One"), 120);
        assert_eq!(same, id);
        assert_eq!(db.find(&id).unwrap().chapters.len(), 1);
        assert_eq!(
            db.find(&id).unwrap().sources.len(),
            3,
            "its sources are kept"
        );
    }

    #[test]
    fn a_known_source_only_fills_in_a_missing_cover_path() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        let id = db.add_saved("X", vec![source("a", 1)], &d, 1);
        let mut with_cover = source("a", 1);
        with_cover.thumbnail = Some("/cover".into());
        db.add_sources(&id, vec![with_cover]);
        let s = db.find(&id).unwrap();
        assert_eq!(s.sources.len(), 1);
        assert_eq!(s.sources[0].thumbnail.as_deref(), Some("/cover"));
        // A later, different path does not replace the first.
        let mut other = source("a", 1);
        other.thumbnail = Some("/other".into());
        db.add_sources(&id, vec![other]);
        assert_eq!(
            db.find(&id).unwrap().sources[0].thumbnail.as_deref(),
            Some("/cover")
        );
    }

    #[test]
    fn the_index_survives_a_json_round_trip() {
        let mut db = LibraryDb::default();
        db.record_chapter(
            "X",
            &ConversionSettings::default(),
            chapter(Some(1.5), 1, "a"),
            1,
        );
        let json = serde_json::to_string(&db).unwrap();
        assert_eq!(serde_json::from_str::<LibraryDb>(&json).unwrap(), db);
    }

    #[test]
    fn paths_stay_inside_the_library_even_for_hostile_names() {
        let root = PathBuf::from("/lib");
        let files = LibraryFiles::new(&root);
        for evil in ["../../etc", "..", "a/b", r"a\b", "/abs", ""] {
            let p = files.chapter_dir(evil, evil);
            assert!(p.starts_with(&root), "{evil:?} -> {p:?}");
            assert!(
                !p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
                "{p:?}"
            );
        }
    }

    #[test]
    fn pages_are_listed_in_reading_order_and_empty_files_are_ignored() {
        let root = temp("pages");
        let files = LibraryFiles::new(&root);
        for (i, data) in [(2usize, b"c".as_slice()), (0, b"a"), (1, b"b"), (3, b"")] {
            let p = files.page_path("s", "c1", i);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        fs::write(files.chapter_dir("s", "c1").join("source.txt"), "x").unwrap();
        let listed = files.page_files("s", "c1");
        assert_eq!(
            listed.len(),
            3,
            "the empty page and source.txt are not pages"
        );
        let contents: Vec<_> = listed.iter().map(|p| fs::read(p).unwrap()).collect();
        assert_eq!(contents, [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        assert_eq!(files.chapter_bytes("s", "c1"), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_removes_only_the_chosen_chapter_or_series() {
        let root = temp("delete");
        let files = LibraryFiles::new(&root);
        for (s, c) in [("a", "c1"), ("a", "c2"), ("b", "c1")] {
            let p = files.page_path(s, c, 0);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        files.remove_chapter("a", "c1").unwrap();
        assert!(!files.chapter_dir("a", "c1").exists());
        assert!(files.chapter_dir("a", "c2").exists());
        files.remove_series("a").unwrap();
        assert!(!files.series_dir("a").exists());
        assert!(
            files.chapter_dir("b", "c1").exists(),
            "other series untouched"
        );
        files.remove_series("a").unwrap(); // already gone is fine
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_inside_refuses_the_root_outsiders_and_files() {
        let base = temp("guard");
        let root = base.join("library");
        let outside = base.join("precious");
        fs::create_dir_all(root.join("series")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        fs::write(root.join("note.txt"), "x").unwrap();

        assert!(
            remove_inside(&root, &root).is_err(),
            "never the library itself"
        );
        assert!(remove_inside(&root, &outside).is_err(), "never a sibling");
        assert!(remove_inside(&root, &base).is_err(), "never a parent");
        assert!(remove_inside(
            &root,
            &root.join("series").join("..").join("..").join("precious")
        )
        .is_err());
        assert!(
            remove_inside(&root, &root.join("note.txt")).is_err(),
            "not a plain file"
        );
        assert!(outside.join("keep.txt").exists());
        assert!(root.join("series").exists());

        assert!(remove_inside(&root, &root.join("series")).is_ok());
        assert!(!root.join("series").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_series_stays_in_the_folder_it_started_in() {
        let base = std::env::temp_dir().join(format!("mangapoint-roots-{}", std::process::id()));
        let (old, new) = (base.join("old"), base.join("new"));
        fs::create_dir_all(old.join("kept-series")).unwrap();
        fs::create_dir_all(&new).unwrap();
        let files = LibraryFiles::new(&new).with_other_roots([old.clone()]);
        assert_eq!(files.root(), new.as_path());
        // An existing series is found where it is, a new one goes to the new folder.
        assert_eq!(files.series_dir("kept-series"), old.join("kept-series"));
        assert_eq!(files.series_dir("fresh-series"), new.join("fresh-series"));
        // Deleting it deletes it from the folder it is in.
        files.remove_series("kept-series").unwrap();
        assert!(!old.join("kept-series").exists());
        assert!(old.exists(), "the folder itself is left alone");
        let _ = fs::remove_dir_all(&base);
    }
}
