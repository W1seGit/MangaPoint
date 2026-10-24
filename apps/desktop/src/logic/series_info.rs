//! What the Library keeps about a series: its details and a small cover.

/// The biggest a saved cover is kept: posters are shown at most 200 px wide, so
/// this is sharp on a high-density screen, fills an e-reader screen when it is
/// the cover page of an export, and still takes only tens of kilobytes.
const COVER_MAX: (u32, u32) = (540, 810);
/// Covers larger than this are shrunk when they are next read.
pub const COVER_KEEP_BYTES: usize = 160 * 1024;

/// The most bytes and pixels a picture may have to be shrunk into a cover (the
/// pixel limit is the one the converter applies to pages): a bomb of a picture
/// is refused from its header, before anything is allocated for it.
const COVER_MAX_BYTES: usize = 64 * 1024 * 1024;
const COVER_MAX_PIXELS: u64 = pipeline::import::MAX_PAGE_PIXELS;

/// A cover as a small JPEG, to keep on disk instead of what the source sent.
/// `None` if the bytes are not an image, or are too large to be one.
pub fn small_cover(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > COVER_MAX_BYTES {
        return None;
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if u64::from(w) * u64::from(h) > COVER_MAX_PIXELS {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > COVER_MAX.0 || img.height() > COVER_MAX.1 {
        img.resize(
            COVER_MAX.0,
            COVER_MAX.1,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&img.to_rgb8())
        .ok()?;
    Some(out)
}

/// Turn what a source says about a series into what the Library keeps.
pub fn series_info_from(
    details: &catalog::MangaDetails,
    source_names: Vec<(String, String)>,
    has_cover: bool,
    now: u64,
) -> library::SeriesInfo {
    let clean = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or("").to_string();
    let mut genres: Vec<String> = Vec::new();
    for g in &details.genre {
        let g = g.trim();
        if !g.is_empty() && !genres.iter().any(|x| x.eq_ignore_ascii_case(g)) {
            genres.push(g.to_string());
        }
    }
    // Descriptions often arrive with Windows line breaks and long runs of blank lines.
    let mut description = clean(&details.description).replace("\r\n", "\n");
    while description.contains("\n\n\n") {
        description = description.replace("\n\n\n", "\n\n");
    }
    library::SeriesInfo {
        description,
        author: clean(&details.author),
        artist: clean(&details.artist),
        status: library::status_label(details.status.as_deref().unwrap_or("")),
        genres,
        source_names,
        has_cover,
        saved_at: now,
    }
}

/// How many chapters make a series slow to load: the sources send the whole
/// list at once, so a long one takes a while.
pub const LONG_SERIES: usize = 500;

/// What to say while the chapters of a long series load, when how many it has
/// is known; nothing for a short series or when the count is not known.
pub fn long_series_note(chapters: Option<usize>) -> Option<String> {
    let n = chapters.filter(|n| *n > LONG_SERIES)?;
    // "about 3,000": rounded, with a comma in the thousands.
    let step = if n >= 1000 { 100 } else { 50 };
    let rounded = (n + step / 2) / step * step;
    let text = if rounded >= 1000 {
        format!("{},{:03}", rounded / 1000, rounded % 1000)
    } else {
        rounded.to_string()
    };
    Some(format!(
        "This series has about {text} chapters, so it can take a while to load them from your sources."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details() -> catalog::MangaDetails {
        catalog::MangaDetails {
            id: 1,
            title: "Series".into(),
            description: Some("  First.\r\nSecond.\n\n\n\nThird.  ".into()),
            author: Some(" A. Writer ".into()),
            artist: None,
            genre: vec![
                "Action".into(),
                " action ".into(),
                "".into(),
                "Fantasy".into(),
            ],
            status: Some("ONGOING".into()),
            thumbnail_url: Some("/cover".into()),
        }
    }

    #[test]
    fn series_details_are_tidied_for_display() {
        let info = series_info_from(&details(), vec![("1".into(), "Src".into())], true, 99);
        assert_eq!(info.description, "First.\nSecond.\n\nThird.");
        assert_eq!(info.author, "A. Writer");
        assert_eq!(info.artist, "");
        assert_eq!(info.status, "Ongoing");
        assert_eq!(info.genres, ["Action", "Fantasy"], "no blanks or repeats");
        assert!(info.has_cover);
        assert_eq!(info.saved_at, 99);
        assert_eq!(info.source_name("1"), Some("Src"));
    }

    #[test]
    fn missing_details_become_empty_not_errors() {
        let info = series_info_from(&catalog::MangaDetails::default(), Vec::new(), false, 0);
        assert_eq!(info.description, "");
        assert_eq!(info.status, "");
        assert!(info.genres.is_empty());
        assert!(!info.has_cover);
    }

    #[test]
    fn a_long_series_gets_a_note_and_a_short_one_does_not() {
        assert_eq!(long_series_note(None), None);
        assert_eq!(long_series_note(Some(500)), None);
        assert!(long_series_note(Some(501)).unwrap().contains("about 500 chapters"));
        assert!(long_series_note(Some(3_187)).unwrap().contains("about 3,200 chapters"));
        assert!(long_series_note(Some(12_000)).unwrap().contains("about 12,000 chapters"));
    }

    #[test]
    fn a_cover_is_kept_small() {
        let mut big = image::RgbImage::new(1200, 1800);
        for (x, y, p) in big.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8]);
        }
        let mut png = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let small = small_cover(&png).expect("an image shrinks");
        let decoded = image::load_from_memory(&small).unwrap();
        assert!(decoded.width() <= 540 && decoded.height() <= 810);
        assert_eq!(decoded.width() * 3, decoded.height() * 2, "shape kept");
        assert!(
            small.len() < png.len() / 5,
            "{} vs {}",
            small.len(),
            png.len()
        );
        assert!(small_cover(b"not an image").is_none());
    }

    /// A PNG that says it is `w` x `h` and has little else of a picture.
    fn png_header_only(w: u32, h: u32) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = 0xffff_ffffu32;
            for &byte in data {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
                }
            }
            !crc
        }
        let chunk = |kind: &[u8; 4], data: &[u8]| {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend(crc32(&body).to_be_bytes());
            out
        };
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 0, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &[0x78, 0x9c]));
        out
    }

    #[test]
    fn a_picture_too_large_to_be_a_cover_is_refused_from_its_header() {
        // 400 megapixels: it would be a gigabyte of pixels to decode for a thumbnail.
        assert!(small_cover(&png_header_only(20_000, 20_000)).is_none());
        // Over the byte limit, whatever it says it is.
        let mut heavy = png_header_only(4, 4);
        heavy.resize(COVER_MAX_BYTES + 1, 0);
        assert!(small_cover(&heavy).is_none());
    }
}
