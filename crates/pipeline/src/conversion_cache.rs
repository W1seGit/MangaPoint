//! One compact, replaceable conversion per downloaded chapter. Original
//! images remain the source of truth; a missing or damaged cache is a miss.

use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use converter::process::{CropMode, Layout, PageZoom, StripCut, ZoomMode};
use converter::xtc::{Format, PAGE_HEADER_SIZE};
use converter::{Depth, ProcessOptions};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// Bump this whenever packing or the cache's own layout changes. A change to
// what the converter makes bumps `converter::CONVERTER_REV` instead.
// Version 3 / 4: the key names the options that shape the bytes (not the
// whole saved settings) and the converter's revision.
const MAGIC: &[u8; 8] = b"MPCACHE3";
pub(crate) const FILE_NAME: &str = ".mangapoint-converted-v1.bin";
/// The same for an EPUB, whose page pictures are each their own length.
pub(crate) const EPUB_FILE_NAME: &str = ".mangapoint-converted-epub-v1.bin";
const PICTURES_MAGIC: &[u8; 8] = b"MPCACHE4";

/// What decides the bytes of a chapter's pages, as text: the conversion
/// options the pages are made with, less everything that cannot change them
/// for these settings. An EPUB and a CBZ share their pictures, the format
/// does not matter to pictures, the overlap does not matter to a quiet cut,
/// the trim does not matter to a page that keeps its whole picture, and a
/// zoom that comes to Fit is Fit.
pub(crate) fn cache_key(options: &ProcessOptions, pictures: bool) -> String {
    let mut o = options.clone();
    o.zoom = o.zoom.clamped();
    if o.zoom.is_fit() {
        o.zoom = PageZoom::Fit;
        o.zoom_mode = ZoomMode::Tile;
    }
    // A full-picture crop never looks for a margin. The strip still tells
    // blank rows from drawn ones by the sensitivity, unless pages are never a strip.
    if o.crop == CropMode::Full {
        o.trim_padding = 0;
        if o.layout == Layout::Manga {
            o.trim_sensitivity = 0;
        }
    }
    match o.layout {
        // Pages are never cut from a strip.
        Layout::Manga => {
            o.strip_cut = StripCut::Quiet;
            o.overlap_percent = 0;
        }
        // Strips have no spreads, zoom or guide arrows.
        Layout::Webtoon => {
            o.spread = converter::SpreadMode::Fit;
            o.zoom = PageZoom::Fit;
            o.zoom_mode = ZoomMode::Tile;
            o.guide_arrows = false;
        }
        _ => {}
    }
    if o.strip_cut == StripCut::Quiet {
        o.overlap_percent = 0;
    }
    // Full grays are not dithered.
    if o.depth == Depth::Eight {
        o.dither = converter::Dither::Threshold;
    }
    format!("{}|{o:?}", if pictures { "pictures" } else { "packed" })
}

fn fingerprint_header(key: &str, pages: usize) -> Sha256 {
    fingerprint_header_for(converter::CONVERTER_REV, key, pages)
}

fn fingerprint_header_for(rev: u32, key: &str, pages: usize) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(MAGIC);
    hash.update(rev.to_le_bytes());
    hash.update((key.len() as u64).to_le_bytes());
    hash.update(key.as_bytes());
    hash.update((pages as u64).to_le_bytes());
    hash
}

/// What stands for one source image in a chapter's fingerprint, so the image
/// itself can be let go as soon as it is read.
pub(crate) fn page_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
    hash.finalize().into()
}

/// On a miss, fingerprint the exact snapshot handed to the decoder (by the
/// digest of each image, in order). This avoids publishing a cache under an
/// earlier hash if a download replaces the sources between the cache lookup
/// and conversion.
pub(crate) fn fingerprint_digests(digests: &[[u8; 32]], key: &str) -> [u8; 32] {
    let mut hash = fingerprint_header(key, digests.len());
    for digest in digests {
        hash.update(digest);
    }
    hash.finalize().into()
}

#[cfg(test)]
pub(crate) fn fingerprint_bytes<'a>(
    pages: impl ExactSizeIterator<Item = &'a [u8]>,
    key: &str,
) -> [u8; 32] {
    let digests: Vec<[u8; 32]> = pages.map(page_digest).collect();
    fingerprint_digests(&digests, key)
}

