//! User preferences.

use converter::process::{
    CropMode, Layout, Margins, PageZoom, SpreadMode, StripCut, ZoomMode, DEFAULT_TRIM_PADDING,
    DEFAULT_TRIM_SENSITIVITY,
};
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use device_profiles::{DeviceProfile, Firmware};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What a book is made as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    /// `.xtc`: black and white pages for the Xteink readers.
    #[default]
    Xtc,
    /// `.xtch`: four grays, for the Xteink readers (twice the size, and slower to show).
    Xtch,
    /// `.epub`: a fixed-layout book of page pictures, which most readers open.
    Epub,
    /// `.cbz`: a comic book archive: a zip of page pictures with a ComicInfo.xml
    /// that carries the series, the chapters and the reading direction. Kobo,
    /// PocketBook, many Boox readers and KOReader open it.
    Cbz,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 4] = [
        OutputFormat::Xtc,
        OutputFormat::Xtch,
        OutputFormat::Epub,
        OutputFormat::Cbz,
    ];

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Xtc => "xtc",
            OutputFormat::Xtch => "xtch",
            OutputFormat::Epub => "epub",
            OutputFormat::Cbz => "cbz",
        }
    }

    /// Its name as shown, and as listed among a reader's file formats.
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Xtc => "XTC",
            OutputFormat::Xtch => "XTCH",
            OutputFormat::Epub => "EPUB",
            OutputFormat::Cbz => "CBZ",
        }
    }
}

/// How the reader is held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Upright: pages are as tall as the screen is.
    #[default]
    Portrait,
    /// Sideways: pages are as wide as the screen is tall, for a reader held
    /// turned. An XTC or XTCH book has its pages turned into the reader's
    /// own frame (hold the reader turned counter-clockwise); an EPUB or CBZ
    /// has pictures the shape of the sideways screen, and the reader's own
    /// rotation shows them.
    Landscape,
}

impl Orientation {
    pub fn is_portrait(&self) -> bool {
        *self == Orientation::Portrait
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionSettings {
    /// The page format of an XTC or XTCH book. An EPUB ignores it.
    pub format: Format,
    /// Make an EPUB instead of an XTC or XTCH book.
    pub epub: bool,
    /// Make a CBZ (a comic book archive) instead. Wins over `epub` if both are set.
    pub cbz: bool,
    pub direction: ReadDirection,
    pub layout: Layout,
    /// Trim blank borders automatically.
    pub crop: CropMode,
    /// Fixed margins cut from every page before the automatic trim.
    pub margins: Margins,
    /// 0..=100: higher notices fainter marks, so trims less.
    pub trim_sensitivity: u8,
    /// Pixels kept around the content after trimming.
    pub trim_padding: u8,
    pub spread: SpreadMode,
    /// 0..=100, 50 = unchanged.
    pub contrast: u8,
    pub auto_levels: bool,
    /// How a long strip is cut where it has no blank gap.
    pub strip_cut: StripCut,
    /// With the older cut, how much of a page repeats on the next.
    pub overlap_percent: u8,
    pub dither: Dither,
    /// Left out of the saved file while upright, so books converted before
    /// this setting existed keep their cached pages.
    #[serde(skip_serializing_if = "Orientation::is_portrait")]
    pub orientation: Orientation,
    /// How far pages are zoomed in from Fit. Left out of the saved file at
    /// Fit, like `orientation`.
    #[serde(skip_serializing_if = "PageZoom::is_fit")]
    pub zoom: PageZoom,
    /// What happens to the part that no longer fits: pieces, or a crop.
    #[serde(skip_serializing_if = "ZoomMode::is_tile")]
    pub zoom_mode: ZoomMode,
    /// Draw an arrow on a page whose next page carries on from it. Left out
    /// of the saved file while off, like `orientation`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub guide_arrows: bool,
    /// An EPUB or CBZ for a colour reader is made in colour unless this is
    /// set: the user chose black and white. Kept (and ignored) when the book
    /// is an XTC or XTCH, or the reader is not colour, so choosing those and
    /// coming back does not lose it. Left out of the saved file while off, so
    /// files from before colour existed load as "colour where it can be".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub black_and_white: bool,
}

impl Default for ConversionSettings {
    /// The default profile: XTC (black and white), which readers show quickly
    /// and which is half the size, and long strips cut with repeated rows so
    /// nothing is cut off. XTCH (4 grays) is there to choose, but readers are
    /// slow with it.
    fn default() -> Self {
        Self {
            format: Format::Xtc,
            epub: false,
            cbz: false,
            direction: ReadDirection::RightToLeft,
            layout: Layout::Auto,
            crop: CropMode::Margins,
            margins: Margins::default(),
            trim_sensitivity: DEFAULT_TRIM_SENSITIVITY,
            trim_padding: DEFAULT_TRIM_PADDING,
            spread: SpreadMode::Split,
            contrast: 55,
            auto_levels: true,
            strip_cut: StripCut::Overlap,
            overlap_percent: 8,
            dither: Dither::FloydSteinberg,
            orientation: Orientation::Portrait,
            zoom: PageZoom::Fit,
            zoom_mode: ZoomMode::Tile,
            guide_arrows: false,
            black_and_white: false,
        }
    }
}

/// One setting of [`ConversionSettings`], named, so a series can remember
/// which ones it changed from its preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversionField {
    Format,
    Direction,
    Layout,
    Crop,
    Margins,
    TrimSensitivity,
    TrimPadding,
    Spread,
    Contrast,
    AutoLevels,
    StripCut,
    OverlapPercent,
    Dither,
    Orientation,
    Zoom,
    GuideArrows,
}

impl ConversionField {
    pub const ALL: [ConversionField; 16] = [
        ConversionField::Format,
        ConversionField::Direction,
        ConversionField::Layout,
        ConversionField::Crop,
        ConversionField::Margins,
        ConversionField::TrimSensitivity,
        ConversionField::TrimPadding,
        ConversionField::Spread,
        ConversionField::Contrast,
        ConversionField::AutoLevels,
        ConversionField::StripCut,
        ConversionField::OverlapPercent,
        ConversionField::Dither,
        ConversionField::Orientation,
        ConversionField::Zoom,
        ConversionField::GuideArrows,
    ];
}

impl ConversionSettings {
    /// What the book is made as.
    pub fn output(&self) -> OutputFormat {
        match (self.cbz, self.epub, self.format) {
            (true, _, _) => OutputFormat::Cbz,
            (false, true, _) => OutputFormat::Epub,
            (false, false, Format::Xtc) => OutputFormat::Xtc,
            (false, false, Format::Xtch) => OutputFormat::Xtch,
        }
    }

