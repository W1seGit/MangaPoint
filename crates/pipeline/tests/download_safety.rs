//! Download hardening: page size limits, leftover temp files, and staging
//! folders of a cancelled race.

use catalog::fake::FakeCatalog;
use image::{ImageEncoder, Rgba, RgbaImage};
use library::{DownloadLimits, LibraryFiles, PageQuality};
use pipeline::{
    download_chapters, CancelFlag, ChapterRef, ChapterRequest, PipelineError, RetryPolicy,
};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let v = ((x * 3 + y * 2) as u8).wrapping_add(seed);
        Rgba([v, v, v, 255])
    });
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A small PNG whose header claims `w` x `h` pixels (the picture data is not
/// there; only the size is read before a page is accepted).
fn png_claiming(w: u32, h: u32) -> Vec<u8> {
    let mut bytes = png(8, 8, 0);
    // Signature (8) + length (4) + "IHDR" (4), then width and height.
    bytes[16..20].copy_from_slice(&w.to_be_bytes());
    bytes[20..24].copy_from_slice(&h.to_be_bytes());
    let crc = crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    bytes
}

struct Lib {
    root: PathBuf,
    files: LibraryFiles,
    id: String,
}

fn lib(tag: &str) -> Lib {
    let root = std::env::temp_dir().join(format!("mangapoint-dlsafe-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    Lib {
        files: LibraryFiles::new(root.join("library")),
        id: library::series_id("Safety Series"),
        root,
    }
}

fn quick() -> RetryPolicy {
    RetryPolicy {
        attempts: 1,
        base_delay: Duration::from_millis(1),
    }
}

fn add_chapter(c: &FakeCatalog, id: i64, pages: usize) {
    let paths: Vec<String> = (0..pages).map(|i| format!("/c{id}/p{i}")).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    c.set_pages(id, &refs);
    for (i, p) in paths.iter().enumerate() {
        c.set_bytes(p, png(40, 60, i as u8));
    }
}

fn request(number: f64, options: &[(&str, i64)]) -> ChapterRequest {
    ChapterRequest {
        number: Some(number),
        name: format!("Chapter {number}"),
        key: None,
        options: options
            .iter()
            .map(|(s, id)| ChapterRef {
                source_id: s.to_string(),
                chapter_id: *id,
            })
            .collect(),
    }
}

fn limits(race: bool) -> DownloadLimits {
    DownloadLimits {
        chapters: 1,
        pages: 3,
        spread_sources: true,
        race_sources: race,
    }
}

async fn run(
    c: &FakeCatalog,
    l: &Lib,
    reqs: &[ChapterRequest],
    limits: DownloadLimits,
    cancel: &CancelFlag,
) -> Result<pipeline::DownloadOutcome, PipelineError> {
    download_chapters(
        c,
        &l.files,
        &l.id,
        reqs,
        PageQuality::Full,
        limits,
        &quick(),
        None,
        cancel,
    )
    .await
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[tokio::test]
async fn many_pages_all_land_as_files_with_no_temp_files() {
    let l = lib("many");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 40);
    let out = run(&c, &l, &[request(1.0, &[("a", 1)])], limits(false), &CancelFlag::new())
        .await
        .unwrap();
    assert_eq!(out.chapters[0].pages, 40);
    let found = names(&l.files.chapter_dir(&l.id, "c1"));
    assert_eq!(found.iter().filter(|n| n.ends_with(".img")).count(), 40, "{found:?}");
    assert!(found.iter().all(|n| !n.ends_with(".part")), "{found:?}");
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn a_page_with_too_many_pixels_fails_at_download_time_with_a_clear_message() {
    let l = lib("pixels");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/c1/p0"]);
    c.set_bytes("/c1/p0", png_claiming(20_000, 20_000));
    let err = run(&c, &l, &[request(1.0, &[("a", 1)])], limits(false), &CancelFlag::new())
        .await
        .unwrap_err();
    // The only source failed, so nothing was downloaded; the page was not kept.
    assert!(matches!(err, PipelineError::NothingDownloaded), "{err:?}");
    assert!(l.files.page_files(&l.id, "c1").is_empty());
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn the_reason_for_a_refused_page_is_given() {
    let l = lib("pixels-reason");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/c1/p0"]);
    c.set_bytes("/c1/p0", png_claiming(20_000, 20_000));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = download_chapters(
        &c,
        &l.files,
        &l.id,
        &[request(1.0, &[("a", 1)])],
        PageQuality::Full,
        limits(false),
        &quick(),
        Some(tx),
        &CancelFlag::new(),
    )
    .await;
    let mut reason = String::new();
    while let Ok(e) = rx.try_recv() {
        if let pipeline::JobEvent::ChapterSkipped { reason: r, .. } = e {
            reason = r;
        }
    }
    assert!(reason.contains("megapixels"), "{reason}");
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn a_huge_download_body_is_refused_before_it_is_written() {
    let l = lib("bytes");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/c1/p0"]);
    let mut big = png(8, 8, 0);
    big.resize(65 * 1024 * 1024, 0);
    c.set_bytes("/c1/p0", big);
    let err = run(&c, &l, &[request(1.0, &[("a", 1)])], limits(false), &CancelFlag::new())
        .await
        .unwrap_err();
    assert!(matches!(err, PipelineError::NothingDownloaded), "{err:?}");
    assert!(l.files.page_files(&l.id, "c1").is_empty());
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn leftover_part_files_in_the_chapter_folder_are_swept_when_it_resumes() {
    let l = lib("sweep");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    add_chapter(&c, 2, 1);
    // A crashed earlier run of chapter 1 (same source copy) and an unrelated
    // chapter folder that must be left alone.
    let dir = l.files.chapter_dir(&l.id, "c1");
    let other = l.files.chapter_dir(&l.id, "c2");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&other).unwrap();
    fs::write(dir.join("source.txt"), "a:1").unwrap();
    fs::write(dir.join("00002.part"), b"half").unwrap();
    fs::write(dir.join("00003.123-4.part"), b"half").unwrap();
    fs::write(other.join("keep.part"), b"not ours").unwrap();
    run(&c, &l, &[request(1.0, &[("a", 1)])], limits(false), &CancelFlag::new())
        .await
        .unwrap();
    let found = names(&dir);
    assert!(found.iter().all(|n| !n.ends_with(".part")), "{found:?}");
    assert_eq!(found.iter().filter(|n| n.ends_with(".img")).count(), 3);
    assert_eq!(names(&other), ["keep.part"], "other folders are not touched");
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn a_cancelled_race_leaves_no_staging_folders() {
    let l = lib("race-cancel");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 30);
    add_chapter(&c, 11, 30);
    c.set_path_latency_ms("/c1/", 40);
    c.set_path_latency_ms("/c11/", 40);
    let cancel = CancelFlag::new();
    let flag = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        flag.cancel();
    });
    let err = run(
        &c,
        &l,
        &[request(1.0, &[("s1", 1), ("s2", 11)])],
        limits(true),
        &cancel,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PipelineError::Cancelled), "{err:?}");
    let series = l.files.series_dir(&l.id);
    // Nothing may be left, in particular no `<key>-via-<source>` folders.
    let left = names(&series);
    assert!(left.iter().all(|n| !n.contains("-via-")), "{left:?}");
    let _ = fs::remove_dir_all(l.root);
}
