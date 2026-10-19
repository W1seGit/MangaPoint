//! XTG (1-bit) and XTH (2-bit) page encoding, plus the matching decoder used
//! for round-trip tests and the inspect tool.

use super::{XtcError, GRAY_MAP, PAGE_HEADER_SIZE, XTG_MAGIC, XTH_MAGIC};
use crate::gray::{Depth, LevelImage};

/// Bytes of bitmap data in an XTG page.
pub fn xtg_data_size(width: u32, height: u32) -> usize {
    width.div_ceil(8) as usize * height as usize
}

/// Bytes of bitmap data in an XTH page: two planes, each column padded to a
/// whole byte (this is the formula CrossPoint documents).
pub fn xth_data_size(width: u32, height: u32) -> usize {
    width as usize * height.div_ceil(8) as usize * 2
}

fn check(img: &LevelImage) -> Result<(), XtcError> {
    let (w, h) = (img.width, img.height);
    if w == 0 || h == 0 || w > 65535 || h > 65535 {
        return Err(XtcError::BadDimensions {
            width: w,
            height: h,
        });
    }
    let expected = w as usize * h as usize;
    if img.levels.len() != expected {
        return Err(XtcError::BadPixelCount {
            width: w,
            height: h,
            expected,
            found: img.levels.len(),
        });
    }
    Ok(())
}

/// Encode one page, including its 22-byte header. The page type follows
/// `img.depth`.
pub fn encode_page(img: &LevelImage) -> Result<Vec<u8>, XtcError> {
    check(img)?;
    let (magic, data) = match img.depth {
        Depth::One => (XTG_MAGIC, encode_xtg_data(img)),
        Depth::Two => (XTH_MAGIC, encode_xth_data(img)),
        Depth::Eight => {
            return Err(XtcError::DepthMismatch {
                expected: Depth::Two,
                found: Depth::Eight,
            })
        }
    };
    let mut out = Vec::with_capacity(PAGE_HEADER_SIZE + data.len());
    out.extend_from_slice(&magic.to_le_bytes());
    out.extend_from_slice(&(img.width as u16).to_le_bytes());
    out.extend_from_slice(&(img.height as u16).to_le_bytes());
    out.push(0); // colorMode: 0 for both XTG and XTH
    out.push(0); // compression: none
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // md5: optional, unused
    debug_assert_eq!(out.len(), PAGE_HEADER_SIZE);
    out.extend_from_slice(&data);
    Ok(out)
}

/// Row-major, 8 pixels per byte, most significant bit leftmost, 1 = white.
fn encode_xtg_data(img: &LevelImage) -> Vec<u8> {
    let row_bytes = img.width.div_ceil(8) as usize;
    let mut data = vec![0u8; row_bytes * img.height as usize];
    for (row, out) in img
        .levels
        .chunks_exact(img.width as usize)
        .zip(data.chunks_exact_mut(row_bytes))
    {
        for (eight, byte) in row.chunks(8).zip(out.iter_mut()) {
            *byte = eight
                .iter()
                .enumerate()
                .fold(0u8, |bits, (i, &level)| bits | (((level != 0) as u8) << (7 - i)));
        }
    }
    data
}

/// Two bit planes (high bit, then low bit of the stored value). Each plane is
/// column-major starting at the rightmost column; a column packs 8 vertical
/// pixels per byte, topmost pixel in the most significant bit.
fn encode_xth_data(img: &LevelImage) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let col_bytes = h.div_ceil(8);
    let plane_len = w * col_bytes;
    let mut data = vec![0u8; plane_len * 2];
    let (high, low) = data.split_at_mut(plane_len);
    // Eight rows at a time, each read straight along, so a column's byte is
    // made from eight neighbouring rows instead of a walk down the image.
    for (block, rows) in img.levels.chunks(8 * w).enumerate() {
        for x in 0..w {
            let (mut hi, mut lo) = (0u8, 0u8);
            for (k, row) in rows.chunks_exact(w).enumerate() {
                let stored = GRAY_MAP[row[x] as usize];
                hi |= ((stored >> 1) & 1) << (7 - k);
                lo |= (stored & 1) << (7 - k);
            }
            let at = (w - 1 - x) * col_bytes + block;
            high[at] = hi;
            low[at] = lo;
        }
    }
    data
}

