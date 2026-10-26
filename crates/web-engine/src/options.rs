//! The options of an export, as the page sends them to the [`Exporter`] and as
//! [`default_options_json`] makes them.
//!
//! Everything about *how pages are converted* is the desktop app's own type,
//! [`library::ConversionSettings`] (the same JSON the desktop saves), so a
//! preset means the same thing in both places. This module adds what the
//! desktop gets from elsewhere: which device, which file type, how chapters are
//! grouped into files, and what the book is called.
//!
//! ```json
//! { "device": "kobo-libra-colour",   // an id from devices_json(); or give "customDevice"
//!   "format": "EPUB",                // XTC | XTCH | EPUB | CBZ | PDF (PDF: "phone-original" only)
//!   "settings": { ... },             // library::ConversionSettings (every field optional)
//!   "grouping": { "mode": "single" },// or { "mode": "perChapter" } / { "mode": "everyN", "n": 10 }
//!   "title": "My Series", "language": "en",
//!   "info": { "description": "", "author": "", "artist": "", "genres": [], "status": "", "sources": [] },
//!   "cover": false,                  // open every file with the first page (as a cover page)
//!   "created": 1781000000,           // Unix seconds written into the files (the page passes "now")
//!   "maxPagesPerFile": 60000 }
//! ```
//!
//! `settings` has the desktop's names (`layout`, `crop`, `direction`,
//! `contrast`, `black_and_white`, ...); the file type in `format` wins over the
//! `epub`/`cbz`/`format` flags inside it.
//!
//! [`Exporter`]: crate::export::Exporter

use device_profiles::DeviceProfile;
use library::{ConversionSettings, OutputFormat};
use serde::{Deserialize, Serialize};

/// How chapters are divided into files (the desktop's `Grouping`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum Grouping {
    /// Everything in one file (the default).
    #[default]
    Single,
    /// One file per chapter.
    PerChapter,
    /// One file for every `n` chapters.
    EveryN { n: usize },
}

impl Grouping {
    /// Which group (file) the chapter with this 0-based index belongs to.
    pub fn group_of(self, chapter_index: usize) -> usize {
        match self {
            Grouping::Single => 0,
            Grouping::PerChapter => chapter_index,
            Grouping::EveryN { n } => chapter_index / n.max(1),
        }
    }
}

/// What the series says about itself, for the files' details.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    pub description: String,
    pub author: String,
    pub artist: String,
    pub genres: Vec<String>,
    /// "Ongoing", "Completed" and so on.
    pub status: String,
    /// Where the pages came from, for a CBZ's notes.
    pub sources: Vec<String>,
}

/// The file types an export can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FileType {
    Xtc,
    Xtch,
    Epub,
    Cbz,
    /// Only for the "phone or tablet" entry: the pages as they are, in a PDF.
    Pdf,
}

impl FileType {
    pub fn extension(self) -> &'static str {
        match self {
            FileType::Xtc => "xtc",
            FileType::Xtch => "xtch",
            FileType::Epub => "epub",
            FileType::Cbz => "cbz",
            FileType::Pdf => "pdf",
        }
    }

    pub fn output(self) -> Option<OutputFormat> {
        match self {
            FileType::Xtc => Some(OutputFormat::Xtc),
            FileType::Xtch => Some(OutputFormat::Xtch),
            FileType::Epub => Some(OutputFormat::Epub),
            FileType::Cbz => Some(OutputFormat::Cbz),
            FileType::Pdf => None,
        }
    }

    fn of(output: OutputFormat) -> Self {
        match output {
            OutputFormat::Xtc => FileType::Xtc,
            OutputFormat::Xtch => FileType::Xtch,
            OutputFormat::Epub => FileType::Epub,
            OutputFormat::Cbz => FileType::Cbz,
        }
    }
}

/// The options of one export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportOptions {
    /// The id of a device from `devices_json()`.
    pub device: String,
    /// A device of the visitor's own (a size), used when `device` is empty.
    pub custom_device: Option<DeviceProfile>,
    /// The file type; when left out it follows `settings`.
    pub format: Option<FileType>,
    pub settings: ConversionSettings,
    pub grouping: Grouping,
    pub title: String,
    pub language: String,
    pub info: Info,
    pub cover: bool,
    pub created: u64,
    pub max_pages_per_file: usize,
}

/// The most pages one file may hold unless the options say less.
pub const DEFAULT_MAX_PAGES_PER_FILE: usize = 60_000;

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            device: device_profiles::X4_PRO_ID.into(),
            custom_device: None,
            format: None,
            settings: ConversionSettings::default(),
            grouping: Grouping::Single,
            title: String::new(),
            language: "en".into(),
            info: Info::default(),
            cover: false,
            created: 0,
            max_pages_per_file: DEFAULT_MAX_PAGES_PER_FILE,
        }
    }
}