    /// Choose what the book is made as.
    pub fn set_output(&mut self, output: OutputFormat) {
        self.cbz = false;
        match output {
            OutputFormat::Xtc => (self.epub, self.format) = (false, Format::Xtc),
            OutputFormat::Xtch => (self.epub, self.format) = (false, Format::Xtch),
            OutputFormat::Epub => self.epub = true,
            OutputFormat::Cbz => (self.epub, self.cbz) = (false, true),
        }
    }

    /// Whether pages are made as pictures the size of the reader's screen (an
    /// EPUB or a CBZ) rather than packed into an XTC or XTCH book.
    pub fn pictures(&self) -> bool {
        self.epub || self.cbz
    }

    /// Whether the colour choice applies at all: the book is an EPUB or CBZ
    /// (the only formats that carry colour) and `device` shows colour. The
    /// Export screen offers "Black & white | Color" exactly then.
    pub fn offers_color(&self, device: &DeviceProfile) -> bool {
        device.color && self.pictures()
    }

    /// Whether the pages of this book are kept in colour on `device`. This is
    /// the one place that decides it, for the preview and the export alike:
    /// never for an XTC or XTCH, never for a gray screen, and otherwise unless
    /// the user chose black and white.
    pub fn keeps_color(&self, device: &DeviceProfile) -> bool {
        self.offers_color(device) && !self.black_and_white
    }

    /// The settings in which `self` and `other` differ.
    pub fn differing(&self, other: &ConversionSettings) -> Vec<ConversionField> {
        ConversionField::ALL
            .into_iter()
            .filter(|&f| {
                let mut probe = self.clone();
                probe.copy_field(other, f);
                probe != *self
            })
            .collect()
    }

    /// Take one setting from `from`.
    pub fn copy_field(&mut self, from: &ConversionSettings, field: ConversionField) {
        match field {
            ConversionField::Format => {
                self.format = from.format;
                self.epub = from.epub;
                self.cbz = from.cbz;
            }
            ConversionField::Direction => self.direction = from.direction,
            ConversionField::Layout => self.layout = from.layout,
            ConversionField::Crop => self.crop = from.crop,
            ConversionField::Margins => self.margins = from.margins,
            ConversionField::TrimSensitivity => self.trim_sensitivity = from.trim_sensitivity,
            ConversionField::TrimPadding => self.trim_padding = from.trim_padding,
            ConversionField::Spread => self.spread = from.spread,
            ConversionField::Contrast => self.contrast = from.contrast,
            ConversionField::AutoLevels => self.auto_levels = from.auto_levels,
            ConversionField::StripCut => self.strip_cut = from.strip_cut,
            ConversionField::OverlapPercent => self.overlap_percent = from.overlap_percent,
            ConversionField::Dither => self.dither = from.dither,
            ConversionField::Orientation => self.orientation = from.orientation,
            ConversionField::Zoom => {
                self.zoom = from.zoom;
                self.zoom_mode = from.zoom_mode;
            }
            ConversionField::GuideArrows => self.guide_arrows = from.guide_arrows,
        }
    }
}

/// How hard downloads push on a source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadSpeed {
    /// Careful: for sources that block easily.
    Gentle,
    #[default]
    Normal,
    Fast,
    /// The most chapters and pages at once, all from the one best source, so
    /// every chapter stays from the same translation.
    Ultra,
    /// The user's own numbers.
    Custom,
}

/// What a download may do at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadLimits {
    /// Chapters being downloaded at the same time.
    pub chapters: usize,
    /// Page requests in flight at the same time, across all chapters.
    pub pages: usize,
    /// Download different chapters from different sources at the same time
    /// when several offer them.
    pub spread_sources: bool,
    /// Ask every source that has a chapter at once and keep the first to
    /// finish. `pages` is then the allowance of each source.
    pub race_sources: bool,
    /// Keep a page's colour when it is shrunk to save space (an RGB JPEG
    /// instead of a gray one). Pages kept as downloaded are untouched either way.
    pub keep_color: bool,
}

