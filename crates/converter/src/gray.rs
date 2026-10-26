//! Image buffers used between pipeline stages.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImageError {
    #[error("image dimensions must be non-zero (got {width}x{height})")]
    ZeroSize { width: u32, height: u32 },
    #[error("pixel buffer has {found} entries but {width}x{height} needs {expected}")]
    BufferSize {
        width: u32,
        height: u32,
        expected: usize,
        found: usize,
    },
}

/// 8-bit grayscale image: 0 = black, 255 = white. Row-major, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayImage {
    pub fn new(width: u32, height: u32, fill: u8) -> Result<Self, ImageError> {
        check_size(width, height)?;
        Ok(Self {
            width,
            height,
            pixels: vec![fill; width as usize * height as usize],
        })
    }

    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        check_size(width, height)?;
        let expected = width as usize * height as usize;
        if pixels.len() != expected {
            return Err(ImageError::BufferSize {
                width,
                height,
                expected,
                found: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn from_fn(
        width: u32,
        height: u32,
        mut f: impl FnMut(u32, u32) -> u8,
    ) -> Result<Self, ImageError> {
        check_size(width, height)?;
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.push(f(x, y));
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, value: u8) {
        self.pixels[y as usize * self.width as usize + x as usize] = value;
    }

    /// Fill an axis-aligned rectangle, clipped to the image.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, value: u8) {
        for yy in y..(y.saturating_add(h)).min(self.height) {
            for xx in x..(x.saturating_add(w)).min(self.width) {
                self.set(xx, yy, value);
            }
        }
    }
}

/// A full-colour page: 8-bit R, G, B per pixel, row-major, top to bottom.
/// What a colour export draws a page as, before it is encoded as a JPEG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` bytes: R, G, B for each pixel.
    pub rgb: Vec<u8>,
}

impl ColorImage {
    /// Join a luma plane with its two chroma planes (Cb, Cr, centred on 128;
    /// BT.601, the JPEG convention). The planes must be the same size. Where
    /// both chroma planes are 128 the pixel is exactly the gray luma.
    pub fn from_ycc(luma: &GrayImage, cb: &GrayImage, cr: &GrayImage) -> ColorImage {
        assert!(
            (luma.width, luma.height) == (cb.width, cb.height)
                && (luma.width, luma.height) == (cr.width, cr.height),
            "the planes of a colour page are the same size"
        );
        let mut rgb = Vec::with_capacity(luma.pixels.len() * 3);
        for ((&y, &b), &r) in luma.pixels.iter().zip(&cb.pixels).zip(&cr.pixels) {
            let (y, b, r) = (y as f32, b as f32 - 128.0, r as f32 - 128.0);
            let channel = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            rgb.push(channel(y + 1.402 * r));
            rgb.push(channel(y - 0.344_136 * b - 0.714_136 * r));
            rgb.push(channel(y + 1.772 * b));
        }
        ColorImage {
            width: luma.width,
            height: luma.height,
            rgb,
        }
    }

    /// The `[r, g, b]` of one pixel.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [u8; 3] {
        let at = (y as usize * self.width as usize + x as usize) * 3;
        [self.rgb[at], self.rgb[at + 1], self.rgb[at + 2]]
    }
}

fn check_size(width: u32, height: u32) -> Result<(), ImageError> {
    if width == 0 || height == 0 {
        Err(ImageError::ZeroSize { width, height })
    } else {
        Ok(())
    }
}

/// Bits per pixel of a quantized page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// Black and white (XTC pages).
    One,
    /// Four gray levels (XTCH pages).
    Two,
    /// The full 256 grays, not reduced at all (pictures inside an EPUB, for
    /// readers that show many grays).
    Eight,
}

impl Depth {
    /// Highest brightness level: 1 for [`Depth::One`], 3 for [`Depth::Two`],
    /// 255 for [`Depth::Eight`].
    pub fn max_level(self) -> u8 {
        match self {
            Depth::One => 1,
            Depth::Two => 3,
            Depth::Eight => 255,
        }
    }
}

/// A quantized image holding *brightness levels*, not stored bit values.
///
/// Level 0 is black and `depth.max_level()` is white; for [`Depth::Two`]
/// level 1 is dark gray and level 2 is light gray. Translation to the
/// on-disk value encoding happens only inside the XTC encoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelImage {
    pub width: u32,
    pub height: u32,
    pub depth: Depth,
    pub levels: Vec<u8>,
}

impl LevelImage {
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.levels[y as usize * self.width as usize + x as usize]
    }

    /// The image turned a quarter turn clockwise: its top edge becomes the
    /// right edge. Levels are moved, never recomputed.
    pub fn rotated_cw(&self) -> LevelImage {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut levels = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                // (x, y) lands at (h - 1 - y, x) in an image h wide and w tall.
                levels[x * h + (h - 1 - y)] = self.levels[y * w + x];
            }
        }
        LevelImage {
            width: self.height,
            height: self.width,
            depth: self.depth,
            levels,
        }
    }
}
