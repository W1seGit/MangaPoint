//! TIFF pictures, which a TIFF file can hold several of. Each becomes a PNG,
//! which the converter reads like any other page.

use super::{fail, MAX_PAGE_BYTES};
use crate::error::{PipelineError, Result};
use image::{DynamicImage, ImageFormat};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::ColorType;

fn open(path: &Path) -> Result<Decoder<BufReader<File>>> {
    let file = File::open(path).map_err(|e| PipelineError::Io(path.to_path_buf(), e))?;
    Decoder::new(BufReader::new(file)).map_err(|e| fail(format!("not a readable TIFF ({e})")))
}

/// How many pictures a TIFF holds.
pub(crate) fn count_pages(path: &Path) -> Result<usize> {
    let mut decoder = open(path)?;
    let mut count = 1;
    while decoder.more_images() {
        if decoder.next_image().is_err() {
            break;
        }
        count += 1;
        if count > 10_000 {
            break;
        }
    }
    Ok(count)
}

/// A TIFF kept open while several of its pictures are read, so a file of
/// hundreds of pictures is opened once, not once for each.
pub(crate) struct Reader {
    decoder: Decoder<BufReader<File>>,
}

impl Reader {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        Ok(Self { decoder: open(path)? })
    }

    /// Picture number `index`, as a PNG.
    pub(crate) fn read_page(&mut self, index: usize) -> Result<Vec<u8>> {
        read_from(&mut self.decoder, index)
    }
}

/// Picture number `index` of a TIFF, as a PNG.
#[cfg(test)]
pub(crate) fn read_page(path: &Path, index: usize) -> Result<Vec<u8>> {
    Reader::open(path)?.read_page(index)
}

fn read_from(decoder: &mut Decoder<BufReader<File>>, index: usize) -> Result<Vec<u8>> {
    decoder
        .seek_to_image(index)
        .map_err(|e| fail(format!("TIFF picture {index}: {e}")))?;
    let (w, h) = decoder.dimensions().map_err(|e| fail(e.to_string()))?;
    if u64::from(w) * u64::from(h) > super::MAX_PAGE_PIXELS {
        return Err(fail("the TIFF picture is too large"));
    }
    let color = decoder.colortype().map_err(|e| fail(e.to_string()))?;
    let data = decoder.read_image().map_err(|e| fail(format!("TIFF picture {index}: {e}")))?;
    let bytes: Vec<u8> = match data {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|s| (s >> 8) as u8).collect(),
        _ => return Err(fail("this kind of TIFF is not supported")),
    };
    let image = match color {
        ColorType::Gray(8) | ColorType::Gray(16) => {
            image::GrayImage::from_raw(w, h, bytes).map(DynamicImage::ImageLuma8)
        }
        ColorType::GrayA(8) | ColorType::GrayA(16) => {
            image::GrayAlphaImage::from_raw(w, h, bytes).map(DynamicImage::ImageLumaA8)
        }
        ColorType::RGB(8) | ColorType::RGB(16) => {
            image::RgbImage::from_raw(w, h, bytes).map(DynamicImage::ImageRgb8)
        }
        ColorType::RGBA(8) | ColorType::RGBA(16) => {
            image::RgbaImage::from_raw(w, h, bytes).map(DynamicImage::ImageRgba8)
        }
        ColorType::CMYK(8) | ColorType::CMYK(16) => {
            let rgb: Vec<u8> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let k = 255 - u32::from(p[3]);
                    [0, 1, 2].map(|i| ((255 - u32::from(p[i])) * k / 255) as u8)
                })
                .collect();
            image::RgbImage::from_raw(w, h, rgb).map(DynamicImage::ImageRgb8)
        }
        other => return Err(fail(format!("this kind of TIFF ({other:?}) is not supported"))),
    }
    .ok_or_else(|| fail("the TIFF picture is damaged"))?;
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|e| fail(e.to_string()))?;
    let png = png.into_inner();
    if png.len() as u64 > MAX_PAGE_BYTES {
        return Err(fail("the TIFF picture is too large"));
    }
    Ok(png)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tiff::encoder::{colortype, TiffEncoder};

    pub(crate) fn write_tiff(path: &Path, pictures: &[(u32, u32, u8)]) {
        let file = File::create(path).unwrap();
        let mut encoder = TiffEncoder::new(file).unwrap();
        for &(w, h, shade) in pictures {
            encoder
                .write_image::<colortype::Gray8>(w, h, &vec![shade; (w * h) as usize])
                .unwrap();
        }
    }

    #[test]
    fn a_tiff_with_several_pictures_gives_a_png_for_each() {
        let dir = std::env::temp_dir().join(format!("mp-tiff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pages.tif");
        write_tiff(&path, &[(8, 6, 10), (4, 4, 200)]);
        assert_eq!(count_pages(&path).unwrap(), 2);
        let second = read_page(&path, 1).unwrap();
        let decoded = image::load_from_memory(&second).unwrap().to_luma8();
        assert_eq!((decoded.width(), decoded.height()), (4, 4));
        assert_eq!(decoded.get_pixel(1, 1).0[0], 200);
        assert!(read_page(&path, 9).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