impl DownloadLimits {
    pub const MAX_CHAPTERS: u8 = 10;
    pub const MAX_PAGES: u8 = 100;
}

impl Default for DownloadLimits {
    fn default() -> Self {
        DownloadSettings::default().limits()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    pub speed: DownloadSpeed,
    /// Used when `speed` is Custom.
    pub custom_chapters: u8,
    pub custom_pages: u8,
    /// Download different chapters from different sources at the same time.
    /// Adds the sources' speeds together, but chapters can then come from
    /// different translations. Off unless the user turns it on, in any speed.
    pub spread_sources: bool,
    /// Ask every source that has a chapter at once and keep the first to
    /// finish. Fastest per chapter, and chapters can then come from different
    /// translations. Off unless the user turns it on, in any speed.
    pub race_sources: bool,
    /// Keep colour in the pages that are shrunk to save space ("Save" and
    /// "Ultra save"), so a colour reader can show them in colour. Off, they
    /// are saved as gray JPEGs, which are smaller. A page already saved gray
    /// cannot get its colour back; download it again.
    pub keep_color: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            speed: DownloadSpeed::Ultra,
            custom_chapters: 4,
            custom_pages: 24,
            spread_sources: false,
            race_sources: false,
            keep_color: false,
        }
    }
}

impl DownloadSettings {
    /// The numbers behind the choice, from measuring real sources: gains
    /// level off at about 50 requests in flight, some sources gain nothing at
    /// all (they cap themselves), and none pushed back even at 200.
    ///
    /// Mixing sources is only done when the user has switched it on, because it
    /// can mix translations within one series; it works with any speed.
    pub fn limits(&self) -> DownloadLimits {
        let (chapters, pages) = match self.speed {
            DownloadSpeed::Gentle => (1, 2),
            DownloadSpeed::Normal => (2, 6),
            DownloadSpeed::Fast => (4, 24),
            DownloadSpeed::Ultra => (8, 48),
            DownloadSpeed::Custom => (
                self.custom_chapters.clamp(1, DownloadLimits::MAX_CHAPTERS) as usize,
                self.custom_pages.clamp(1, DownloadLimits::MAX_PAGES) as usize,
            ),
        };
        DownloadLimits {
            chapters,
            pages,
            spread_sources: self.spread_sources,
            race_sources: self.race_sources,
            keep_color: self.keep_color,
        }
    }
}

/// How files get from this computer to a reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportMethod {
    /// Over a cable (or the SD card put in this computer): the drive is chosen
    /// when exporting.
    #[default]
    Usb,
    /// Over Wi-Fi, to the reader's own file transfer.
    WiFi,
}

/// What belongs to one reader about getting files onto it. Kept per reader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderSetup {
    /// Files can be sent over a cable or from its SD card.
    pub usb: bool,
    /// Files can be sent over Wi-Fi.
    pub wifi: bool,
    /// The reader's address on Wi-Fi, for [`ExportMethod::WiFi`].
    pub wifi_url: Option<String>,
    /// The folder files go into on the reader, for USB and Wi-Fi.
    pub folder: String,
    /// The drive (or a folder on it) chosen for USB, as last used.
    pub reader_dir: Option<PathBuf>,
    /// The file types the reader opens, such as "XTC" and "XTCH".
    pub formats: Vec<String>,
    /// What the reader runs.
    pub firmware: Firmware,
}

impl ReaderSetup {
    /// The ways this reader takes files, USB first.
    pub fn methods(&self) -> Vec<ExportMethod> {
        let mut all = Vec::new();
        if self.usb {
            all.push(ExportMethod::Usb);
        }
        if self.wifi {
            all.push(ExportMethod::WiFi);
        }
        all
    }

    /// Whether `method` is one of the ways this reader takes files.
    pub fn allows(&self, method: ExportMethod) -> bool {
        match method {
            ExportMethod::Usb => self.usb,
            ExportMethod::WiFi => self.wifi,
        }
    }
}

impl Default for ReaderSetup {
    fn default() -> Self {
        Self {
            usb: true,
            wifi: false,
            wifi_url: None,
            folder: "Manga".into(),
            reader_dir: None,
            formats: vec!["XTC".into(), "XTCH".into()],
            firmware: Firmware::CrossPoint,
        }
    }
}

/// How far the first-run setup has got. It is kept, so closing the app part
/// way through picks up where it stopped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupStep {
    /// Installing the catalog engine.
    #[default]
    Engine,
    /// Choosing sources.
    Sources,
    /// Choosing or making a reader.
    Reader,
    /// Finished (or never needed: the app was already in use).
    Done,
}

fn setup_done() -> SetupStep {
    SetupStep::Done
}

fn not_yet() -> bool {
    false
}

/// How much of a page is kept when it is downloaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageQuality {
    /// As small as is still easy to read on a small e-ink screen: about a
    /// tenth of the full size.
    Ultra,
    /// Tall pages are shrunk to twice the reader's screen, which loses nothing
    /// you can see. Pages already that small are left as downloaded.
    #[default]
    Save,
    /// Exactly as downloaded.
    Full,
}

