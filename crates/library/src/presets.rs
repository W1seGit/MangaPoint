//! Conversion presets: a named set of conversion settings.
//!
//! Three presets ship with the app (manga, manhwa, manhua) because those are
//! made differently. The user can overwrite any of them, save their own, rename
//! their own, and delete their own. Deleting a changed built-in puts back the
//! original, so the built-ins can never be lost.

use crate::settings::ConversionSettings;
use converter::process::{CropMode, Layout, SpreadMode};
use converter::xtc::ReadDirection;
use converter::Dither;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_NAME_CHARS: usize = 40;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionPreset {
    pub id: String,
    pub name: String,
    /// One line about what the preset is for.
    #[serde(default)]
    pub description: String,
    pub settings: ConversionSettings,
}

/// A preset as shown in a list.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetEntry {
    pub preset: ConversionPreset,
    /// Ships with the app.
    pub builtin: bool,
    /// A built-in the user has overwritten.
    pub modified: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresetError {
    #[error("give the preset a name")]
    EmptyName,
    #[error("a preset called \"{0}\" already exists")]
    NameTaken(String),
    #[error("that preset no longer exists")]
    Missing,
    #[error("the built-in presets keep their names; save a copy under your own name")]
    BuiltinName,
}

/// The presets that ship with the app.
pub fn builtin_presets() -> Vec<ConversionPreset> {
    let manga = ConversionSettings::default();
    vec![
        ConversionPreset {
            id: "manga".into(),
            name: "Manga".into(),
            description: "Japanese manga. Read right to left, one page per image, blank margins trimmed, and double-page spreads split into two pages."
                .into(),
            settings: manga.clone(),
        },
        ConversionPreset {
            id: "manhwa".into(),
            name: "Manhwa".into(),
            description:
                "Korean webtoons. Read left to right. The long vertical strips are joined and cut into screen-sized pages at the gaps between scenes."
                    .into(),
            settings: ConversionSettings {
                direction: ReadDirection::LeftToRight,
                layout: Layout::Webtoon,
                crop: CropMode::Margins,
                trim_padding: 0,
                spread: SpreadMode::Fit,
                overlap_percent: 10,
                ..manga.clone()
            },
        },
        ConversionPreset {
            id: "manhua".into(),
            name: "Manhua".into(),
            description:
                "Chinese comics, often in color. Read left to right. Strips and pages are told apart automatically, with a little more contrast so color art stays readable in gray."
                    .into(),
            settings: ConversionSettings {
                direction: ReadDirection::LeftToRight,
                layout: Layout::Auto,
                crop: CropMode::Margins,
                spread: SpreadMode::Split,
                contrast: 60,
                dither: Dither::FloydSteinberg,
                ..manga
            },
        },
    ]
}

/// What the user changed or added. A stored preset whose id matches a built-in
/// replaces it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetsDb {
    #[serde(deserialize_with = "crate::json::lenient_vec")]
    pub presets: Vec<ConversionPreset>,
}

impl PresetsDb {
    /// Built-ins first (with the user's changes applied), then the user's own.
    pub fn all(&self) -> Vec<PresetEntry> {
        let mut out: Vec<PresetEntry> = builtin_presets()
            .into_iter()
            .map(
                |original| match self.presets.iter().find(|p| p.id == original.id) {
                    Some(changed) => PresetEntry {
                        preset: ConversionPreset {
                            id: original.id.clone(),
                            name: original.name.clone(),
                            description: original.description.clone(),
                            settings: changed.settings.clone(),
                        },
                        builtin: true,
                        modified: changed.settings != original.settings,
                    },
                    None => PresetEntry {
                        preset: original,
                        builtin: true,
                        modified: false,
                    },
                },
            )
            .collect();
        let builtin_ids: Vec<String> = out.iter().map(|e| e.preset.id.clone()).collect();
        out.extend(
            self.presets
                .iter()
                .filter(|p| !builtin_ids.contains(&p.id))
                .map(|p| PresetEntry {
                    preset: p.clone(),
                    builtin: false,
                    modified: false,
                }),
        );
        out
    }

    pub fn find(&self, id: &str) -> Option<PresetEntry> {
        self.all().into_iter().find(|e| e.preset.id == id)
    }

