//! Target e-reader devices and what we know about their file support.
//!
//! This is data, not logic. The built-in devices are listed here; users can
//! add their own (any page size) and those travel in the settings file. When
//! hardware testing settles an open question (see [`Support`]), only the
//! tables here change.

/// Firmware running on the device.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Firmware {
    /// The firmware the device ships with.
    Stock,
    /// The open-source CrossPoint Reader firmware.
    CrossPoint,
}

/// How well we know that a device/firmware combination opens XTC/XTCH files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Stated in the firmware's own documentation or source.
    Documented,
    /// Widely reported by the community, not documented by the vendor.
    CommunityReported,
    /// Nothing found either way; needs a hardware test.
    Unverified,
}

/// Smallest and largest page edge, in pixels. The XTC page table stores sizes
/// as 16-bit numbers; the lower bound just keeps a typo from making a device
/// no page can be made for.
pub const MIN_EDGE: u32 = 64;
pub const MAX_EDGE: u32 = 4096;

/// A device's screen and what we know about its file support.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Stable key, kept in settings files and series.
    pub id: String,
    pub name: String,
    /// Width of the page as the reader shows it, in pixels (portrait).
    pub width: u32,
    pub height: u32,
    /// Pixels per inch of the screen: what turns its size in inches into the
    /// size in pixels the pages are made at.
    #[cfg_attr(feature = "serde", serde(default = "default_ppi"))]
    pub ppi: u32,
    /// Who makes it, for a reader from the list the app knows; empty for one
    /// of your own.
    #[cfg_attr(feature = "serde", serde(default))]
    pub maker: String,
    /// How many grays the screen shows: 4 on the Xteink readers, 16 on most others.
    #[cfg_attr(feature = "serde", serde(default = "default_grays"))]
    pub gray_levels: u8,
    /// Ships with the app, so it cannot be edited or removed.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub builtin: bool,
    /// What we know about XTC support on this device.
    #[cfg_attr(feature = "serde", serde(skip, default = "unknown_support"))]
    pub support: SupportTable,
}

/// Support per firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportTable {
    pub stock: Support,
    pub crosspoint: Support,
}

/// The density of the Xteink screens, used until a reader says otherwise.
pub const DEFAULT_PPI: u32 = 220;

#[cfg(feature = "serde")]
fn default_ppi() -> u32 {
    DEFAULT_PPI
}

#[cfg(feature = "serde")]
fn default_grays() -> u8 {
    16
}

#[cfg(feature = "serde")]
fn unknown_support() -> SupportTable {
    SupportTable::UNKNOWN
}

impl SupportTable {
    /// A device we have never seen: nothing is known either way.
    pub const UNKNOWN: SupportTable = SupportTable {
        stock: Support::Unverified,
        crosspoint: Support::Unverified,
    };
}

pub const X4_ID: &str = "xteink-x4";
pub const X4_PRO_ID: &str = "xteink-x4-pro";
pub const X3_ID: &str = "xteink-x3";

/// What the app knows about a reader it lists, besides its screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnownInfo {
    /// The company that makes it.
    pub maker: &'static str,
    /// The screen's diagonal in inches, as the maker gives it.
    pub diagonal: f32,
    /// The file types it opens, of those MangaPoint can make: XTC, XTCH, EPUB, CBZ.
    pub formats: &'static [&'static str],
    /// Whether the CrossPoint firmware runs on it. Where it does not, the
    /// reader runs only its own (stock) firmware.
    pub crosspoint: bool,
    /// Something worth knowing about getting files onto it.
    pub note: &'static str,
}

struct Entry {
    id: &'static str,
    name: &'static str,
    width: u32,
    height: u32,
    ppi: u32,
    grays: u8,
    info: KnownInfo,
}

#[allow(clippy::too_many_arguments)]
const fn entry(
    id: &'static str,
    maker: &'static str,
    name: &'static str,
    (width, height, ppi, diagonal): (u32, u32, u32, f32),
    grays: u8,
    formats: &'static [&'static str],
    crosspoint: bool,
    note: &'static str,
) -> Entry {
    Entry {
        id,
        name,
        width,
        height,
        ppi,
        grays,
        info: KnownInfo {
            maker,
            diagonal,
            formats,
            crosspoint,
            note,
        },
    }
}