/// How the app moves between pages and parts of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Animations {
    /// Pages and their parts come up one after another, with fades.
    #[default]
    Clean,
    /// Almost no waiting for anything to move: pages just appear.
    Snappy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    /// Where finished files are written. `None` means the app's default folder.
    pub output_dir: Option<PathBuf>,
    /// The reader's drive (or a folder on it), as last chosen: files sent
    /// "on my reader" go into `device_folder` inside it.
    pub reader_dir: Option<PathBuf>,
    /// Folder created on the device's storage for copied files.
    pub device_folder: String,
    /// Base URL of the reader's WebDAV share, if the user set one up.
    pub webdav_url: Option<String>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            output_dir: None,
            reader_dir: None,
            device_folder: "Manga".into(),
            webdav_url: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Id of the device pages are made for: a built-in or one of
    /// `custom_devices`. Files from before devices were data hold the old
    /// enum's name, which is mapped on load.
    #[serde(deserialize_with = "device_id")]
    pub device: String,
    /// The user's readers. None to begin with: they are added from the known
    /// ones, or made.
    pub custom_devices: Vec<DeviceProfile>,
    /// First-run setup progress. Files from before it existed count as done.
    #[serde(default = "setup_done")]
    pub setup_step: SetupStep,
    /// Whether the readers that used to be built in were turned into readers
    /// of the user's own. Files from before that did not have them yet.
    #[serde(default = "not_yet")]
    pub readers_listed: bool,
    /// Whether the readers copied from the list of known ones were checked for
    /// a colour screen, once. Readers added before colour was known have no
    /// such mark, though the list says that (say) the Kobo Libra Colour has one.
    #[serde(default = "not_yet")]
    pub color_screens_checked: bool,
    /// The preset a device starts from, by device id.
    pub device_presets: BTreeMap<String, String>,
    pub firmware: Firmware,
    /// Show every conversion setting, not just the few that matter most.
    pub advanced_conversion: bool,
    pub conversion: ConversionSettings,
    pub export: ExportSettings,
    /// Allow Suwayomi's embedded Chromium (needed by a few sources).
    pub browser_support: bool,
    /// How much of each page is kept when it is downloaded.
    pub page_quality: PageQuality,
    /// Show a system notification when a download or an export finishes.
    pub notifications: bool,
    /// Look for a newer MangaPoint when the app starts (at most once a day).
    pub update_check: bool,
    /// When the app last looked for a newer version (seconds since the Unix epoch).
    pub update_last_check: u64,
    /// A version the user chose to skip. It is not offered again; a newer one is.
    pub update_skipped: String,
    /// Clean (fades, one part after another) or snappy (pages just appear).
    pub animations: Animations,
    /// The reader's bars (chapter, settings, title) stay on screen instead of hiding while reading.
    pub reader_bars_always: bool,
    /// How each reader receives files, by device id.
    pub reader_setups: BTreeMap<String, ReaderSetup>,
    /// How many chapters and pages to download at once.
    pub download: DownloadSettings,
    /// Where new downloads are kept. `None` means the app's own folder.
    pub library_dir: Option<PathBuf>,
    /// Other folders downloads were kept in before, so they stay available.
    pub library_dirs: Vec<PathBuf>,
    /// Ask where to keep the files each time something is downloaded.
    pub ask_download_folder: bool,
    /// The preset the default conversion settings started from, if any.
    pub default_preset_id: Option<String>,
    /// Preferred source language for search and ranking.
    pub language: String,
    /// Source ids the user prefers when the same series exists in several.
    pub preferred_sources: Vec<String>,
    /// Sources turned off for searching. Everything not listed is searched,
    /// so a source installed later is searched too.
    pub search_excluded_sources: Vec<String>,
}

impl Settings {
    /// How the chosen reader receives files. A reader without a setup of its
    /// own starts from what was kept before readers had one.
    pub fn reader_setup(&self) -> ReaderSetup {
        self.reader_setups.get(&self.device_profile().id).cloned().unwrap_or_else(|| ReaderSetup {
            wifi_url: self.export.webdav_url.clone(),
            folder: self.export.device_folder.clone(),
            reader_dir: self.export.reader_dir.clone(),
            firmware: self.firmware,
            ..ReaderSetup::default()
        })
    }

    /// Change how the chosen reader receives files.
    pub fn update_reader_setup(&mut self, change: impl FnOnce(&mut ReaderSetup)) {
        let mut setup = self.reader_setup();
        change(&mut setup);
        self.reader_setups.insert(self.device_profile().id, setup);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: String::new(),
            custom_devices: Vec::new(),
            setup_step: SetupStep::Engine,
            readers_listed: true,
            color_screens_checked: true,
            device_presets: BTreeMap::new(),
            firmware: Firmware::CrossPoint,
            advanced_conversion: false,
            conversion: ConversionSettings::default(),
            export: ExportSettings::default(),
            browser_support: false,
            page_quality: PageQuality::Save,
            notifications: true,
            update_check: true,
            update_last_check: 0,
            update_skipped: String::new(),
            animations: Animations::Clean,
            reader_bars_always: false,
            reader_setups: BTreeMap::new(),
            download: DownloadSettings::default(),
            library_dir: None,
            library_dirs: Vec::new(),
            ask_download_folder: true,
            default_preset_id: None,
            language: "en".into(),
            preferred_sources: Vec::new(),
            search_excluded_sources: Vec::new(),
        }
    }
}

