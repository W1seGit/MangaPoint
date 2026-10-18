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

#[cfg(test)]
mod tests {
    use super::*;

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