/// The readers the app knows, with the screen sizes and densities their makers
/// publish. The Xteink readers come first. Sizes are portrait, in pixels.
const KNOWN: &[Entry] = &[
    entry("xteink-x4-pro", "Xteink", "Xteink X4 Pro", (480, 800, 220, 4.3), 4, &["XTC", "XTCH", "EPUB"], true, ""),
    entry("xteink-x4", "Xteink", "Xteink X4", (480, 800, 220, 4.3), 4, &["XTC", "XTCH", "EPUB"], true, ""),
    entry("xteink-x3", "Xteink", "Xteink X3", (528, 792, 256, 3.7), 4, &["XTC", "XTCH", "EPUB"], true, ""),
    entry("kindle-paperwhite-12", "Amazon", "Kindle Paperwhite (12th gen)", (1264, 1680, 300, 7.0), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-paperwhite-11", "Amazon", "Kindle Paperwhite (11th gen)", (1236, 1648, 300, 6.8), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-paperwhite-10", "Amazon", "Kindle Paperwhite (10th gen)", (1072, 1448, 300, 6.0), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-11", "Amazon", "Kindle (11th gen, 2022)", (1072, 1448, 300, 6.0), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-10", "Amazon", "Kindle (10th gen, 2019)", (600, 800, 167, 6.0), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-oasis-3", "Amazon", "Kindle Oasis (3rd gen)", (1264, 1680, 300, 7.0), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kindle-scribe", "Amazon", "Kindle Scribe", (1860, 2480, 300, 10.2), 16, &["EPUB"], false, "Kindles open EPUB files sent with Send to Kindle (the app or e-mail); copying one over USB does not work."),
    entry("kobo-clara-bw", "Rakuten Kobo", "Kobo Clara BW", (1072, 1448, 300, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-clara-colour", "Rakuten Kobo", "Kobo Clara Colour", (1072, 1448, 300, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-clara-2e", "Rakuten Kobo", "Kobo Clara 2E", (1072, 1448, 300, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-libra-2", "Rakuten Kobo", "Kobo Libra 2", (1264, 1680, 300, 7.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-libra-colour", "Rakuten Kobo", "Kobo Libra Colour", (1264, 1680, 300, 7.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-sage", "Rakuten Kobo", "Kobo Sage", (1440, 1920, 300, 8.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-elipsa-2e", "Rakuten Kobo", "Kobo Elipsa 2E", (1404, 1872, 227, 10.3), 16, &["EPUB", "CBZ"], false, ""),
    entry("kobo-nia", "Rakuten Kobo", "Kobo Nia", (758, 1024, 212, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-palma", "Onyx Boox", "Boox Palma", (824, 1648, 300, 6.13), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-go-6", "Onyx Boox", "Boox Go 6", (1072, 1448, 300, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-page", "Onyx Boox", "Boox Page", (1264, 1680, 300, 7.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-go-10-3", "Onyx Boox", "Boox Go 10.3", (1404, 1872, 227, 10.3), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-note-air3", "Onyx Boox", "Boox Note Air3", (1404, 1872, 227, 10.3), 16, &["EPUB", "CBZ"], false, ""),
    entry("boox-tab-ultra", "Onyx Boox", "Boox Tab Ultra", (1404, 1872, 227, 10.3), 16, &["EPUB", "CBZ"], false, ""),
    entry("pocketbook-verse", "PocketBook", "PocketBook Verse", (758, 1024, 212, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("pocketbook-verse-pro", "PocketBook", "PocketBook Verse Pro", (1072, 1448, 300, 6.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("pocketbook-era", "PocketBook", "PocketBook Era", (1264, 1680, 300, 7.0), 16, &["EPUB", "CBZ"], false, ""),
    entry("pocketbook-inkpad-4", "PocketBook", "PocketBook InkPad 4", (1404, 1872, 300, 7.8), 16, &["EPUB", "CBZ"], false, ""),
    entry("remarkable-2", "reMarkable", "reMarkable 2", (1404, 1872, 226, 10.3), 16, &["EPUB"], false, ""),
];

fn profile_of(e: &Entry) -> DeviceProfile {
    // What is known of XTC support, where it has been looked into.
    let support = match e.id {
        X4_ID => SupportTable {
            stock: Support::CommunityReported,
            crosspoint: Support::Documented,
        },
        X4_PRO_ID => SupportTable {
            stock: Support::Unverified,
            crosspoint: Support::Documented,
        },
        _ => SupportTable::UNKNOWN,
    };
    DeviceProfile {
        id: e.id.into(),
        name: e.name.into(),
        width: e.width,
        height: e.height,
        ppi: e.ppi,
        maker: e.info.maker.into(),
        gray_levels: e.grays,
        builtin: true,
        support,
    }
}

/// The 4.3" 480x800 panel in the X4. CrossPoint's README lists it and its
/// parser reads both formats; stock support is community-reported.
pub fn x4() -> DeviceProfile {
    known(X4_ID).expect("the X4 is in the list")
}

/// The same panel as the X4, with a touchscreen and front light. Xteink's own
/// spec lists only EPUB/TXT/JPG/BMP, so stock support is unverified.
pub fn x4_pro() -> DeviceProfile {
    known(X4_PRO_ID).expect("the X4 Pro is in the list")
}

/// The smaller Xteink X3, 528x792. Community converters target it; neither
/// firmware has been checked against it here.
pub fn x3() -> DeviceProfile {
    known(X3_ID).expect("the X3 is in the list")
}

/// The readers the app knows: what you can pick from when adding one. Adding
/// one copies it, so it is then yours to change or remove.
pub fn builtin() -> Vec<DeviceProfile> {
    KNOWN.iter().map(profile_of).collect()
}

/// What else is known of a listed reader.
pub fn known_info(id: &str) -> Option<KnownInfo> {
    KNOWN.iter().find(|e| e.id == id).map(|e| e.info)
}

/// A known reader by id, as the app lists it.
pub fn known(id: &str) -> Option<DeviceProfile> {
    KNOWN.iter().find(|e| e.id == id).map(profile_of)
}

/// How well `name` answers what was typed: `None` if it does not, otherwise
/// smaller is better. Case, spaces and punctuation do not matter, and every
/// word typed has to be in the name.
pub fn match_rank(query: &str, name: &str) -> Option<usize> {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let whole = squash(name);
    let q = squash(query);
    if q.is_empty() {
        return Some(0);
    }
    if let Some(at) = whole.find(&q) {
        return Some(at);
    }
    let words: Vec<String> = query.split_whitespace().map(squash).filter(|w| !w.is_empty()).collect();
    if !words.is_empty() && words.iter().all(|w| whole.contains(w.as_str())) {
        return Some(1000);
    }
    None
}

/// Why a custom device was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    EmptyName,
    NameTaken,
    BadSize,
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::EmptyName => write!(f, "give the device a name"),
            DeviceError::NameTaken => write!(f, "a device with that name already exists"),
            DeviceError::BadSize => write!(
                f,
                "width and height must each be between {MIN_EDGE} and {MAX_EDGE} pixels"
            ),
        }
    }
}

impl std::error::Error for DeviceError {}

impl DeviceProfile {
    /// A new user-made device. Support is unknown until the user has tried it.
    pub fn custom(
        existing: &[DeviceProfile],
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<DeviceProfile, DeviceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DeviceError::EmptyName);
        }
        let profile = DeviceProfile {
            id: fresh_id(existing, name),
            name: name.chars().take(40).collect(),
            width,
            height,
            ppi: DEFAULT_PPI,
            maker: String::new(),
            gray_levels: 16,
            builtin: false,
            support: SupportTable::UNKNOWN,
        };
        profile.validate(existing)?;
        Ok(profile)
    }

    /// Check size and name against the other devices in the list.
    pub fn validate(&self, others: &[DeviceProfile]) -> Result<(), DeviceError> {
        if self.name.trim().is_empty() {
            return Err(DeviceError::EmptyName);
        }
        let ok = |n: u32| (MIN_EDGE..=MAX_EDGE).contains(&n);
        if !ok(self.width) || !ok(self.height) {
            return Err(DeviceError::BadSize);
        }
        if others
            .iter()
            .any(|o| o.id != self.id && o.name.trim().eq_ignore_ascii_case(self.name.trim()))
        {
            return Err(DeviceError::NameTaken);
        }
        Ok(())
    }

    /// Whether this device/firmware pair is known to open `.xtc`/`.xtch` files.
    pub fn xtc_support(&self, firmware: Firmware) -> Support {
        match firmware {
            Firmware::Stock => self.support.stock,
            Firmware::CrossPoint => self.support.crosspoint,
        }
    }

    /// The screen's width and height in inches.
    pub fn size_inches(&self) -> (f64, f64) {
        let ppi = f64::from(self.ppi.max(1));
        (f64::from(self.width) / ppi, f64::from(self.height) / ppi)
    }

    /// `2.18 x 3.64 in`.
    pub fn inches_label(&self) -> String {
        let (w, h) = self.size_inches();
        format!("{w:.2} \u{d7} {h:.2} in")
    }

    /// `480 x 800`.
    pub fn size_label(&self) -> String {
        format!("{} \u{d7} {}", self.width, self.height)
    }
}

/// Find one of the user's readers.
pub fn find(id: &str, custom: &[DeviceProfile]) -> Option<DeviceProfile> {
    all(custom).into_iter().find(|p| p.id == id)
}

/// Every reader the user has.
pub fn all(custom: &[DeviceProfile]) -> Vec<DeviceProfile> {
    custom
        .iter()
        .cloned()
        .map(|mut c| {
            c.builtin = false;
            c
        })
        .collect()
}

/// Settings files written before devices became data stored the enum's name.
pub fn legacy_id(old: &str) -> Option<&'static str> {
    match old {
        "XteinkX4" => Some(X4_ID),
        "XteinkX4Pro" => Some(X4_PRO_ID),
        _ => None,
    }
}

fn fresh_id(existing: &[DeviceProfile], name: &str) -> String {
    let slug: String = name
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
    let base = format!("custom-{}", if slug.is_empty() { "device" } else { &slug });
    let taken = |id: &str| builtin().iter().chain(existing).any(|p| p.id == id);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("an unused id exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_readers_are_found_by_id_and_nobody_has_any_to_begin_with() {
        for p in builtin() {
            assert_eq!(known(&p.id), Some(p));
        }
        assert_eq!(known("nope"), None);
        assert_eq!(find(X4_ID, &[]), None);
        let mine = vec![x4()];
        assert_eq!(find(X4_ID, &mine).map(|p| p.builtin), Some(false));
    }

    #[test]
    fn typing_finds_the_closest_reader() {
        assert_eq!(match_rank("x4", "Xteink X4 Pro"), Some(6));
        assert!(match_rank("x4 pro", "Xteink X4 Pro").is_some());
        assert_eq!(match_rank("xteink", "Xteink X3"), Some(0));
        assert_eq!(match_rank("PRO x4", "Xteink X4 Pro"), Some(1000));
        assert_eq!(match_rank("kobo", "Xteink X4"), None);
        assert_eq!(match_rank("", "Anything"), Some(0));
    }

    #[test]
    fn every_listed_reader_is_consistent() {
        let all = builtin();
        assert!(all.len() >= 25, "{} readers", all.len());
        for (i, p) in all.iter().enumerate() {
            let info = known_info(&p.id).unwrap();
            // Ids and names are unique.
            assert!(all[i + 1..].iter().all(|o| o.id != p.id && o.name != p.name), "{}", p.name);
            // Width and height with the density come to the maker's diagonal.
            let (w, h) = p.size_inches();
            let diagonal = (w * w + h * h).sqrt();
            assert!(
                (diagonal - f64::from(info.diagonal)).abs() < 0.16,
                "{}: {diagonal:.2} in, said to be {}",
                p.name,
                info.diagonal
            );
            assert!(p.validate(&[]).is_ok(), "{}", p.name);
            assert!(!info.formats.is_empty() && info.formats.contains(&"EPUB"), "{}", p.name);
            // Only the Xteink readers take XTC and run CrossPoint.
            let xteink = info.maker == "Xteink";
            assert_eq!(info.crosspoint, xteink, "{}", p.name);
            assert_eq!(info.formats.contains(&"XTC"), xteink, "{}", p.name);
            assert_eq!(p.gray_levels, if xteink { 4 } else { 16 }, "{}", p.name);
        }
    }

    #[test]
    fn cbz_is_listed_for_the_readers_that_open_it_and_not_for_those_that_do_not() {
        let opens = |id: &str| known_info(id).unwrap().formats.contains(&"CBZ");
        for id in ["kobo-clara-bw", "kobo-libra-colour", "boox-palma", "boox-tab-ultra", "pocketbook-era"] {
            assert!(opens(id), "{id} opens CBZ");
        }
        // Kindle takes no CBZ, reMarkable is better served by PDF, and the Xteink
        // firmware is not documented to read it.
        for id in ["kindle-paperwhite-12", "kindle-scribe", "remarkable-2", "xteink-x4", "xteink-x4-pro", "xteink-x3"] {
            assert!(!opens(id), "{id} is not listed for CBZ");
        }
    }

    #[test]
    fn the_popular_readers_are_there_with_the_sizes_their_makers_give() {
        let size = |id: &str| known(id).map(|p| (p.width, p.height, p.ppi));
        assert_eq!(size("kindle-paperwhite-12"), Some((1264, 1680, 300)));
        assert_eq!(size("kindle-paperwhite-11"), Some((1236, 1648, 300)));
        assert_eq!(size("kobo-libra-2"), Some((1264, 1680, 300)));
        assert_eq!(size("kobo-elipsa-2e"), Some((1404, 1872, 227)));
        assert_eq!(size("boox-palma"), Some((824, 1648, 300)));
        assert_eq!(size("remarkable-2"), Some((1404, 1872, 226)));
        assert!(known_info(X4_ID).unwrap().crosspoint);
        assert!(!known_info("kobo-sage").unwrap().crosspoint);
    }

    #[test]
    fn screens_are_described_in_inches() {
        let (w, h) = x4_pro().size_inches();
        assert_eq!((format!("{w:.2}"), format!("{h:.2}")), ("2.18".into(), "3.64".into()));
        assert_eq!(x4().inches_label(), "2.18 \u{d7} 3.64 in");
        // The X3's 528x792 at 256 ppi is the 3.7 inch screen it is sold with.
        let (w, h) = x3().size_inches();
        assert!(((w * w + h * h).sqrt() - 3.7).abs() < 0.05);
    }

    #[test]
    fn sizes_match_the_spec_sheets() {
        let size = |id| known(id).map(|p| (p.width, p.height));
        assert_eq!(size(X4_ID), Some((480, 800)));
        assert_eq!(size(X4_PRO_ID), Some((480, 800)), "same panel as the X4");
        assert_eq!(size(X3_ID), Some((528, 792)));
    }

    #[test]
    fn stock_x4_pro_is_not_claimed_to_work() {
        let p = x4_pro();
        assert_eq!(p.xtc_support(Firmware::Stock), Support::Unverified);
        assert_eq!(p.xtc_support(Firmware::CrossPoint), Support::Documented);
    }

    #[test]
    fn custom_devices_get_unique_ids_and_unknown_support() {
        let a = DeviceProfile::custom(&[], "My Reader", 600, 1024).unwrap();
        assert_eq!(a.id, "custom-my-reader");
        assert!(!a.builtin);
        assert_eq!(a.xtc_support(Firmware::CrossPoint), Support::Unverified);
        let b = DeviceProfile::custom(std::slice::from_ref(&a), "My  Reader!", 600, 1024);
        // Different name text but the same slug: still gets its own id.
        assert_eq!(b.unwrap().id, "custom-my-reader-2");
    }

    #[test]
    fn custom_devices_are_checked() {
        let e = |r: Result<DeviceProfile, DeviceError>| r.unwrap_err();
        assert_eq!(
            e(DeviceProfile::custom(&[], "  ", 480, 800)),
            DeviceError::EmptyName
        );
        assert_eq!(
            e(DeviceProfile::custom(&[], "A", 10, 800)),
            DeviceError::BadSize
        );
        assert_eq!(
            e(DeviceProfile::custom(&[], "A", 480, 9000)),
            DeviceError::BadSize
        );
        assert_eq!(
            e(DeviceProfile::custom(&[x4()], "xteink x4", 480, 800)),
            DeviceError::NameTaken,
            "two readers cannot share a name"
        );
    }

    #[test]
    fn old_settings_names_map_to_ids() {
        assert_eq!(legacy_id("XteinkX4Pro"), Some(X4_PRO_ID));
        assert_eq!(legacy_id("XteinkX4"), Some(X4_ID));
        assert_eq!(legacy_id("other"), None);
    }
}
