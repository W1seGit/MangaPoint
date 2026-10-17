//! Mapping saved settings onto the converter's options.

use converter::{Depth, ProcessOptions};
use device_profiles::DeviceProfile;
use library::{ConversionSettings, Orientation, Settings};

/// Everything the converter needs for pages shown on `device`, converted the
/// way `conversion` says. Previews and exports both call this, so what you
/// see is what gets written.
pub fn process_options_for(
    device: &DeviceProfile,
    conversion: &ConversionSettings,
) -> ProcessOptions {
    // Held sideways, the pages are laid out on the sideways screen. Pictures
    // (EPUB, CBZ) stay that shape and the reader rotates them; an XTC or XTCH
    // page is always the reader's own frame, so it is turned into it.
    let sideways = conversion.orientation == Orientation::Landscape;
    ProcessOptions {
        width: if sideways { device.height } else { device.width },
        height: if sideways { device.width } else { device.height },
        layout: conversion.layout,
        crop: conversion.crop,
        margins: conversion.margins,
        trim_sensitivity: conversion.trim_sensitivity,
        trim_padding: conversion.trim_padding,
        spread: conversion.spread,
        direction: conversion.direction,
        contrast: conversion.contrast,
        auto_levels: conversion.auto_levels,
        strip_cut: conversion.strip_cut,
        overlap_percent: conversion.overlap_percent,
        dither: conversion.dither,
        depth: output_depth(device, conversion),
        turn: sideways && !conversion.pictures(),
    }
}

/// How many grays the pages are made with. An XTC or XTCH book is as its
/// format says. An EPUB follows the reader's screen: four grays (dithered the
/// way the settings say) for a reader that shows four, and the full grays,
/// which it shows as it can, for one that shows more.
pub fn output_depth(device: &DeviceProfile, conversion: &ConversionSettings) -> Depth {
    if conversion.pictures() {
        if device.gray_levels <= 4 {
            Depth::Two
        } else {
            Depth::Eight
        }
    } else {
        conversion.format.depth()
    }
}

/// The same, using the saved defaults.
pub fn process_options(settings: &Settings) -> ProcessOptions {
    process_options_for(&settings.device_profile(), &settings.conversion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::xtc::Format;
    use converter::Depth;

    #[test]
    fn follows_device_and_format() {
        let mut s = Settings::default();
        let o = process_options(&s);
        assert_eq!((o.width, o.height), (480, 800));
        assert_eq!(o.depth, Depth::One, "default is XTC");
        assert_eq!(o.contrast, 55);

        s.conversion.format = Format::Xtch;
        assert_eq!(process_options(&s).depth, Depth::Two);
    }

    #[test]
    fn held_sideways_an_xtc_page_is_laid_out_wide_and_turned_into_the_readers_frame() {
        let mut s = Settings::default();
        s.conversion.orientation = library::Orientation::Landscape;
        let o = process_options(&s);
        assert_eq!((o.width, o.height), (800, 480));
        assert!(o.turn);
        assert_eq!(o.output_size(), (480, 800), "the file still holds the reader's own frame");

        s.conversion.orientation = library::Orientation::Portrait;
        let o = process_options(&s);
        assert_eq!((o.width, o.height, o.turn), (480, 800, false));
    }

    #[test]
    fn held_sideways_pictures_are_wide_and_not_turned() {
        for output in [library::OutputFormat::Epub, library::OutputFormat::Cbz] {
            let mut c = ConversionSettings::default();
            c.set_output(output);
            c.orientation = library::Orientation::Landscape;
            let o = process_options_for(&Settings::default().device_profile(), &c);
            assert_eq!((o.width, o.height, o.turn), (800, 480, false), "{output:?}");
            assert_eq!(o.output_size(), (800, 480));
        }
    }

    #[test]
    fn an_epub_follows_the_readers_screen() {
        let mut c = ConversionSettings::default();
        c.set_output(library::OutputFormat::Epub);
        let xteink = device_profiles::x4_pro();
        let kindle = device_profiles::known("kindle-paperwhite-12").unwrap();
        assert_eq!(output_depth(&xteink, &c), Depth::Two);
        assert_eq!(output_depth(&kindle, &c), Depth::Eight);
        assert_eq!((process_options_for(&kindle, &c).width, process_options_for(&kindle, &c).height), (1264, 1680));
        // XTC and XTCH ignore the screen.
        c.set_output(library::OutputFormat::Xtc);
        assert_eq!(output_depth(&kindle, &c), Depth::One);
        c.set_output(library::OutputFormat::Xtch);
        assert_eq!(output_depth(&kindle, &c), Depth::Two);
    }

    #[test]
    fn per_series_settings_override_the_defaults() {
        let conversion = ConversionSettings {
            contrast: 80,
            format: Format::Xtc,
            ..Default::default()
        };
        let o = process_options_for(&device_profiles::x4(), &conversion);
        assert_eq!(o.contrast, 80);
        assert_eq!(o.depth, Depth::One);
    }
}
