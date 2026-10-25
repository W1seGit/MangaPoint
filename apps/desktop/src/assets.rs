//! The app icon as files (`icon.png`, `icon.ico`) for installers and for
//! embedding in the Windows executable. Written by
//! `mangapoint --write-icons <folder>`.

use crate::ui::mascot::icon_rgba;
use std::path::Path;

/// An `.ico` holding one 256x256 image stored as PNG (supported by every
/// Windows since Vista).
pub fn ico_from_png(png: &[u8]) -> Vec<u8> {
    let mut ico = Vec::with_capacity(22 + png.len());
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    ico.extend_from_slice(&1u16.to_le_bytes()); // one image
    ico.extend_from_slice(&[0, 0, 0, 0]); // 256 wide, 256 high, no palette, reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // colour planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes()); // image data starts after the 22-byte header
    ico.extend_from_slice(png);
    ico
}

/// Write `icon.png` (256x256) and `icon.ico` into `dir`.
pub fn write_icons(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (rgba, w, h) = icon_rgba(4);
    let image = image::RgbaImage::from_raw(w, h, rgba).ok_or("the icon has the wrong size")?;
    let png_path = dir.join("icon.png");
    image.save(&png_path).map_err(|e| e.to_string())?;
    let png = std::fs::read(&png_path).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("icon.ico"), ico_from_png(&png)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ico_wraps_the_png_with_a_valid_header() {
        let png = b"\x89PNG fake payload";
        let ico = ico_from_png(png);
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 1, 0], "icon file with one image");
        assert_eq!(
            u32::from_le_bytes(ico[14..18].try_into().unwrap()) as usize,
            png.len()
        );
        assert_eq!(u32::from_le_bytes(ico[18..22].try_into().unwrap()), 22);
        assert_eq!(&ico[22..], png);
    }

    #[test]
    fn written_icons_are_a_real_png_and_ico() {
        let dir = std::env::temp_dir().join(format!("mangapoint-icons-{}", std::process::id()));
        write_icons(&dir).unwrap();
        let png = std::fs::read(dir.join("icon.png")).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let ico = std::fs::read(dir.join("icon.ico")).unwrap();
        assert_eq!(&ico[22..], &png[..]);
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        let _ = std::fs::remove_file(dir.join("icon.png"));
        let _ = std::fs::remove_file(dir.join("icon.ico"));
        let _ = std::fs::remove_dir(&dir);
    }
}