fn device_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = String::deserialize(d)?;
    Ok(device_profiles::legacy_id(&raw).map_or(raw, str::to_string))
}

impl Settings {
    /// The chosen reader. With none chosen (or the chosen one removed) it is
    /// the first of the user's, and with none at all the Xteink X4 Pro, so
    /// pages can still be made.
    pub fn device_profile(&self) -> DeviceProfile {
        device_profiles::find(&self.device, &self.custom_devices)
            .or_else(|| self.devices().into_iter().next())
            .unwrap_or_else(device_profiles::x4_pro)
    }

    /// The readers the user has.
    pub fn devices(&self) -> Vec<DeviceProfile> {
        device_profiles::all(&self.custom_devices)
    }

    /// Readers the user added before colour screens were known are copies of
    /// the list's readers without the mark: give a colour one its mark, once.
    /// After that the mark is the user's to change (a reader form checkbox).
    pub fn mark_color_readers(&mut self) {
        if self.color_screens_checked {
            return;
        }
        for device in &mut self.custom_devices {
            if device_profiles::known(&device.id).is_some_and(|k| k.color) {
                device.color = true;
            }
        }
        self.color_screens_checked = true;
    }

    /// Whether new downloads keep the colour of the pages they shrink: the
    /// user turned it on, or the reader in use has a colour screen (so Color
    /// export is not silently gray with the default settings).
    pub fn keeps_color_in_downloads(&self) -> bool {
        self.download.keep_color || self.device_profile().color
    }

    /// What a download may do at once, with colour kept as
    /// [`Settings::keeps_color_in_downloads`] says.
    pub fn download_limits(&self) -> DownloadLimits {
        DownloadLimits {
            keep_color: self.keeps_color_in_downloads(),
            ..self.download.limits()
        }
    }

    /// Settings from before readers were all the user's own: the three that
    /// used to be built in become theirs, once.
    pub fn list_old_readers(&mut self) {
        self.mark_color_readers();
        if self.readers_listed {
            return;
        }
        // The three readers that used to be built in (the others were added later).
        let old = [device_profiles::X4_PRO_ID, device_profiles::X4_ID, device_profiles::X3_ID];
        for known in old.iter().rev().filter_map(|id| device_profiles::known(id)) {
            if !self.custom_devices.iter().any(|d| d.id == known.id) {
                self.custom_devices.insert(0, known);
            }
        }
        self.readers_listed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_is_not_saved_so_cached_pages_stay_valid_and_sideways_is() {
        let mut c = ConversionSettings::default();
        assert!(!serde_json::to_string(&c).unwrap().contains("orientation"));
        c.orientation = Orientation::Landscape;
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains("\"orientation\":\"Landscape\""));
        let back: ConversionSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(back.orientation, Orientation::Landscape);
        // A settings file from before the setting existed is upright.
        let old: ConversionSettings = serde_json::from_str("{\"contrast\":60}").unwrap();
        assert_eq!(old.orientation, Orientation::Portrait);
    }

