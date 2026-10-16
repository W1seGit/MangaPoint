//! User preferences.

use converter::process::{
    CropMode, Layout, Margins, PanelFinder, PanelView, SpreadMode, StripCut, DEFAULT_TRIM_PADDING,
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
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 3] = [OutputFormat::Xtc, OutputFormat::Xtch, OutputFormat::Epub];

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Xtc => "xtc",
            OutputFormat::Xtch => "xtch",
            OutputFormat::Epub => "epub",
        }
    }

    /// Its name as shown, and as listed among a reader's file formats.
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Xtc => "XTC",
            OutputFormat::Xtch => "XTCH",
            OutputFormat::Epub => "EPUB",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionSettings {
    /// The page format of an XTC or XTCH book. An EPUB ignores it.
    pub format: Format,
    /// Make an EPUB instead of an XTC or XTCH book.
    pub epub: bool,
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
    /// Ordinary pages shown panel by panel. Left out of the saved settings
    /// while off, so the converted chapters cached before it existed stay
    /// valid.
    #[serde(skip_serializing_if = "PanelView::is_off")]
    pub panel_view: PanelView,
    /// How the panels are found: the gaps between them, or the AI model.
    /// Left out while it is the gaps, for the same reason.
    #[serde(skip_serializing_if = "PanelFinder::is_gutters")]
    pub panel_finder: PanelFinder,
    pub dither: Dither,
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
            panel_view: PanelView::Off,
            panel_finder: PanelFinder::Gutters,
            dither: Dither::FloydSteinberg,
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
    PanelView,
    PanelFinder,
    Dither,
}

impl ConversionField {
    pub const ALL: [ConversionField; 15] = [
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
        ConversionField::PanelView,
        ConversionField::PanelFinder,
        ConversionField::Dither,
    ];
}

impl ConversionSettings {
    /// What the book is made as.
    pub fn output(&self) -> OutputFormat {
        match (self.epub, self.format) {
            (true, _) => OutputFormat::Epub,
            (false, Format::Xtc) => OutputFormat::Xtc,
            (false, Format::Xtch) => OutputFormat::Xtch,
        }
    }

    /// Choose what the book is made as.
    pub fn set_output(&mut self, output: OutputFormat) {
        match output {
            OutputFormat::Xtc => (self.epub, self.format) = (false, Format::Xtc),
            OutputFormat::Xtch => (self.epub, self.format) = (false, Format::Xtch),
            OutputFormat::Epub => self.epub = true,
        }
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
            ConversionField::PanelView => self.panel_view = from.panel_view,
            ConversionField::PanelFinder => self.panel_finder = from.panel_finder,
            ConversionField::Dither => self.dither = from.dither,
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
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            speed: DownloadSpeed::Ultra,
            custom_chapters: 4,
            custom_pages: 24,
            spread_sources: false,
            race_sources: false,
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
    /// Clean (fades, one part after another) or snappy (pages just appear).
    pub animations: Animations,
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
            device_presets: BTreeMap::new(),
            firmware: Firmware::CrossPoint,
            advanced_conversion: false,
            conversion: ConversionSettings::default(),
            export: ExportSettings::default(),
            browser_support: false,
            page_quality: PageQuality::Save,
            notifications: true,
            animations: Animations::Clean,
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

    /// Settings from before readers were all the user's own: the three that
    /// used to be built in become theirs, once.
    pub fn list_old_readers(&mut self) {
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