/// A page read back from bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedPage {
    pub image: LevelImage,
    pub compression: u8,
    pub color_mode: u8,
}

/// Check a packed page without allocating an unpacked pixel buffer.
pub(crate) fn validate_page(bytes: &[u8], index: usize) -> Result<(u32, u32, Depth), XtcError> {
    let bad = |reason| XtcError::InvalidPage { index, reason };
    if bytes.len() < PAGE_HEADER_SIZE {
        return Err(bad("shorter than the 22-byte page header"));
    }
    let depth = match u32::from_le_bytes(bytes[0..4].try_into().unwrap()) {
        XTG_MAGIC => Depth::One,
        XTH_MAGIC => Depth::Two,
        _ => return Err(bad("unknown page magic")),
    };
    let width = u16::from_le_bytes(bytes[4..6].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as u32;
    if width == 0 || height == 0 {
        return Err(bad("zero-sized page"));
    }
    if bytes[8] != 0 || bytes[9] != 0 {
        return Err(bad("unsupported color mode or compression"));
    }
    let size = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let expected = match depth {
        Depth::One => xtg_data_size(width, height),
        Depth::Two => xth_data_size(width, height),
        Depth::Eight => unreachable!("a page's depth comes from its magic, which is One or Two"),
    };
    if size != expected || bytes.len() != PAGE_HEADER_SIZE + expected {
        return Err(bad("bitmap length does not match the page dimensions"));
    }
    Ok((width, height, depth))
}

/// Decode a page (header + bitmap). `index` is only used in error messages.
pub fn decode_page(bytes: &[u8], index: usize) -> Result<DecodedPage, XtcError> {
    let bad = |reason| XtcError::InvalidPage { index, reason };
    if bytes.len() < PAGE_HEADER_SIZE {
        return Err(bad("shorter than the 22-byte page header"));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let width = u16::from_le_bytes(bytes[4..6].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as u32;
    let color_mode = bytes[8];
    let compression = bytes[9];
    let data_size = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let depth = match magic {
        XTG_MAGIC => Depth::One,
        XTH_MAGIC => Depth::Two,
        _ => return Err(bad("unknown page magic")),
    };
    if width == 0 || height == 0 {
        return Err(bad("zero-sized page"));
    }
    if compression != 0 {
        return Err(bad("compressed pages are not supported"));
    }
    let expected = match depth {
        Depth::One => xtg_data_size(width, height),
        Depth::Two => xth_data_size(width, height),
        Depth::Eight => unreachable!("a page's depth comes from its magic, which is One or Two"),
    };
    if data_size != expected {
        return Err(bad("dataSize does not match the page dimensions"));
    }
    let data = bytes
        .get(PAGE_HEADER_SIZE..PAGE_HEADER_SIZE + data_size)
        .ok_or_else(|| bad("bitmap data is truncated"))?;

    let mut levels = vec![0u8; width as usize * height as usize];
    match depth {
        Depth::One => {
            let row_bytes = width.div_ceil(8) as usize;
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let bit = data[y * row_bytes + x / 8] & (0x80 >> (x % 8)) != 0;
                    levels[y * width as usize + x] = bit as u8;
                }
            }
        }
        Depth::Two => {
            let col_bytes = height.div_ceil(8) as usize;
            let plane_len = width as usize * col_bytes;
            for (col_index, x) in (0..width as usize).rev().enumerate() {
                for y in 0..height as usize {
                    let at = col_index * col_bytes + y / 8;
                    let mask = 0x80 >> (y % 8);
                    let hi = (data[at] & mask != 0) as u8;
                    let lo = (data[plane_len + at] & mask != 0) as u8;
                    let stored = (hi << 1) | lo;
                    levels[y * width as usize + x] = GRAY_MAP[stored as usize];
                }
            }
        }
        Depth::Eight => unreachable!("a page's depth comes from its magic, which is One or Two"),
    }
    Ok(DecodedPage {
        image: LevelImage {
            width,
            height,
            depth,
            levels,
        },
        compression,
        color_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(width: u32, height: u32, depth: Depth, levels: Vec<u8>) -> LevelImage {
        LevelImage {
            width,
            height,
            depth,
            levels,
        }
    }

    // Hand-computed golden bytes -------------------------------------------

    #[test]
    fn xtg_golden_bytes() {
        // 10x2, so rows are padded to 2 bytes. 1 = white.
        // row 0: 1010101010 -> 0b10101010, 0b10 000000
        // row 1: 0000000011 -> 0b00000000, 0b11 000000
        let page = img(
            10,
            2,
            Depth::One,
            vec![
                1, 0, 1, 0, 1, 0, 1, 0, 1, 0, //
                0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
            ],
        );
        let bytes = encode_page(&page).unwrap();
        let mut expected = vec![
            0x58, 0x54, 0x47, 0x00, // "XTG\0"
            10, 0, // width
            2, 0, // height
            0, // colorMode
            0, // compression
            4, 0, 0, 0, // dataSize = 2 bytes * 2 rows
        ];
        expected.extend_from_slice(&[0; 8]); // md5
        expected.extend_from_slice(&[0b1010_1010, 0b1000_0000, 0b0000_0000, 0b1100_0000]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn xth_golden_bytes() {
        // 2 wide x 8 tall. Levels (brightness): left column all white (3),
        // right column alternates black(0) / dark gray(1) / light gray(2) / white(3) twice.
        // Stored value = GRAY_MAP[level] = [3,1,2,0][level]:
        //   white 3 -> 0 (bits 00), black 0 -> 3 (11), dark 1 -> 1 (01), light 2 -> 2 (10)
        let right = [0u8, 1, 2, 3, 0, 1, 2, 3];
        let mut levels = Vec::new();
        for r in right {
            levels.push(3); // x = 0, left
            levels.push(r); // x = 1, right
        }
        let page = img(2, 8, Depth::Two, levels);
        let bytes = encode_page(&page).unwrap();

        assert_eq!(&bytes[0..4], &[0x58, 0x54, 0x48, 0x00]); // "XTH\0"
        assert_eq!(&bytes[10..14], &[4, 0, 0, 0]); // 2 cols * 1 byte * 2 planes

        let data = &bytes[PAGE_HEADER_SIZE..];
        // Columns are stored right to left, so column 0 in the file is x = 1.
        // Stored values for x = 1 from top: 3,1,2,0,3,1,2,0
        // high bits: 1,0,1,0,1,0,1,0 = 0xAA ; low bits: 1,1,0,0,1,1,0,0 = 0xCC
        // x = 0 is all white -> stored 0 -> all bits clear.
        assert_eq!(data, &[0xAA, 0x00, 0xCC, 0x00]);
    }

    #[test]
    fn xth_columns_are_padded_when_height_is_not_a_multiple_of_8() {
        assert_eq!(xth_data_size(3, 9), 3 * 2 * 2);
        let page = img(3, 9, Depth::Two, vec![0; 27]);
        let bytes = encode_page(&page).unwrap();
        assert_eq!(bytes.len(), PAGE_HEADER_SIZE + 12);
        // 9 rows -> bit 0x80 of the second byte of each column is row 8.
        // All black = stored 3 = both planes set; padding rows stay clear.
        let data = &bytes[PAGE_HEADER_SIZE..];
        assert_eq!(data[0], 0xFF);
        assert_eq!(data[1], 0x80);
    }

    #[test]
    fn device_size_page_sizes_match_documented_formulas() {
        assert_eq!(xtg_data_size(480, 800), 48_000);
        assert_eq!(xth_data_size(480, 800), 96_000);
    }

    // Round trips -----------------------------------------------------------

    fn pattern(w: u32, h: u32, depth: Depth) -> LevelImage {
        let max = depth.max_level() as u32 + 1;
        let levels = (0..w * h)
            .map(|i| ((i * 7 + i / w * 3) % max) as u8)
            .collect();
        img(w, h, depth, levels)
    }

    #[test]
    fn round_trip_all_sizes() {
        for depth in [Depth::One, Depth::Two] {
            for (w, h) in [(1, 1), (7, 9), (8, 8), (9, 17), (480, 800)] {
                let page = pattern(w, h, depth);
                let bytes = encode_page(&page).unwrap();
                let back = decode_page(&bytes, 0).unwrap();
                assert_eq!(back.image, page, "{depth:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            encode_page(&img(0, 5, Depth::One, vec![])),
            Err(XtcError::BadDimensions {
                width: 0,
                height: 5
            })
        );
        assert!(matches!(
            encode_page(&img(2, 2, Depth::One, vec![0; 3])),
            Err(XtcError::BadPixelCount { .. })
        ));
        assert!(decode_page(&[0; 10], 0).is_err());
        let mut bytes = encode_page(&pattern(8, 8, Depth::One)).unwrap();
        bytes[0] = 0; // break magic
        assert!(decode_page(&bytes, 0).is_err());
    }

    /// The encoders as they were: a pixel at a time, columns read downward.
    fn xtg_old(img: &LevelImage) -> Vec<u8> {
        let row_bytes = img.width.div_ceil(8) as usize;
        let mut data = vec![0u8; row_bytes * img.height as usize];
        for y in 0..img.height {
            for x in 0..img.width {
                if img.get(x, y) != 0 {
                    data[y as usize * row_bytes + (x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
        }
        data
    }

    fn xth_old(img: &LevelImage) -> Vec<u8> {
        let col_bytes = img.height.div_ceil(8) as usize;
        let plane_len = img.width as usize * col_bytes;
        let mut data = vec![0u8; plane_len * 2];
        for (plane, shift) in [(0usize, 1u8), (1usize, 0u8)] {
            for (col_index, x) in (0..img.width).rev().enumerate() {
                for y in 0..img.height {
                    let stored = GRAY_MAP[img.get(x, y) as usize];
                    if (stored >> shift) & 1 == 1 {
                        data[plane * plane_len + col_index * col_bytes + (y / 8) as usize] |=
                            0x80 >> (y % 8);
                    }
                }
            }
        }
        data
    }

    #[test]
    fn the_row_by_row_encoders_write_the_same_bytes_as_the_original_ones() {
        let mut seed = 12345u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as u8
        };
        for (w, h) in [(1, 1), (2, 8), (3, 9), (7, 17), (16, 16), (31, 5), (480, 800), (800, 480)] {
            let one = img(w, h, Depth::One, (0..w * h).map(|_| next() & 1).collect());
            assert_eq!(encode_xtg_data(&one), xtg_old(&one), "xtg {w}x{h}");
            let two = img(w, h, Depth::Two, (0..w * h).map(|_| next() & 3).collect());
            assert_eq!(encode_xth_data(&two), xth_old(&two), "xth {w}x{h}");
        }
    }

    /// `cargo test -p converter --release -- --ignored --nocapture encoders`
    #[test]
    #[ignore]
    fn timing_of_the_encoders_against_the_originals() {
        let two = pattern(480, 800, Depth::Two);
        let one = pattern(480, 800, Depth::One);
        let time = |f: &dyn Fn() -> Vec<u8>| {
            let started = std::time::Instant::now();
            for _ in 0..200 {
                std::hint::black_box(f());
            }
            started.elapsed() / 200
        };
        let (a, b) = (time(&|| xth_old(&two)), time(&|| encode_xth_data(&two)));
        eprintln!("xth 480x800 per page: before {a:?}, after {b:?}");
        let (a, b) = (time(&|| xtg_old(&one)), time(&|| encode_xtg_data(&one)));
        eprintln!("xtg 480x800 per page: before {a:?}, after {b:?}");
    }
}