    #[test]
    fn fit_is_not_saved_and_a_zoom_is_held_to_what_the_converter_allows() {
        let mut c = ConversionSettings::default();
        assert!(!serde_json::to_string(&c).unwrap().contains("zoom"));
        c.zoom = PageZoom::Percent(150);
        c.zoom_mode = ZoomMode::Crop;
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains("\"zoom_mode\":\"Crop\"") && saved.contains("150"), "{saved}");
        let back: ConversionSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!((back.zoom, back.zoom_mode), (PageZoom::Percent(150), ZoomMode::Crop));
        c.zoom = PageZoom::Width;
        let back: ConversionSettings = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.zoom, PageZoom::Width);
        assert_eq!(PageZoom::Percent(5000).clamped(), PageZoom::Percent(300));
        assert_eq!(PageZoom::Percent(20).clamped(), PageZoom::Fit);
        assert_eq!(PageZoom::Percent(100).clamped(), PageZoom::Fit);
        let old: ConversionSettings = serde_json::from_str("{\"contrast\":60}").unwrap();
        assert_eq!((old.zoom, old.zoom_mode), (PageZoom::Fit, ZoomMode::Tile));
        let mut other = ConversionSettings::default();
        other.copy_field(&c, ConversionField::Zoom);
        assert_eq!((other.zoom, other.zoom_mode), (PageZoom::Width, ZoomMode::Crop));
    }

    #[test]
    fn guide_arrows_are_off_by_default_unsaved_while_off_and_copied_with_their_field() {
        let mut c = ConversionSettings::default();
        assert!(!c.guide_arrows && !serde_json::to_string(&c).unwrap().contains("guide"));
        c.guide_arrows = true;
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains("\"guide_arrows\":true"), "{saved}");
        let back: ConversionSettings = serde_json::from_str(&saved).unwrap();
        assert!(back.guide_arrows);
        let old: ConversionSettings = serde_json::from_str("{\"contrast\":60}").unwrap();
        assert!(!old.guide_arrows);
        assert_eq!(c.differing(&ConversionSettings::default()), vec![ConversionField::GuideArrows]);
        let mut other = ConversionSettings::default();
        other.copy_field(&c, ConversionField::GuideArrows);
        assert!(other.guide_arrows);
    }

    #[test]
    fn colour_is_offered_for_pictures_on_a_colour_screen_and_black_and_white_is_the_choice_not_to() {
        let colour = device_profiles::known("kobo-libra-colour").unwrap();
        let mono = device_profiles::known("kobo-libra-2").unwrap();
        let xteink = device_profiles::x4_pro();
        let mut c = ConversionSettings::default();
        assert!(!c.black_and_white && !serde_json::to_string(&c).unwrap().contains("black"));
        for (output, pictures) in [
            (OutputFormat::Xtc, false),
            (OutputFormat::Xtch, false),
            (OutputFormat::Epub, true),
            (OutputFormat::Cbz, true),
        ] {
            c.set_output(output);
            // Colour by default, on a colour screen, in a format that carries it.
            assert_eq!(c.offers_color(&colour), pictures, "{output:?}");
            assert_eq!(c.keeps_color(&colour), pictures, "{output:?}");
            // Never on a gray screen.
            assert!(!c.offers_color(&mono) && !c.keeps_color(&mono), "{output:?}");
            assert!(!c.keeps_color(&xteink), "{output:?}");
        }
        // Choosing black and white is kept when the format changes and back.
        c.set_output(OutputFormat::Epub);
        c.black_and_white = true;
        assert!(c.offers_color(&colour) && !c.keeps_color(&colour));
        c.set_output(OutputFormat::Xtc);
        assert!(!c.keeps_color(&colour));
        assert!(c.black_and_white, "the choice is ignored, not deleted");
        c.set_output(OutputFormat::Cbz);
        assert!(!c.keeps_color(&colour));
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains("\"black_and_white\":true"), "{saved}");
        let back: ConversionSettings = serde_json::from_str(&saved).unwrap();
        assert!(back.black_and_white);
        // Settings from before colour existed load as "colour where it can be".
        let old: ConversionSettings = serde_json::from_str("{\"contrast\":60,\"epub\":true}").unwrap();
        assert!(!old.black_and_white && old.keeps_color(&colour));
    }

    #[test]
    fn settings_written_before_colour_still_load_and_known_colour_readers_are_marked_once() {
        // A reader of the user's own, written before it had a colour mark.
        let old = r#"{"device":"kobo-libra-colour","custom_devices":[
            {"id":"kobo-libra-colour","name":"Kobo Libra Colour","width":1264,"height":1680,"ppi":300,"maker":"Rakuten Kobo","gray_levels":16},
            {"id":"custom-mine","name":"Mine","width":600,"height":800}]}"#;
        let mut s: Settings = serde_json::from_str(old).unwrap();
        assert!(s.custom_devices.iter().all(|d| !d.color), "no mark in an old file means gray");
        assert!(!s.color_screens_checked);
        s.list_old_readers();
        let by_id = |s: &Settings, id: &str| s.custom_devices.iter().find(|d| d.id == id).unwrap().color;
        assert!(by_id(&s, "kobo-libra-colour"), "the list says it is a colour screen");
        assert!(!by_id(&s, "custom-mine"), "a reader of the user's own is theirs to mark");
        // Once: unticking the box is not undone at the next start.
        s.custom_devices.iter_mut().find(|d| d.id == "kobo-libra-colour").unwrap().color = false;
        s.list_old_readers();
        assert!(!by_id(&s, "kobo-libra-colour"));
        // The mark is saved and read back; new settings need no check.
        s.custom_devices.iter_mut().find(|d| d.id == "custom-mine").unwrap().color = true;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let marks = |s: &Settings| s.custom_devices.iter().map(|d| (d.id.clone(), d.color)).collect::<Vec<_>>();
        assert_eq!(marks(&back), marks(&s));
        assert!(Settings::default().color_screens_checked);
        // And a file with none of the new fields at all.
        let bare: Settings = serde_json::from_str("{\"language\":\"ja\"}").unwrap();
        assert!(!bare.download.keep_color && !bare.conversion.black_and_white);
    }

    #[test]
    fn downloads_keep_colour_when_asked_or_while_a_colour_reader_is_in_use() {
        let mut s = Settings::default();
        // Nobody chosen: the fallback reader is gray, and the switch is off.
        assert!(!s.keeps_color_in_downloads() && !s.download_limits().keep_color);
        let add = |s: &mut Settings, id: &str| {
            let reader = device_profiles::known(id).unwrap();
            s.custom_devices.push(reader);
            s.device = id.to_string();
        };
        add(&mut s, "kobo-libra-2");
        assert!(!s.keeps_color_in_downloads(), "a gray reader keeps the switch's value");
        add(&mut s, "kobo-libra-colour");
        assert!(s.keeps_color_in_downloads() && s.download_limits().keep_color);
        assert!(!s.download.keep_color, "the switch itself is not changed");
        // Back to a gray reader: off again, unless the switch is on.
        s.device = "kobo-libra-2".into();
        assert!(!s.keeps_color_in_downloads());
        s.download.keep_color = true;
        assert!(s.keeps_color_in_downloads() && s.download_limits().keep_color);
        // The rest of the limits are the usual ones.
        let plain = DownloadSettings::default().limits();
        let mut mine = Settings::default();
        mine.download.keep_color = true;
        assert_eq!((mine.download_limits().chapters, mine.download_limits().pages), (plain.chapters, plain.pages));
        // A colour reader the user unmarked is a gray reader.
        add(&mut s, "kobo-clara-colour");
        s.download.keep_color = false;
        s.custom_devices.last_mut().unwrap().color = false;
        assert!(!s.keeps_color_in_downloads());
    }

    #[test]
    fn keeping_colour_in_downloads_is_off_by_default_and_reaches_the_download_limits() {
        let mut d = DownloadSettings::default();
        assert!(!d.keep_color && !d.limits().keep_color);
        let old: DownloadSettings = serde_json::from_str("{\"speed\":\"Fast\"}").unwrap();
        assert!(!old.keep_color);
        d.keep_color = true;
        assert!(d.limits().keep_color);
        let back: DownloadSettings = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(back.keep_color);
    }

    #[test]
    fn a_series_remembers_when_its_orientation_differs_from_the_preset() {
        let base = ConversionSettings::default();
        let mut sideways = base.clone();
        sideways.orientation = Orientation::Landscape;
        assert_eq!(sideways.differing(&base), vec![ConversionField::Orientation]);
        let mut copy = base.clone();
        copy.copy_field(&sideways, ConversionField::Orientation);
        assert_eq!(copy.orientation, Orientation::Landscape);
    }

    #[test]
    fn the_output_is_xtc_xtch_or_epub() {
        let mut c = ConversionSettings::default();
        assert_eq!(c.output(), OutputFormat::Xtc);
        c.set_output(OutputFormat::Xtch);
        assert_eq!((c.output(), c.format, c.epub), (OutputFormat::Xtch, Format::Xtch, false));
        c.set_output(OutputFormat::Epub);
        assert_eq!(c.output(), OutputFormat::Epub);
        // Going back to a page format remembers nothing of the EPUB.
        c.set_output(OutputFormat::Xtc);
        assert_eq!((c.output(), c.epub), (OutputFormat::Xtc, false));
        // Taking the format from a preset takes the EPUB choice with it.
        let mut epub = ConversionSettings::default();
        epub.set_output(OutputFormat::Epub);
        let mut other = ConversionSettings::default();
        other.copy_field(&epub, ConversionField::Format);
        assert_eq!(other.output(), OutputFormat::Epub);
        assert_eq!(other.differing(&epub), Vec::new());
        // A CBZ is its own choice, and choosing anything else drops it.
        c.set_output(OutputFormat::Cbz);
        assert_eq!((c.output(), c.epub, c.cbz, c.pictures()), (OutputFormat::Cbz, false, true, true));
        c.set_output(OutputFormat::Epub);
        assert_eq!((c.output(), c.cbz, c.pictures()), (OutputFormat::Epub, false, true));
        c.set_output(OutputFormat::Cbz);
        c.set_output(OutputFormat::Xtch);
        assert_eq!((c.output(), c.cbz, c.pictures()), (OutputFormat::Xtch, false, false));
        let mut cbz = ConversionSettings::default();
        cbz.set_output(OutputFormat::Cbz);
        let mut other = ConversionSettings::default();
        other.copy_field(&cbz, ConversionField::Format);
        assert_eq!(other.output(), OutputFormat::Cbz);
        assert_eq!(OutputFormat::Cbz.extension(), "cbz");
        // Files from before EPUB existed have no such choice.
        let old: ConversionSettings = serde_json::from_str(r#"{"format":"Xtch"}"#).unwrap();
        assert_eq!(old.output(), OutputFormat::Xtch);
    }

    #[test]
    fn defaults_follow_the_plan() {
        let s = Settings::default();
        assert!(s.devices().is_empty(), "no readers to begin with");
        assert_eq!(s.device_profile().id, device_profiles::X4_PRO_ID, "pages can still be made");
        assert_eq!(s.setup_step, SetupStep::Engine);
        assert_eq!(s.conversion.direction, ReadDirection::RightToLeft);
        assert_eq!(s.conversion.contrast, 55);
        assert_eq!(s.conversion.overlap_percent, 8);
        assert_eq!(s.conversion.dither, Dither::FloydSteinberg);
        assert!(!s.browser_support, "browser support is opt-in");
        assert!(
            s.search_excluded_sources.is_empty(),
            "every source is searched until the user says otherwise"
        );
        assert_eq!(
            s.page_quality,
            PageQuality::Save,
            "pages are shrunk when they are far larger than a reader screen, by default"
        );
    }

    #[test]
    fn download_speeds_map_to_limits_and_custom_numbers_are_held_in_range() {
        let plain = |chapters, pages| DownloadLimits {
            chapters,
            pages,
            spread_sources: false,
            race_sources: false,
            keep_color: false,
        };
        let mut d = DownloadSettings::default();
        assert_eq!(
            d.speed,
            DownloadSpeed::Ultra,
            "the fastest mode is the default"
        );
        assert_eq!(d.limits(), plain(8, 48));
        d.speed = DownloadSpeed::Gentle;
        assert_eq!(d.limits(), plain(1, 2));
        d.speed = DownloadSpeed::Normal;
        assert_eq!(d.limits(), plain(2, 6));
        d.speed = DownloadSpeed::Fast;
        assert_eq!(d.limits(), plain(4, 24));
        d.speed = DownloadSpeed::Custom;
        d.custom_chapters = 0;
        d.custom_pages = 250;
        assert_eq!(d.limits(), plain(1, 100));
        d.custom_chapters = 99;
        d.custom_pages = 0;
        assert_eq!(d.limits(), plain(10, 1));
    }

    #[test]
    fn mixing_sources_follows_its_own_switches_in_every_speed() {
        let speeds = [
            DownloadSpeed::Gentle,
            DownloadSpeed::Normal,
            DownloadSpeed::Fast,
            DownloadSpeed::Ultra,
            DownloadSpeed::Custom,
        ];
        // Off by default, whatever the speed.
        let mut d = DownloadSettings::default();
        for speed in speeds {
            d.speed = speed;
            let l = d.limits();
            assert!(!l.spread_sources && !l.race_sources, "{speed:?}");
        }
        // Switched on, it applies to every speed, not just Advanced.
        d.spread_sources = true;
        d.race_sources = true;
        for speed in speeds {
            d.speed = speed;
            let l = d.limits();
            assert!(l.spread_sources && l.race_sources, "{speed:?}");
        }
        // And each switch is its own.
        d.race_sources = false;
        let l = d.limits();
        assert!(l.spread_sources && !l.race_sources);
    }

    #[test]
    fn differing_names_exactly_the_changed_settings() {
        let a = ConversionSettings::default();
        assert!(a.differing(&a).is_empty());
        let mut b = a.clone();
        b.contrast = 80;
        b.margins.top = 5;
        assert_eq!(
            a.differing(&b),
            vec![ConversionField::Margins, ConversionField::Contrast]
        );
        let mut c = a.clone();
        c.copy_field(&b, ConversionField::Contrast);
        assert_eq!(c.contrast, 80);
        assert_eq!(c.margins, a.margins, "only the named setting is copied");
    }

    #[test]
    fn partial_files_fill_in_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{"language":"ja","conversion":{"contrast":70}}"#).unwrap();
        assert_eq!(s.language, "ja");
        assert_eq!(s.conversion.contrast, 70);
        assert_eq!(s.conversion.overlap_percent, 8);
        assert_eq!(s.export.device_folder, "Manga");
    }

    #[test]
    fn old_device_names_and_missing_devices_are_handled() {
        let s: Settings = serde_json::from_str(r#"{"device":"XteinkX4"}"#).unwrap();
        assert_eq!(s.device, device_profiles::X4_ID);
        assert_eq!(s.device_profile().width, 480);
        let gone: Settings = serde_json::from_str(r#"{"device":"custom-removed"}"#).unwrap();
        assert_eq!(gone.device_profile().id, device_profiles::X4_PRO_ID);
    }

    #[test]
    fn old_files_keep_their_readers_and_skip_the_first_run_setup() {
        let mut old: Settings = serde_json::from_str(r#"{"device":"xteink-x4"}"#).unwrap();
        assert_eq!(old.setup_step, SetupStep::Done);
        assert!(old.devices().is_empty());
        old.list_old_readers();
        assert_eq!(old.devices().len(), 3);
        assert_eq!(old.device_profile().id, device_profiles::X4_ID);
        old.list_old_readers();
        assert_eq!(old.devices().len(), 3, "only once");
        // A reader removed afterwards stays removed.
        old.custom_devices.retain(|d| d.id != device_profiles::X3_ID);
        old.list_old_readers();
        assert_eq!(old.devices().len(), 2);
        let mut fresh = Settings::default();
        fresh.list_old_readers();
        assert!(fresh.devices().is_empty());
    }

    #[test]
    fn custom_devices_round_trip_and_are_selectable() {
        let mut s = Settings::default();
        let dev = DeviceProfile::custom(&s.devices(), "Kobo Nia", 758, 1024).unwrap();
        s.device = dev.id.clone();
        s.custom_devices.push(dev);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(
            (back.device_profile().width, back.device_profile().height),
            (758, 1024)
        );
        assert!(!back.device_profile().builtin);
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut s = Settings::default();
        s.conversion.format = Format::Xtc;
        s.export.webdav_url = Some("http://192.168.3.3/".into());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }

    #[test]
    fn a_reader_has_its_own_way_of_receiving_files() {
        let mut s = Settings::default();
        s.custom_devices = vec![
            DeviceProfile::custom(&[], "Mine", 480, 800).unwrap(),
            DeviceProfile::custom(&[], "Other", 480, 800).unwrap(),
        ];
        s.device = s.custom_devices[0].id.clone();
        s.export.device_folder = "Books".into();
        s.export.webdav_url = Some("http://192.168.3.3".into());
        // Without a setup of its own, the older settings are what it starts from.
        let first = s.reader_setup();
        assert_eq!(first.methods(), [ExportMethod::Usb]);
        assert_eq!(first.folder, "Books");
        assert_eq!(first.wifi_url.as_deref(), Some("http://192.168.3.3"));
        assert_eq!(first.formats, ["XTC", "XTCH"]);
        s.update_reader_setup(|r| {
            r.wifi = true;
            r.folder = "Manga".into();
        });
        assert_eq!(s.reader_setup().methods(), [ExportMethod::Usb, ExportMethod::WiFi]);
        // Another reader is not affected.
        s.device = s.custom_devices[1].id.clone();
        assert_eq!(s.reader_setup().methods(), [ExportMethod::Usb]);
        // It is kept in the file and comes back.
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reader_setups.len(), 1);
        assert!(serde_json::from_str::<Settings>("{}").unwrap().reader_setups.is_empty());
    }
}
