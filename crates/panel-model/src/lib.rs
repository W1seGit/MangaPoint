//! The trained panel finder.
//!
//! A YOLO26s segmentation model trained on Manga109 to find panel frames,
//! lettering and balloons ("Manga109-panel-balloon-text-yolov26-segmentation"
//! by ShadowB / Abdelhadi Marjane). Only its frames are used. It runs on the
//! CPU with tract, so nothing besides the model file has to be installed, and
//! it is downloaded the first time it is turned on rather than shipped with
//! the app.
//!
//! The frames it returns are raw: [`converter::panels_from_frames`] cleans
//! them up and puts them in reading order.

use converter::process::resize;
use converter::{GrayImage, PanelModel, Rect};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tract_onnx::prelude::*;

/// The model file the app downloads.
pub struct ModelFile {
    /// Its name in the models folder. A new model gets a new name, so a
    /// half-updated folder never mixes the two.
    pub file_name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

pub const PANELS: ModelFile = ModelFile {
    file_name: "panels-yolo26s-seg-1024-v1.onnx",
    url: "https://github.com/Rhonstin/MangaPoint/releases/download/models-v1/panels-yolo26s-seg-1024-v1.onnx",
    sha256: "aee780fcf7fcb053f9bffa66b5bffae5bc484b687c1990f9d5f03a93c320eea6",
    bytes: 42_039_968,
};

/// The square the page is fitted into: the size the model was exported at.
const SIDE: usize = 1024;
/// Frames the model is less sure of than this are ignored.
const MIN_SCORE: f32 = 0.3;
/// Two frames overlapping more than this (intersection over union) are one.
const SAME_FRAME: f32 = 0.5;
/// The model's answer for each class starts at this row of its output:
/// four box rows come first, then frame, text, balloon.
const FRAME_ROW: usize = 4;
/// Pages remembered, so a preview that is drawn again does not run the
/// model again.
const REMEMBERED: usize = 4096;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("could not read the panel model: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not run the panel model: {0}")]
    Model(String),
    #[error("could not download the panel model: {0}")]
    Download(String),
    #[error("the downloaded panel model is damaged (checksum {found}, expected {expected})")]
    Checksum { found: String, expected: String },
}

impl From<TractError> for ModelError {
    fn from(e: TractError) -> Self {
        ModelError::Model(e.to_string())
    }
}

/// Where the model is kept: `<data folder>/models/<file>`.
pub fn model_path(data_root: &Path) -> PathBuf {
    data_root.join("models").join(PANELS.file_name)
}

/// Whether the model has been downloaded.
pub fn is_downloaded(data_root: &Path) -> bool {
    std::fs::metadata(model_path(data_root)).is_ok_and(|m| m.len() == PANELS.bytes)
}

/// Download the model into the data folder, checking it against its
/// checksum before it is put in place. `progress` gets the bytes done and
/// the total.
pub async fn download(
    data_root: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let path = model_path(data_root);
    let dir = path.parent().expect("the model path has a folder");
    tokio::fs::create_dir_all(dir).await?;
    let part = path.with_extension("onnx.part");
    let response = reqwest::get(PANELS.url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| ModelError::Download(e.to_string()))?;
    let total = response.content_length().unwrap_or(PANELS.bytes);
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| ModelError::Download(e.to_string()))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await?;
    drop(file);
    let found = format!("{:x}", hasher.finalize());
    if found != PANELS.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(ModelError::Checksum {
            found,
            expected: PANELS.sha256.into(),
        });
    }
    tokio::fs::rename(&part, &path).await?;
    Ok(path)
}

/// The loaded model, ready to find frames.
pub struct YoloPanels {
    plan: Arc<TypedRunnableModel>,
    remembered: Mutex<HashMap<u64, Vec<Rect>>>,
}

