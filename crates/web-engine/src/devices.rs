//! The readers the site knows, from the same table the desktop app uses
//! (`device-profiles`), plus the built-in "Phone or tablet · original quality"
//! entry that keeps pages exactly as they are.

use device_profiles::{DeviceProfile, PHONE_ORIGINAL_ID};
use serde::Serialize;

pub use crate::options::default_options_json;

/// One device as the page lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub maker: String,
    /// Portrait pixels.
    pub width: u32,
    pub height: u32,
    pub ppi: u32,
    /// How many grays the screen shows.
    pub grays: u8,
    /// The file types it opens, as `"XTC" | "XTCH" | "EPUB" | "CBZ" | "PDF"`.
    pub formats: Vec<String>,
    /// The screen shows colour.
    pub color: bool,
    /// Runs the open-source CrossPoint firmware.
    pub crosspoint: bool,
    /// Something worth knowing about getting files onto it.
    pub note: String,
    /// The "phone or tablet" entry: pages are kept as they are.
    pub original: bool,
}

/// The file types a device opens, first the one to start with.
pub fn formats_of(device: &DeviceProfile) -> Vec<&'static str> {
    if device.id == PHONE_ORIGINAL_ID {
        return vec!["CBZ", "PDF"];
    }
    device_profiles::known_info(&device.id)
        .map(|i| i.formats.to_vec())
        .unwrap_or_else(|| vec!["XTC", "XTCH", "EPUB", "CBZ"])
}

/// Describe one device for the page.
pub fn info_of(device: &DeviceProfile) -> DeviceInfo {
    let known = device_profiles::known_info(&device.id);
    DeviceInfo {
        id: device.id.clone(),
        name: device.name.clone(),
        maker: device.maker.clone(),
        width: device.width,
        height: device.height,
        ppi: device.ppi,
        grays: device.gray_levels,
        formats: formats_of(device).into_iter().map(String::from).collect(),
        color: device.color,
        crosspoint: known.is_some_and(|k| k.crosspoint),
        note: known.map(|k| k.note.to_string()).unwrap_or_default(),
        original: device.id == PHONE_ORIGINAL_ID,
    }
}

/// Every device, the Xteink readers first and the phone entry last.
pub fn devices() -> Vec<DeviceInfo> {
    let mut all = device_profiles::builtin();
    all.push(device_profiles::phone_original());
    all.iter().map(info_of).collect()
}

/// `devices_json()`: the list as JSON text.
pub fn devices_json() -> String {
    serde_json::to_string(&devices()).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_every_reader_and_the_phone_entry_last() {
        let list = devices();
        assert_eq!(list.len(), device_profiles::builtin().len() + 1);
        assert_eq!(list[0].id, "xteink-x4-pro");
        assert_eq!(list[0].formats, ["XTC", "XTCH", "EPUB"]);
        assert!(list[0].crosspoint && !list[0].color);
        let phone = list.last().unwrap();
        assert!(phone.original && phone.color);
        assert_eq!(phone.name, "Phone or tablet \u{b7} original quality");
        assert_eq!(phone.formats, ["CBZ", "PDF"]);
        let kindle = list.iter().find(|d| d.id == "kindle-colorsoft").unwrap();
        assert!(kindle.color && kindle.note.contains("Send to Kindle"));
        let ids: std::collections::HashSet<_> = list.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids.len(), list.len(), "ids are unique");
    }

    #[test]
    fn the_json_names_are_the_contracts() {
        let v: serde_json::Value = serde_json::from_str(&devices_json()).unwrap();
        let first = &v[0];
        for key in [
            "id", "name", "maker", "width", "height", "ppi", "grays", "formats", "color",
            "crosspoint", "note", "original",
        ] {
            assert!(first.get(key).is_some(), "{key}");
        }
    }
}