/// Hash actual source contents (including their order), not just timestamps:
/// replacing an image with one of the same size must invalidate the cache.
pub(crate) fn fingerprint(
    pages: &[PathBuf],
    key: &str,
    cancel: &CancelFlag,
) -> Result<[u8; 32]> {
    let mut digests = Vec::with_capacity(pages.len());
    let mut buf = [0u8; 64 * 1024];
    for path in pages {
        let io = |e| PipelineError::Io(path.clone(), e);
        let mut file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        let mut hash = Sha256::new();
        hash.update(len.to_le_bytes());
        loop {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let n = file.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        digests.push(hash.finalize().into());
    }
    Ok(fingerprint_digests(&digests, key))
}

/// Empty crop configuration retains existing uncropped cache compatibility.
pub(crate) fn with_crops(source: [u8; 32], configuration: &[u8]) -> [u8; 32] {
    if configuration.is_empty() {
        return source;
    }
    let mut hash = Sha256::new();
    hash.update(b"manual-crop-v1");
    hash.update(source);
    hash.update(configuration);
    hash.finalize().into()
}

pub(crate) fn load(
    path: &Path,
    fingerprint: &[u8; 32],
    width: u32,
    height: u32,
    format: Format,
) -> Option<Vec<Vec<u8>>> {
    let bytes = fs::read(path).ok()?;
    // magic, source fingerprint, page count, packed pages, SHA-256 checksum.
    if bytes.len() < 76 || &bytes[..8] != MAGIC || &bytes[8..40] != fingerprint {
        return None;
    }
    let end = bytes.len() - 32;
    if Sha256::digest(&bytes[..end]).as_slice() != &bytes[end..] {
        return None;
    }
    let count = u32::from_le_bytes(bytes[40..44].try_into().ok()?) as usize;
    if count == 0 || count > u16::MAX as usize {
        return None;
    }
    let size = PAGE_HEADER_SIZE
        + match format {
            Format::Xtc => converter::xtc::xtg_data_size(width, height),
            Format::Xtch => converter::xtc::xth_data_size(width, height),
        };
    if end != 44 + count.checked_mul(size)? {
        return None;
    }
    let mut pages = Vec::with_capacity(count);
    for page in bytes[44..end].chunks_exact(size) {
        if page[..4] != format.page_magic().to_le_bytes()
            || page[4..6] != (width as u16).to_le_bytes()
            || page[6..8] != (height as u16).to_le_bytes()
            || page[8] != 0
            || page[9] != 0
            || page[10..14] != ((size - PAGE_HEADER_SIZE) as u32).to_le_bytes()
        {
            return None;
        }
        pages.push(page.to_vec());
    }
    Some(pages)
}

/// The pictures of an EPUB's pages: each one its length, then its bytes.
pub(crate) fn load_pictures(path: &Path, fingerprint: &[u8; 32]) -> Option<Vec<Vec<u8>>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 8 + 32 + 4 + 32 || &bytes[..8] != PICTURES_MAGIC || &bytes[8..40] != fingerprint {
        return None;
    }
    let end = bytes.len() - 32;
    if Sha256::digest(&bytes[..end]).as_slice() != &bytes[end..] {
        return None;
    }
    let count = u32::from_le_bytes(bytes[40..44].try_into().ok()?) as usize;
    if count == 0 {
        return None;
    }
    let mut at = 44;
    let mut pages = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        let page = bytes.get(at..at.checked_add(len)?.min(end))?;
        if page.len() != len || !(page.starts_with(&[0x89, b'P']) || page.starts_with(&[0xff, 0xd8])) {
            return None;
        }
        pages.push(page.to_vec());
        at += len;
    }
    (at == end).then_some(pages)
}

pub(crate) fn save_pictures(path: &Path, fingerprint: &[u8; 32], pages: &[Vec<u8>]) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!(
        "{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::io::BufWriter::new(File::create(&tmp)?);
        let mut hash = Sha256::new();
        let mut append = |data: &[u8]| -> std::io::Result<()> {
            hash.update(data);
            file.write_all(data)
        };
        append(PICTURES_MAGIC)?;
        append(fingerprint)?;
        append(&(pages.len() as u32).to_le_bytes())?;
        for page in pages {
            append(&(page.len() as u32).to_le_bytes())?;
            append(page)?;
        }
        file.write_all(&hash.finalize())?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, path)
    };
    if write().is_err() {
        let _ = fs::remove_file(tmp);
    }
}