impl YoloPanels {
    /// Load and prepare the model. Takes about half a second.
    pub fn load(path: &Path) -> Result<Self, ModelError> {
        let plan = tract_onnx::onnx()
            .model_for_path(path)?
            .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            plan,
            remembered: Mutex::new(HashMap::new()),
        })
    }

    /// Load the downloaded model, if there is one.
    pub fn load_downloaded(data_root: &Path) -> Option<Result<Self, ModelError>> {
        is_downloaded(data_root).then(|| Self::load(&model_path(data_root)))
    }

    /// The panel frames on `img`, in its pixels, strongest first.
    pub fn detect(&self, img: &GrayImage) -> Result<Vec<Rect>, ModelError> {
        // Fit the page into the square, centred on gray as the model was
        // trained (Ultralytics' letterbox).
        let scale = (SIDE as f64 / img.width as f64).min(SIDE as f64 / img.height as f64);
        let w = ((img.width as f64 * scale).round() as usize).clamp(1, SIDE);
        let h = ((img.height as f64 * scale).round() as usize).clamp(1, SIDE);
        let fitted = resize(img, w as u32, h as u32);
        let (dx, dy) = ((SIDE - w) / 2, (SIDE - h) / 2);
        let mut input = tract_ndarray::Array4::<f32>::from_elem((1, 3, SIDE, SIDE), 114.0 / 255.0);
        for y in 0..h {
            for x in 0..w {
                let v = fitted.pixels[y * w + x] as f32 / 255.0;
                for c in 0..3 {
                    input[[0, c, dy + y, dx + x]] = v;
                }
            }
        }
        let outputs = self.plan.run(tvec!(Tensor::from(input).into()))?;
        let out = outputs[0].to_plain_array_view::<f32>()?;
        let candidates = out.shape()[2];

        let mut found: Vec<(f32, [f32; 4])> = (0..candidates)
            .filter_map(|i| {
                let score = out[[0, FRAME_ROW, i]];
                if score < MIN_SCORE {
                    return None;
                }
                let (cx, cy) = (out[[0, 0, i]], out[[0, 1, i]]);
                let (bw, bh) = (out[[0, 2, i]], out[[0, 3, i]]);
                let back = |v: f32, offset: usize| ((v - offset as f32) / scale as f32).max(0.0);
                Some((
                    score,
                    [
                        back(cx - bw / 2.0, dx),
                        back(cy - bh / 2.0, dy),
                        back(cx + bw / 2.0, dx).min(img.width as f32),
                        back(cy + bh / 2.0, dy).min(img.height as f32),
                    ],
                ))
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));

        let mut kept: Vec<[f32; 4]> = Vec::new();
        for (_, b) in found {
            if kept.iter().all(|k| iou(k, &b) <= SAME_FRAME) {
                kept.push(b);
            }
        }
        Ok(kept
            .into_iter()
            .filter_map(|[x0, y0, x1, y1]| {
                let (x, y) = (x0.floor() as u32, y0.floor() as u32);
                let (w, h) = (
                    (x1.ceil() as u32).saturating_sub(x),
                    (y1.ceil() as u32).saturating_sub(y),
                );
                (w > 0 && h > 0).then_some(Rect { x, y, w, h })
            })
            .collect())
    }
}

impl PanelModel for YoloPanels {
    fn frames(&self, img: &GrayImage) -> Option<Vec<Rect>> {
        let key = page_key(img);
        if let Some(known) = self.remembered.lock().ok()?.get(&key) {
            return Some(known.clone());
        }
        let frames = self.detect(img).ok()?;
        let mut remembered = self.remembered.lock().ok()?;
        if remembered.len() >= REMEMBERED {
            remembered.clear();
        }
        remembered.insert(key, frames.clone());
        Some(frames)
    }
}

/// Identifies a page by its pixels.
fn page_key(img: &GrayImage) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (img.width, img.height).hash(&mut h);
    img.pixels.hash(&mut h);
    h.finish()
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let area = |r: &[f32; 4]| (r[2] - r[0]) * (r[3] - r[1]);
    inter / (area(a) + area(b) - inter).max(f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_measured_as_intersection_over_union() {
        let a = [0.0, 0.0, 10.0, 10.0];
        assert_eq!(iou(&a, &a), 1.0);
        assert_eq!(iou(&a, &[20.0, 20.0, 30.0, 30.0]), 0.0);
        assert!((iou(&a, &[5.0, 0.0, 15.0, 10.0]) - 1.0 / 3.0).abs() < 1e-6);
    }

    /// Downloads the real model (40 MB) and runs it once.
    #[tokio::test]
    #[ignore = "downloads the model; run by hand with --ignored"]
    async fn the_published_model_downloads_checks_out_and_runs() {
        let root = std::env::temp_dir().join(format!("mp-panel-model-{}", std::process::id()));
        let path = download(&root, |_, _| {}).await.expect("download");
        assert!(is_downloaded(&root));
        let model = YoloPanels::load(&path).expect("load");
        let blank = GrayImage::new(800, 1200, 255).unwrap();
        assert!(model.detect(&blank).expect("run").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_model_lives_in_the_models_folder() {
        let root = Path::new("/data");
        assert_eq!(
            model_path(root),
            Path::new("/data/models").join(PANELS.file_name)
        );
        assert_eq!(PANELS.sha256.len(), 64);
    }
}
