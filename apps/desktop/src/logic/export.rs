//! Where an export goes, and about how big it will be.

use library::Settings;
use pipeline::ExportTarget;
use std::path::Path;

/// Where the user chose to send the result.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetChoice {
    /// The configured output folder (or the app's default one).
    Folder,
    /// The reader's drive (or SD card).
    Usb,
    /// The reader over Wi-Fi.
    WiFi,
}

pub fn export_target(
    choice: &TargetChoice,
    settings: &Settings,
    default_dir: &Path,
) -> Result<ExportTarget, String> {
    match choice {
        TargetChoice::Folder => Ok(ExportTarget::Folder {
            path: settings
                .export
                .output_dir
                .clone()
                .unwrap_or_else(|| default_dir.to_path_buf()),
        }),
        TargetChoice::Usb => {
            let setup = settings.reader_setup();
            match setup.reader_dir {
                Some(dir) => Ok(ExportTarget::Device {
                    mount: dir,
                    folder: setup.folder,
                }),
                None => Err("Choose where the reader is first (under Send to).".into()),
            }
        }
        TargetChoice::WiFi => {
            let setup = settings.reader_setup();
            match setup.wifi_url.as_deref() {
                Some(url) if !url.trim().is_empty() => Ok(ExportTarget::WebDav {
                    base_url: url.trim().to_string(),
                    folder: setup.folder,
                }),
                _ => Err("Add the reader's Wi-Fi address first (under Send to).".into()),
            }
        }
    }
}

/// A rough size for a reader file: each page is a fixed size in the format
/// (about 48 KB for black and white, 96 KB for four grays). Split spreads make
/// it a little bigger.
pub fn estimate_export_bytes(pages: usize, format: converter::xtc::Format) -> u64 {
    let per_page: u64 = match format {
        converter::xtc::Format::Xtc => 48_022,
        converter::xtc::Format::Xtch => 96_022,
    };
    pages as u64 * per_page
}

/// A rough size of a book as `output`: the XTC and XTCH figures are exact for a
/// 480 x 800 screen; a page of an EPUB depends on the drawing, and about 24 KB
/// is what manga comes to.
pub fn estimate_output_bytes(pages: usize, output: library::OutputFormat) -> u64 {
    match output {
        library::OutputFormat::Xtc => estimate_export_bytes(pages, converter::xtc::Format::Xtc),
        library::OutputFormat::Xtch => estimate_export_bytes(pages, converter::xtc::Format::Xtch),
        library::OutputFormat::Epub => pages as u64 * 24_000,
        library::OutputFormat::Cbz => pages as u64 * 23_000,
    }
}

/// The size of a series' book, kept in colour or not: colour pages are
/// JPEGs of a picture, about five times a gray page.
pub fn estimate_series_bytes(
    pages: usize,
    settings: &library::ConversionSettings,
    device: &device_profiles::DeviceProfile,
) -> u64 {
    if settings.keeps_color(device) {
        pages as u64 * pipeline::epub::typical_color_page_bytes()
    } else {
        estimate_output_bytes(pages, settings.output())
    }
}

/// What the "Black & white | Color" control on the Export screen shows: `None`
/// where it is not offered (the book is an XTC or XTCH, or the reader's screen
/// is not colour), otherwise whether Color is the one chosen. A choice made
/// earlier is kept while the control is hidden and applies again when it is back.
pub fn color_choice(
    settings: &library::ConversionSettings,
    device: &device_profiles::DeviceProfile,
) -> Option<bool> {
    settings.offers_color(device).then_some(!settings.black_and_white)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colour_control_is_shown_only_for_a_colour_reader_and_a_format_that_carries_colour() {
        use library::{ConversionSettings, OutputFormat};
        let colour = device_profiles::known("kobo-libra-colour").unwrap();
        let gray = device_profiles::known("kobo-libra-2").unwrap();
        let xteink = device_profiles::x4_pro();
        let with = |output, black_and_white| {
            let mut c = ConversionSettings::default();
            c.set_output(output);
            c.black_and_white = black_and_white;
            c
        };
        for (output, shown) in [
            (OutputFormat::Xtc, false),
            (OutputFormat::Xtch, false),
            (OutputFormat::Epub, true),
            (OutputFormat::Cbz, true),
        ] {
            // Colour is what a colour reader starts on.
            assert_eq!(color_choice(&with(output, false), &colour), shown.then_some(true), "{output:?}");
            assert_eq!(color_choice(&with(output, true), &colour), shown.then_some(false), "{output:?}");
            assert_eq!(color_choice(&with(output, false), &gray), None, "{output:?} on a gray screen");
            assert_eq!(color_choice(&with(output, false), &xteink), None);
        }
        // The choice made for an EPUB comes back with the control.
        let mut c = with(OutputFormat::Epub, true);
        c.set_output(OutputFormat::Xtc);
        assert_eq!(color_choice(&c, &colour), None);
        c.set_output(OutputFormat::Cbz);
        assert_eq!(color_choice(&c, &colour), Some(false));
        // And the size guess follows what is made.
        let epub = with(OutputFormat::Epub, false);
        let bw = with(OutputFormat::Epub, true);
        assert!(estimate_series_bytes(100, &epub, &colour) > 2 * estimate_series_bytes(100, &bw, &colour));
        assert_eq!(estimate_series_bytes(100, &epub, &gray), estimate_series_bytes(100, &bw, &gray));
    }

    #[test]
    fn export_targets_follow_the_choice_and_settings() {
        let mut s = Settings::default();
        let default_dir = Path::new("/exports");
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/exports".into()
            }
        );
        s.export.output_dir = Some("/mine".into());
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/mine".into()
            }
        );
        assert!(export_target(&TargetChoice::Usb, &s, default_dir).is_err());
        s.update_reader_setup(|r| r.reader_dir = Some("E:\\".into()));
        assert_eq!(
            export_target(&TargetChoice::Usb, &s, default_dir).unwrap(),
            ExportTarget::Device {
                mount: "E:\\".into(),
                folder: "Manga".into()
            }
        );
        assert!(export_target(&TargetChoice::WiFi, &s, default_dir).is_err());
        s.update_reader_setup(|r| r.wifi_url = Some(" http://192.168.3.3 ".into()));
        assert_eq!(
            export_target(&TargetChoice::WiFi, &s, default_dir).unwrap(),
            ExportTarget::WebDav {
                base_url: "http://192.168.3.3".into(),
                folder: "Manga".into()
            }
        );
    }

    #[test]
    fn export_size_estimates_follow_the_format() {
        use converter::xtc::Format;
        assert_eq!(estimate_export_bytes(0, Format::Xtch), 0);
        assert_eq!(estimate_export_bytes(100, Format::Xtch), 9_602_200);
        assert_eq!(estimate_export_bytes(100, Format::Xtc), 4_802_200);
        // Four grays take about twice the room of black and white.
        assert!(
            estimate_export_bytes(10, Format::Xtch)
                > 2 * estimate_export_bytes(10, Format::Xtc) - 1000
        );
    }
}
