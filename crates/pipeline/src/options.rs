//! Mapping saved settings onto the converter's options.

use converter::{Depth, ProcessOptions};
use device_profiles::DeviceProfile;
use library::{ConversionSettings, Settings};

/// Everything the converter needs for pages shown on `device`, converted the
/// way `conversion` says. Previews and exports both call this, so what you
/// see is what gets written.
pub fn process_options_for(
    device: &DeviceProfile,
    conversion: &ConversionSettings,
) -> ProcessOptions {
    ProcessOptions {
        width: device.width,
        height: device.height,
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