    /// Save the settings under a new name. Returns the new preset's id.
    pub fn save_new(
        &mut self,
        name: &str,
        settings: ConversionSettings,
    ) -> Result<String, PresetError> {
        let name = clean_name(name)?;
        self.ensure_name_free(&name, None)?;
        let id = self.fresh_id(&name);
        self.presets.push(ConversionPreset {
            id: id.clone(),
            name,
            description: String::new(),
            settings,
        });
        Ok(id)
    }

    /// Replace a preset's settings with these. For a built-in this stores the
    /// user's version beside it, which [`PresetsDb::delete`] can undo.
    pub fn overwrite(&mut self, id: &str, settings: ConversionSettings) -> Result<(), PresetError> {
        let entry = self.find(id).ok_or(PresetError::Missing)?;
        match self.presets.iter_mut().find(|p| p.id == id) {
            Some(stored) => stored.settings = settings,
            None => self.presets.push(ConversionPreset {
                settings,
                ..entry.preset
            }),
        }
        Ok(())
    }

    /// Rename one of the user's own presets.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), PresetError> {
        let entry = self.find(id).ok_or(PresetError::Missing)?;
        if entry.builtin {
            return Err(PresetError::BuiltinName);
        }
        let name = clean_name(name)?;
        self.ensure_name_free(&name, Some(id))?;
        if let Some(p) = self.presets.iter_mut().find(|p| p.id == id) {
            p.name = name;
        }
        Ok(())
    }

    /// Delete one of the user's own presets, or put a changed built-in back
    /// to the original. Returns what happened.
    pub fn delete(&mut self, id: &str) -> Result<Deleted, PresetError> {
        let entry = self.find(id).ok_or(PresetError::Missing)?;
        self.presets.retain(|p| p.id != id);
        Ok(if entry.builtin {
            Deleted::Reset
        } else {
            Deleted::Removed
        })
    }

    fn ensure_name_free(&self, name: &str, except_id: Option<&str>) -> Result<(), PresetError> {
        let taken = self
            .all()
            .iter()
            .any(|e| Some(e.preset.id.as_str()) != except_id && same_name(&e.preset.name, name));
        if taken {
            Err(PresetError::NameTaken(name.to_string()))
        } else {
            Ok(())
        }
    }

    fn fresh_id(&self, name: &str) -> String {
        let base: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if base.is_empty() {
            "preset".to_string()
        } else {
            format!("my-{base}")
        };
        let all = self.all();
        let used = |id: &str| all.iter().any(|e| e.preset.id == id);
        if !used(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !used(id))
            .expect("an unused id exists")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deleted {
    /// A preset of the user's own is gone.
    Removed,
    /// A built-in is back to how it shipped.
    Reset,
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn clean_name(name: &str) -> Result<String, PresetError> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(PresetError::EmptyName);
    }
    Ok(name.chars().take(MAX_NAME_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tweaked() -> ConversionSettings {
        ConversionSettings {
            contrast: 80,
            ..Default::default()
        }
    }

    #[test]
    fn the_three_built_ins_differ_in_the_ways_that_matter() {
        let all = PresetsDb::default().all();
        let names: Vec<_> = all.iter().map(|e| e.preset.name.as_str()).collect();
        assert_eq!(names, ["Manga", "Manhwa", "Manhua"]);
        let by = |id: &str| {
            all.iter()
                .find(|e| e.preset.id == id)
                .unwrap()
                .preset
                .settings
                .clone()
        };
        assert_eq!(by("manga"), ConversionSettings::default());
        assert_eq!(by("manga").direction, ReadDirection::RightToLeft);
        assert_eq!(by("manhwa").layout, Layout::Webtoon);
        assert_eq!(by("manhwa").direction, ReadDirection::LeftToRight);
        assert_eq!(by("manhua").direction, ReadDirection::LeftToRight);
        assert!(all.iter().all(|e| e.builtin && !e.modified));
        assert!(all.iter().all(|e| !e.preset.description.is_empty()));
    }

    #[test]
    fn saving_a_new_preset_names_and_lists_it_after_the_built_ins() {
        let mut db = PresetsDb::default();
        let id = db.save_new("  Dark   scans ", tweaked()).unwrap();
        assert_eq!(id, "my-dark-scans");
        let all = db.all();
        assert_eq!(all.len(), 4);
        let mine = all.last().unwrap();
        assert_eq!(mine.preset.name, "Dark scans");
        assert!(!mine.builtin);
        assert_eq!(mine.preset.settings.contrast, 80);
    }

    #[test]
    fn names_must_be_present_and_not_repeat_case_insensitively() {
        let mut db = PresetsDb::default();
        assert_eq!(db.save_new("   ", tweaked()), Err(PresetError::EmptyName));
        assert_eq!(
            db.save_new("manga", tweaked()),
            Err(PresetError::NameTaken("manga".into()))
        );
        db.save_new("Mine", tweaked()).unwrap();
        assert!(matches!(
            db.save_new("MINE", tweaked()),
            Err(PresetError::NameTaken(_))
        ));
    }

    #[test]
    fn two_names_that_clean_to_the_same_id_get_different_ids() {
        let mut db = PresetsDb::default();
        let a = db.save_new("A b", tweaked()).unwrap();
        let b = db.save_new("A-b", tweaked()).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn overwriting_changes_settings_and_keeps_the_name() {
        let mut db = PresetsDb::default();
        let id = db.save_new("Mine", ConversionSettings::default()).unwrap();
        db.overwrite(&id, tweaked()).unwrap();
        assert_eq!(db.find(&id).unwrap().preset.settings.contrast, 80);
        assert_eq!(db.find(&id).unwrap().preset.name, "Mine");
        assert_eq!(db.overwrite("nope", tweaked()), Err(PresetError::Missing));
    }

    #[test]
    fn a_built_in_can_be_overwritten_and_then_reset() {
        let mut db = PresetsDb::default();
        db.overwrite("manhwa", tweaked()).unwrap();
        let e = db.find("manhwa").unwrap();
        assert!(e.builtin && e.modified);
        assert_eq!(e.preset.name, "Manhwa");
        assert_eq!(e.preset.settings.contrast, 80);
        assert_eq!(db.all().len(), 3, "an override does not add a fourth entry");

        assert_eq!(db.delete("manhwa"), Ok(Deleted::Reset));
        let e = db.find("manhwa").unwrap();
        assert!(!e.modified);
        assert_eq!(e.preset.settings.layout, Layout::Webtoon);
    }

    #[test]
    fn overwriting_a_built_in_back_to_the_original_is_not_a_modification() {
        let mut db = PresetsDb::default();
        let original = db.find("manga").unwrap().preset.settings;
        db.overwrite("manga", tweaked()).unwrap();
        db.overwrite("manga", original).unwrap();
        assert!(!db.find("manga").unwrap().modified);
    }

    #[test]
    fn only_the_users_own_presets_can_be_renamed_or_removed() {
        let mut db = PresetsDb::default();
        assert_eq!(db.rename("manga", "Japan"), Err(PresetError::BuiltinName));
        let id = db.save_new("Mine", tweaked()).unwrap();
        db.save_new("Other", tweaked()).unwrap();
        assert!(matches!(
            db.rename(&id, "other"),
            Err(PresetError::NameTaken(_))
        ));
        db.rename(&id, "Mine, renamed").unwrap();
        assert_eq!(db.find(&id).unwrap().preset.name, "Mine, renamed");
        assert_eq!(db.rename(&id, "  "), Err(PresetError::EmptyName));
        assert_eq!(db.delete(&id), Ok(Deleted::Removed));
        assert!(db.find(&id).is_none());
        assert_eq!(db.delete(&id), Err(PresetError::Missing));
    }

    #[test]
    fn presets_survive_a_round_trip_and_old_files_still_load() {
        let mut db = PresetsDb::default();
        db.save_new("Mine", tweaked()).unwrap();
        db.overwrite("manga", tweaked()).unwrap();
        let text = serde_json::to_string(&db).unwrap();
        assert_eq!(serde_json::from_str::<PresetsDb>(&text).unwrap(), db);
        assert_eq!(
            serde_json::from_str::<PresetsDb>("{}").unwrap(),
            PresetsDb::default()
        );
    }

    #[test]
    fn long_names_are_cut_to_a_sensible_length() {
        let mut db = PresetsDb::default();
        let id = db.save_new(&"x".repeat(100), tweaked()).unwrap();
        assert_eq!(
            db.find(&id).unwrap().preset.name.chars().count(),
            MAX_NAME_CHARS
        );
    }
}