/// Publish only complete caches. Failure to cache must never prevent an
/// export (for example when the library is read-only or the disk is full).
pub(crate) fn save(path: &Path, fingerprint: &[u8; 32], pages: &[Vec<u8>]) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!(
        "{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::io::BufWriter::new(File::create(&tmp)?);
        let mut hash = Sha256::new();
        let mut append = |data: &[u8]| -> std::io::Result<()> {
            hash.update(data);
            file.write_all(data)
        };
        append(MAGIC)?;
        append(fingerprint)?;
        append(&(pages.len() as u32).to_le_bytes())?;
        for page in pages {
            append(page)?;
        }
        file.write_all(&hash.finalize())?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, path)
    };
    if write().is_err() {
        let _ = fs::remove_file(tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::process_options_for;
    use library::{ConversionSettings, OutputFormat};

    fn key_for(settings: &ConversionSettings) -> String {
        let device = device_profiles::x4_pro();
        cache_key(&process_options_for(&device, settings), settings.pictures())
    }

    fn with(output: OutputFormat, change: impl FnOnce(&mut ConversionSettings)) -> ConversionSettings {
        let mut s = ConversionSettings::default();
        s.set_output(output);
        change(&mut s);
        s
    }

    #[test]
    fn decoded_snapshot_fingerprint_matches_disk_and_tracks_replacement() {
        let root =
            std::env::temp_dir().join(format!("mangapoint-cache-hash-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let paths = vec![root.join("a.img"), root.join("b.img")];
        fs::write(&paths[0], b"first").unwrap();
        fs::write(&paths[1], b"second").unwrap();
        let key = key_for(&ConversionSettings::default());
        let snapshot = [b"first".as_slice(), b"second".as_slice()];
        let original = fingerprint(&paths, &key, &CancelFlag::new()).unwrap();
        assert_eq!(fingerprint_bytes(snapshot.into_iter(), &key), original);
        // The export keeps only each image's digest, never the image.
        let digests = [page_digest(b"first"), page_digest(b"second")];
        assert_eq!(fingerprint_digests(&digests, &key), original);
        fs::write(&paths[0], b"other").unwrap();
        let replaced = fingerprint(&paths, &key, &CancelFlag::new()).unwrap();
        assert_ne!(replaced, original);
        let actual_snapshot = [b"other".as_slice(), b"second".as_slice()];
        assert_eq!(fingerprint_bytes(actual_snapshot.into_iter(), &key), replaced);
        let reversed = [b"second".as_slice(), b"other".as_slice()];
        assert_ne!(fingerprint_bytes(reversed.into_iter(), &key), replaced);
        let cancelled = CancelFlag::new();
        cancelled.cancel();
        assert!(matches!(
            fingerprint(&paths, &key, &cancelled),
            Err(PipelineError::Cancelled)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_fingerprint_of_the_same_input_is_the_same_every_time_and_pinned() {
        // Changing how a fingerprint is made orphans every cached chapter, so
        // the value for fixed input is pinned: if this fails, the cache was
        // (knowingly) invalidated and the pin should be updated with MAGIC.
        let pages = [b"page one".as_slice(), b"page two".as_slice()];
        let key = "packed|fixed";
        let a = fingerprint_bytes(pages.into_iter(), key);
        let b = fingerprint_bytes(pages.into_iter(), key);
        assert_eq!(a, b);
        let hex: String = a.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, PINNED_FINGERPRINT, "fingerprint scheme changed");
    }

    const PINNED_FINGERPRINT: &str = "a653826b51c30a1b598ff8b5a6dc51dacb647a2ddeaa4b2b0eed2fedad718753";

    #[test]
    fn the_key_holds_the_converters_revision() {
        let key = key_for(&ConversionSettings::default());
        let now = fingerprint_digests(&[page_digest(b"x")], &key);
        let mut by_hand = fingerprint_header_for(converter::CONVERTER_REV, &key, 1);
        by_hand.update(page_digest(b"x"));
        assert_eq!(now, <[u8; 32]>::from(by_hand.finalize()));
        let mut next = fingerprint_header_for(converter::CONVERTER_REV + 1, &key, 1);
        next.update(page_digest(b"x"));
        assert_ne!(now, <[u8; 32]>::from(next.finalize()), "a new revision must miss the cache");
    }

    #[test]
    fn an_epub_and_a_cbz_share_their_pictures() {
        let epub = key_for(&with(OutputFormat::Epub, |_| {}));
        let cbz = key_for(&with(OutputFormat::Cbz, |_| {}));
        assert_eq!(epub, cbz);
        // The XTC/XTCH format is not a choice of an EPUB or a CBZ.
        let mut s = with(OutputFormat::Epub, |_| {});
        s.format = converter::xtc::Format::Xtch;
        assert_eq!(key_for(&s), epub);
        // But pictures are not packed pages.
        assert_ne!(key_for(&with(OutputFormat::Xtc, |_| {})), epub);
        assert_ne!(
            key_for(&with(OutputFormat::Xtc, |_| {})),
            key_for(&with(OutputFormat::Xtch, |_| {}))
        );
    }

    #[test]
    fn settings_that_change_the_pages_change_the_key() {
        let base = ConversionSettings::default();
        let key = key_for(&base);
        for (name, changed) in [
            ("contrast", ConversionSettings { contrast: 70, ..base.clone() }),
            ("auto levels", ConversionSettings { auto_levels: !base.auto_levels, ..base.clone() }),
            ("direction", ConversionSettings { direction: converter::xtc::ReadDirection::LeftToRight, ..base.clone() }),
            ("dither", ConversionSettings { dither: converter::Dither::Atkinson, ..base.clone() }),
            ("trim padding", ConversionSettings { trim_padding: 20, ..base.clone() }),
            ("overlap", ConversionSettings { overlap_percent: 20, ..base.clone() }),
            ("zoom", ConversionSettings { zoom: PageZoom::Percent(200), ..base.clone() }),
        ] {
            assert_ne!(key_for(&changed), key, "{name}");
        }
    }

    #[test]
    fn settings_that_cannot_change_the_pages_keep_the_key() {
        // The overlap only matters to the older cut.
        let quiet = ConversionSettings { strip_cut: StripCut::Quiet, ..Default::default() };
        let other = ConversionSettings { overlap_percent: 30, ..quiet.clone() };
        assert_eq!(key_for(&quiet), key_for(&other));
        // The trim only matters to pages that are trimmed (and to a strip's blank rows).
        let full = ConversionSettings { crop: CropMode::Full, layout: Layout::Manga, ..Default::default() };
        let tuned = ConversionSettings { trim_sensitivity: 90, trim_padding: 30, ..full.clone() };
        assert_eq!(key_for(&full), key_for(&tuned));
        let strip = ConversionSettings { crop: CropMode::Full, layout: Layout::Webtoon, ..Default::default() };
        assert_eq!(
            key_for(&strip),
            key_for(&ConversionSettings { trim_padding: 30, ..strip.clone() }),
            "padding is only for trimming"
        );
        assert_ne!(
            key_for(&strip),
            key_for(&ConversionSettings { trim_sensitivity: 90, ..strip.clone() }),
            "a strip's gutters are found by it"
        );
        // Zooms that come to the same thing.
        let fit = ConversionSettings::default();
        for same in [PageZoom::Percent(100), PageZoom::Percent(40), PageZoom::Percent(0)] {
            assert_eq!(key_for(&ConversionSettings { zoom: same, ..fit.clone() }), key_for(&fit), "{same:?}");
        }
        assert_eq!(
            key_for(&ConversionSettings { zoom: PageZoom::Percent(300), ..fit.clone() }),
            key_for(&ConversionSettings { zoom: PageZoom::Percent(900), ..fit.clone() })
        );
        assert_eq!(
            key_for(&ConversionSettings { zoom_mode: ZoomMode::Crop, ..fit.clone() }),
            key_for(&fit),
            "the mode of a zoom that is off"
        );
        // Full grays are not dithered.
        let eight = |dither| with(OutputFormat::Epub, |s| s.dither = dither);
        let kindle = |s: &ConversionSettings| {
            let device = device_profiles::known("kindle-paperwhite-12").unwrap();
            cache_key(&process_options_for(&device, s), true)
        };
        assert_eq!(kindle(&eight(converter::Dither::Ordered)), kindle(&eight(converter::Dither::Atkinson)));
    }
}