impl ExportOptions {
    /// Read options from JSON text. An empty text means the defaults.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut options: ExportOptions = if text.trim().is_empty() {
            ExportOptions::default()
        } else {
            serde_json::from_str(text)
                .map_err(|e| format!("The export options are not valid: {e}"))?
        };
        if let Some(output) = options.format.and_then(FileType::output) {
            options.settings.set_output(output);
        }
        Ok(options)
    }

    /// The device the pages are made for.
    pub fn resolve_device(&self) -> Result<DeviceProfile, String> {
        if device_profiles::is_original(&self.device) {
            return Ok(device_profiles::phone_original());
        }
        if !self.device.is_empty() {
            return device_profiles::known(&self.device).ok_or_else(|| {
                format!("MangaPoint does not know a device called \"{}\".", self.device)
            });
        }
        let custom = self
            .custom_device
            .clone()
            .ok_or_else(|| "Choose a device for the export.".to_string())?;
        custom
            .validate(&[])
            .map_err(|e| format!("The device is not usable: {e}."))?;
        Ok(custom)
    }

    /// The file type that will be made.
    pub fn file_type(&self) -> FileType {
        if device_profiles::is_original(&self.device) {
            // The phone entry makes CBZ (the default) or PDF; nothing else applies.
            return match self.format {
                Some(FileType::Pdf) => FileType::Pdf,
                _ => FileType::Cbz,
            };
        }
        match self.format {
            Some(FileType::Pdf) | None => FileType::of(self.settings.output()),
            Some(f) => f,
        }
    }
}

/// `default_options_json(device_id, preset)`: the options a device and a preset
/// (`"manga"`, `"manhwa"` or `"manhua"`) start from, with the first file type the
/// device opens.
pub fn default_options_json(device_id: &str, preset: &str) -> Result<String, String> {
    let device = if device_profiles::is_original(device_id) {
        device_profiles::phone_original()
    } else {
        device_profiles::known(device_id)
            .ok_or_else(|| format!("MangaPoint does not know a device called \"{device_id}\"."))?
    };
    let wanted = preset.trim().to_ascii_lowercase();
    let preset = library::builtin_presets()
        .into_iter()
        .find(|p| p.id == wanted)
        .ok_or_else(|| format!("There is no preset called \"{preset}\" (use manga, manhwa or manhua)."))?;
    let mut settings = preset.settings;
    let format = crate::devices::formats_of(&device)
        .first()
        .copied()
        .and_then(|f| match f {
            "XTC" => Some(FileType::Xtc),
            "XTCH" => Some(FileType::Xtch),
            "EPUB" => Some(FileType::Epub),
            "CBZ" => Some(FileType::Cbz),
            _ => None,
        })
        .unwrap_or(FileType::Cbz);
    if let Some(output) = format.output() {
        settings.set_output(output);
    }
    let options = ExportOptions {
        device: device.id.clone(),
        format: Some(format),
        settings,
        ..ExportOptions::default()
    };
    serde_json::to_string_pretty(&options).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_device_and_the_preset() {
        let o = ExportOptions::from_json(&default_options_json("xteink-x4", "manga").unwrap()).unwrap();
        assert_eq!(o.file_type(), FileType::Xtc);
        assert_eq!(o.settings.output(), OutputFormat::Xtc);
        let o = ExportOptions::from_json(&default_options_json("kobo-libra-colour", "Manhwa").unwrap())
            .unwrap();
        assert_eq!(o.file_type(), FileType::Epub);
        assert_eq!(format!("{:?}", o.settings.layout), "Webtoon");
        assert!(o.settings.keeps_color(&o.resolve_device().unwrap()));
        let o = ExportOptions::from_json(&default_options_json("phone-original", "manga").unwrap())
            .unwrap();
        assert_eq!(o.file_type(), FileType::Cbz);
        assert!(default_options_json("nokia", "manga").unwrap_err().contains("nokia"));
        assert!(default_options_json("xteink-x4", "comic").unwrap_err().contains("preset"));
    }

    #[test]
    fn partial_options_fill_in_and_pdf_is_for_the_phone_only() {
        let o = ExportOptions::from_json(
            r#"{"device":"kindle-paperwhite-12","format":"CBZ","grouping":{"mode":"everyN","n":3}}"#,
        )
        .unwrap();
        assert_eq!(o.file_type(), FileType::Cbz);
        assert_eq!(o.grouping.group_of(7), 2);
        let o = ExportOptions::from_json(r#"{"device":"kindle-paperwhite-12","format":"PDF"}"#).unwrap();
        assert_eq!(o.file_type(), FileType::Xtc, "a PDF needs the phone entry");
        let o = ExportOptions::from_json(r#"{"device":"phone-original","format":"PDF"}"#).unwrap();
        assert_eq!(o.file_type(), FileType::Pdf);
        assert!(ExportOptions::from_json("{").is_err());
        assert!(ExportOptions::from_json(r#"{"device":"nope"}"#)
            .unwrap()
            .resolve_device()
            .is_err());
        let custom = ExportOptions::from_json(
            r#"{"device":"","customDevice":{"id":"mine","name":"Mine","width":600,"height":800}}"#,
        )
        .unwrap();
        assert_eq!(custom.resolve_device().unwrap().ppi, 220);
        let bad = ExportOptions::from_json(
            r#"{"device":"","customDevice":{"id":"m","name":"M","width":5,"height":800}}"#,
        )
        .unwrap();
        assert!(bad.resolve_device().is_err());
    }
}
