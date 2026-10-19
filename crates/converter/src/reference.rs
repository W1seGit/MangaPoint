//! The conversion routines as they were before the speed work, kept only to
//! prove the faster ones give exactly the same pixels.

use crate::gray::GrayImage;
use crate::process::{axis_weights, border_median};

/// A small repeatable random generator for test pictures.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// A page that looks a little like a scan: a paper tone, blocks of ink and
/// some noise, with blank margins of random size.
pub fn scan(rng: &mut Rng, w: u32, h: u32) -> GrayImage {
    let paper = 200 + rng.below(56) as u8;
    let mut img = GrayImage::new(w, h, paper).unwrap();
    let (mx, my) = (rng.below(w / 4 + 1), rng.below(h / 4 + 1));
    for _ in 0..(3 + rng.below(8)) {
        let x = mx + rng.below((w - 2 * mx).max(1));
        let y = my + rng.below((h - 2 * my).max(1));
        let bw = 1 + rng.below((w - mx).saturating_sub(x).max(1));
        let bh = 1 + rng.below((h - my).saturating_sub(y).max(1));
        let ink = rng.below(120) as u8;
        img.fill_rect(x, y, bw, bh, ink);
    }
    for _ in 0..(w * h / 50) {
        let (x, y) = (rng.below(w), rng.below(h));
        img.set(x, y, rng.below(256) as u8);
    }
    img
}

pub fn resize_old(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    if (img.width, img.height) == (w, h) {
        return img.clone();
    }
    // Horizontal pass into f32, then vertical pass.
    let horizontal: Vec<f32> = {
        let weights = axis_weights(img.width, w);
        let mut out = vec![0f32; w as usize * img.height as usize];
        for y in 0..img.height as usize {
            let row = &img.pixels[y * img.width as usize..(y + 1) * img.width as usize];
            for (x, taps) in weights.iter().enumerate() {
                out[y * w as usize + x] = taps.iter().map(|&(i, k)| row[i] as f32 * k).sum();
            }
        }
        out
    };
    let weights = axis_weights(img.height, h);
    let mut out = vec![0u8; w as usize * h as usize];
    for (y, taps) in weights.iter().enumerate() {
        for x in 0..w as usize {
            let v: f32 = taps
                .iter()
                .map(|&(i, k)| horizontal[i * w as usize + x] * k)
                .sum();
            out[y * w as usize + x] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    GrayImage {
        width: w,
        height: h,
        pixels: out,
    }
}

pub fn trim_bounds_old(img: &GrayImage, threshold: i16, pad: u8) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (img.width, img.height);
    let bg = border_median(img);
    let differs = |x: u32, y: u32| (img.get(x, y) as i16 - bg as i16).abs() > threshold;
    let row_has = |y: u32| (0..w).filter(|&x| differs(x, y)).count() as u32 > (w / 200).max(1);
    let col_has = |x: u32| (0..h).filter(|&y| differs(x, y)).count() as u32 > (h / 200).max(1);

    let top = (0..h).find(|&y| row_has(y))?;
    let bottom = (0..h).rev().find(|&y| row_has(y))?;
    let left = (0..w).find(|&x| col_has(x))?;
    let right = (0..w).rev().find(|&x| col_has(x))?;
    let pad = pad as u32;
    let x0 = left.saturating_sub(pad);
    let y0 = top.saturating_sub(pad);
    let x1 = (right + pad + 1).min(w);
    let y1 = (bottom + pad + 1).min(h);
    if (x1 - x0) * 5 < w || (y1 - y0) * 5 < h {
        return None;
    }
    if (x0, y0, x1, y1) == (0, 0, w, h) {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

pub fn blit_old(dst: &mut GrayImage, src: &GrayImage, x0: u32, y0: u32) {
    for y in 0..src.height.min(dst.height.saturating_sub(y0)) {
        for x in 0..src.width.min(dst.width.saturating_sub(x0)) {
            dst.set(x0 + x, y0 + y, src.get(x, y));
        }
    }
}
