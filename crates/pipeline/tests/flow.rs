//! The three stages end to end with a fake catalog: download into the
//! library, preview, then export. Output files are read back with the parser
//! that mirrors the reader's.

use catalog::fake::FakeCatalog;
use converter::decode::{decode_to_gray, image_dimensions};
use converter::xtc::{Format, ReadDirection, XtcFile};
use image::{ImageEncoder, Rgba, RgbaImage};
use library::{
    ConversionSettings, DownloadLimits, DownloadedSeries, LibraryDb, LibraryFiles, PageQuality,
};
use pipeline::{
    download_chapters, export_series, process_options_for, render_levels, CancelFlag, ChapterRef,
    ChapterRequest, ExportJob, ExportTarget, Grouping, JobEvent, PipelineError, RetryPolicy,
};
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

const TITLE: &str = "Test Series";

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

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mangapoint-flow-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn quick() -> RetryPolicy {
    RetryPolicy {
        attempts: 3,
        base_delay: std::time::Duration::from_millis(1),
    }
}

/// Register `chapter_id` with `pages` portrait pages served at `/c<id>/p<n>`.
fn add_chapter(c: &FakeCatalog, chapter_id: i64, pages: usize) {
    let paths: Vec<String> = (0..pages).map(|i| format!("/c{chapter_id}/p{i}")).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    c.set_pages(chapter_id, &refs);
    for (i, p) in paths.iter().enumerate() {
        c.set_bytes(
            p,
            png(300, 450, (chapter_id as u8).wrapping_mul(10) + i as u8),
        );
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

struct Library {
    root: PathBuf,
    files: LibraryFiles,
    id: String,
}

fn library(tag: &str) -> Library {
    let root = temp(tag);
    Library {
        files: LibraryFiles::new(root.join("library")),
        id: library::series_id(TITLE),
        root,
    }
}

impl Library {
    fn work(&self) -> PathBuf {
        self.root.join("work")
    }
    fn out(&self) -> PathBuf {
        self.root.join("out")
    }
    fn leftover_scratch(&self) -> usize {
        fs::read_dir(self.work()).map(|d| d.count()).unwrap_or(0)
    }
}

async fn download(
    c: &FakeCatalog,
    lib: &Library,
    reqs: &[ChapterRequest],
    quality: PageQuality,
) -> Result<pipeline::DownloadOutcome, PipelineError> {
    download_chapters(
        c,
        &lib.files,
        &lib.id,
        reqs,
        quality,
        DownloadLimits::default(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
}

fn series_from(
    outcome: &pipeline::DownloadOutcome,
    settings: ConversionSettings,
) -> DownloadedSeries {
    let mut db = LibraryDb::default();
    for ch in &outcome.chapters {
        db.record_chapter(TITLE, &settings, ch.clone(), 1);
    }
    db.series.remove(0)
}

fn job(lib: &Library, series: &DownloadedSeries, grouping: Grouping) -> ExportJob {
    let mut j = ExportJob::new(
        series.clone(),
        series.chapters.iter().map(|c| c.key.clone()).collect(),
        device_profiles::x4_pro(),
        ExportTarget::Folder { path: lib.out() },
        lib.work(),
    );
    j.grouping = grouping;
    // Most tests count pages, so the cover is left out unless one asks for it.
    j.cover = false;
    j
}

async fn export(lib: &Library, j: &ExportJob) -> Result<pipeline::ExportOutcome, PipelineError> {
    export_series(&lib.files, j, None, &CancelFlag::new()).await
}

fn names(outcome: &pipeline::ExportOutcome) -> Vec<String> {
    outcome
        .files
        .iter()
        .map(|f| {
            PathBuf::from(&f.location)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn gets(c: &FakeCatalog, prefix: &str) -> usize {
    c.calls()
        .iter()
        .filter(|x| x.starts_with(&format!("get {prefix}")))
        .count()
}

// ---- Stage 1: download -----------------------------------------------------

#[tokio::test]
async fn download_saves_every_page_and_reports_each_finished_chapter() {
    let lib = library("dl-basic");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    add_chapter(&c, 2, 2);
    let (tx, mut rx) = unbounded_channel();
    let out = download_chapters(
        &c,
        &lib.files,
        &lib.id,
        &[request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])],
        PageQuality::Full,
        DownloadLimits::default(),
        &quick(),
        Some(tx),
        &CancelFlag::new(),
    )
    .await
    .unwrap();

    assert_eq!(out.chapters.len(), 2);
    assert_eq!(
        (out.chapters[0].key.as_str(), out.chapters[0].pages),
        ("c1", 3)
    );
    assert_eq!(
        (out.chapters[1].key.as_str(), out.chapters[1].pages),
        ("c2", 2)
    );
    assert!(out.chapters.iter().all(|c| c.bytes > 0));
    assert_eq!(lib.files.page_files(&lib.id, "c1").len(), 3);
    // Pages are saved exactly as downloaded.
    assert_eq!(
        fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap(),
        png(300, 450, 10)
    );

    let mut saved = Vec::new();
    let mut page_events = 0;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            JobEvent::ChapterSaved { chapter } => saved.push(chapter.key),
            JobEvent::PageDone { .. } => page_events += 1,
            _ => {}
        }
    }
    assert_eq!(saved, ["c1", "c2"]);
    assert_eq!(page_events, 5);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn transient_download_errors_are_retried() {
    let lib = library("dl-flaky");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    c.flaky_path("/c1/p0", 2);
    let out = download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Full)
        .await
        .unwrap();
    assert_eq!(out.chapters[0].pages, 2);
    assert_eq!(gets(&c, "/c1/p0"), 3);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn an_interrupted_chapter_resumes_without_fetching_saved_pages_again() {
    let lib = library("dl-resume");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 3);
    c.set_pages(1, &["/c1/p0", "/c1/missing", "/c1/p2"]);
    c.set_bytes("/c1/p2", png(300, 450, 12));

    // The middle page is gone for now, so the chapter cannot finish.
    let first = download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Full).await;
    assert!(matches!(first, Err(PipelineError::NothingDownloaded)));
    assert_eq!(gets(&c, "/c1/p0"), 1);
    assert_eq!(gets(&c, "/c1/p2"), 1);

    // The source recovers; the same download continues where it stopped.
    c.set_bytes("/c1/missing", png(300, 450, 11));
    let out = download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Full)
        .await
        .unwrap();
    assert_eq!(out.chapters[0].pages, 3);
    assert_eq!(gets(&c, "/c1/p0"), 1, "page 1 was already saved");
    assert_eq!(gets(&c, "/c1/p2"), 1, "page 3 was already saved");
    assert_eq!(gets(&c, "/c1/missing"), 2);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_failing_source_falls_back_and_its_partial_pages_are_discarded() {
    let lib = library("dl-fallback");
    let c = FakeCatalog::new();
    // Source "bad": page 1 downloads, page 2 is gone. Source "good": complete.
    c.set_pages(10, &["/bad/p0", "/bad/p1"]);
    c.set_bytes("/bad/p0", png(300, 450, 99));
    add_chapter(&c, 20, 2);
    let (tx, mut rx) = unbounded_channel();
    let out = download_chapters(
        &c,
        &lib.files,
        &lib.id,
        &[request(1.0, &[("bad", 10), ("good", 20)])],
        PageQuality::Full,
        DownloadLimits::default(),
        &quick(),
        Some(tx),
        &CancelFlag::new(),
    )
    .await
    .unwrap();

    assert_eq!(out.chapters[0].source_id, "good");
    assert_eq!(out.chapters[0].pages, 2);
    assert_eq!(
        out.source_use
            .iter()
            .map(|u| (u.source_id.as_str(), u.ok))
            .collect::<Vec<_>>(),
        [("bad", false), ("good", true)]
    );
    // Page 1 is the good source's, not the half-downloaded page from the bad one.
    assert_eq!(
        fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap(),
        png(300, 450, 200)
    );
    let mut saw_fallback = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, JobEvent::UsedFallback { ref from, ref to, .. } if from == "bad" && to == "good")
        {
            saw_fallback = true;
        }
    }
    assert!(saw_fallback);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn chapters_no_source_can_supply_are_skipped_and_the_rest_are_saved() {
    let lib = library("dl-skip");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 1);
    add_chapter(&c, 3, 1);
    c.set_pages(2, &["/nowhere"]);
    let out = download(
        &c,
        &lib,
        &[
            request(1.0, &[("s", 1)]),
            request(2.0, &[("s", 2)]),
            request(3.0, &[("s", 3)]),
        ],
        PageQuality::Full,
    )
    .await
    .unwrap();
    assert_eq!(out.chapters.len(), 2);
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].name, "Chapter 2");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn nothing_downloading_and_empty_requests_are_clear_errors() {
    let lib = library("dl-errors");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/nowhere"]);
    assert!(matches!(
        download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Full).await,
        Err(PipelineError::NothingDownloaded)
    ));
    assert!(matches!(
        download(&c, &lib, &[], PageQuality::Full).await,
        Err(PipelineError::EmptyJob)
    ));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn an_error_page_instead_of_an_image_is_never_saved() {
    let lib = library("dl-html");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/bad"]);
    c.set_bytes("/bad", b"<html>access denied</html>".to_vec());
    add_chapter(&c, 2, 1);
    let out = download(&c, &lib, &[request(1.0, &[("a", 1), ("b", 2)])], PageQuality::Full)
        .await
        .unwrap();
    assert_eq!(out.chapters[0].source_id, "b");
    let saved = fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap();
    assert!(!saved.starts_with(b"<html"), "the error page was not kept");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn cancelling_stops_before_fetching_anything() {
    let lib = library("dl-cancel");
    let c = FakeCatalog::new();
    add_chapter(&c, 1, 2);
    let cancel = CancelFlag::new();
    cancel.cancel();
    let r = download_chapters(
        &c,
        &lib.files,
        &lib.id,
        &[request(1.0, &[("s", 1)])],
        PageQuality::Full,
        DownloadLimits::default(),
        &quick(),
        None,
        &cancel,
    )
    .await;
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    assert!(c.calls().is_empty());
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn save_space_shrinks_big_pages_and_keeps_small_ones_untouched() {
    let lib = library("dl-shrink");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/big", "/small"]);
    c.set_bytes("/big", png(1200, 4800, 0));
    c.set_bytes("/small", png(300, 450, 0));
    download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Save)
        .await
        .unwrap();

    let big = fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap();
    assert_eq!(&big[..3], &[0xFF, 0xD8, 0xFF], "re-encoded as JPEG");
    // 960 wide is the narrowest a page is made, so a tall one is not shrunk to 400.
    assert_eq!(image_dimensions(&big).unwrap(), (960, 3840));
    assert_eq!(
        fs::read(lib.files.page_path(&lib.id, "c1", 1)).unwrap(),
        png(300, 450, 0)
    );

    // Full quality: the big page is kept exactly as downloaded.
    let lib2 = library("dl-noshrink");
    download(&c, &lib2, &[request(1.0, &[("s", 1)])], PageQuality::Full)
        .await
        .unwrap();
    assert_eq!(
        fs::read(lib2.files.page_path(&lib2.id, "c1", 0)).unwrap(),
        png(1200, 4800, 0)
    );
    let _ = fs::remove_dir_all(lib.root);
    let _ = fs::remove_dir_all(lib2.root);
}

#[tokio::test]
async fn ultra_makes_every_page_small_and_keeps_an_original_that_is_already_smaller() {
    let lib = library("dl-ultra");
    let c = FakeCatalog::new();
    // A big colored page, a page already within the limits that is still much
    // bigger than it needs to be, and a tiny one a re-encode cannot beat.
    c.set_pages(1, &["/big", "/medium", "/tiny"]);
    c.set_bytes("/big", png(1200, 4800, 0));
    c.set_bytes("/medium", png(500, 800, 7));
    c.set_bytes("/tiny", png(8, 8, 0));
    download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Ultra)
        .await
        .unwrap();

    let page = |i| fs::read(lib.files.page_path(&lib.id, "c1", i)).unwrap();
    let big = page(0);
    assert_eq!(&big[..3], &[0xFF, 0xD8, 0xFF], "re-encoded as JPEG");
    // Never narrower than 600, so a tall strip is not shrunk to a sliver.
    assert_eq!(image_dimensions(&big).unwrap(), (600, 2400));
    let medium = page(1);
    assert_eq!(&medium[..3], &[0xFF, 0xD8, 0xFF], "a page within the limits is re-encoded too");
    assert_eq!(image_dimensions(&medium).unwrap(), (500, 800), "and not resized");
    assert!(medium.len() < png(500, 800, 7).len());
    assert_eq!(page(2), png(8, 8, 0), "an original a re-encode cannot beat is kept");
    let _ = fs::remove_dir_all(lib.root);
}

// ---- Stage 3: export -------------------------------------------------------

#[tokio::test]
async fn a_saved_cover_opens_every_file_as_its_own_page() {
    let (lib, series) = three_chapters("ex-cover").await;
    let plain = export(&lib, &job(&lib, &series, Grouping::PerChapter))
        .await
        .unwrap();
    let plain_pages: Vec<usize> = plain
        .files
        .iter()
        .map(|f| {
            let bytes = fs::read(&f.location).unwrap();
            XtcFile::parse(&bytes).unwrap().page_count()
        })
        .collect();

    let cover = png(400, 600, 9);
    let cover_path = lib.files.cover_path(&lib.id);
    fs::create_dir_all(cover_path.parent().unwrap()).unwrap();
    fs::write(&cover_path, &cover).unwrap();
    let mut j = job(&lib, &series, Grouping::PerChapter);
    j.cover = true;
    let with = export(&lib, &j).await.unwrap();
    assert_eq!(
        with.pages, plain.pages,
        "the cover is not one of the chapter pages"
    );
    for (file, before) in with.files.iter().zip(&plain_pages) {
        let bytes = fs::read(&file.location).unwrap();
        let book = XtcFile::parse(&bytes).unwrap();
        assert_eq!(book.page_count(), before + 1, "{}", file.location);
        let gray = decode_to_gray(&cover).unwrap();
        let mut opts = process_options_for(&j.device, &series.settings);
        opts.layout = converter::process::Layout::Manga;
        opts.crop = converter::process::CropMode::Full;
        opts.margins = Default::default();
        let expected = render_levels(&gray, &opts);
        assert_eq!(book.decode_page(0).unwrap().image, expected[0]);
    }
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_series_without_a_cover_opens_with_its_first_page() {
    let (lib, series) = three_chapters("ex-first-page").await;
    // No cover is saved, so the first page of the first chapter stands in.
    assert!(!lib.files.cover_path(&lib.id).exists());
    let mut j = job(&lib, &series, Grouping::Single);
    j.cover = true;
    let first = lib
        .files
        .page_files(&lib.id, &series.chapters[0].key)
        .into_iter()
        .next()
        .unwrap();
    let page = fs::read(first).unwrap();
    let with = export(&lib, &j).await.unwrap();
    let bytes = fs::read(&with.files[0].location).unwrap();
    let book = XtcFile::parse(&bytes).unwrap();
    assert_eq!(book.page_count(), with.pages + 1, "the stand-in cover is one more page");
    let gray = decode_to_gray(&page).unwrap();
    let mut opts = process_options_for(&j.device, &series.settings);
    opts.layout = converter::process::Layout::Manga;
    opts.crop = converter::process::CropMode::Full;
    opts.margins = Default::default();
    let expected = render_levels(&gray, &opts);
    assert_eq!(book.decode_page(0).unwrap().image, expected[0]);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn repeated_exports_reuse_converted_chapters_across_groupings() {
    let (lib, series) = three_chapters("ex-cache").await;
    let first = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap();
    let (tx, mut rx) = unbounded_channel();
    let second = export_series(
        &lib.files,
        &job(&lib, &series, Grouping::PerChapter),
        Some(tx),
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    let mut decoded = 0;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, JobEvent::PageDone { .. }) {
            decoded += 1;
        }
    }
    assert_eq!(decoded, 0, "unchanged chapters must not be decoded again");
    assert_eq!(first.cached_chapters, 0);
    assert_eq!(second.cached_chapters, 3);
    assert_eq!(second.pages, first.pages);
    let original = fs::read(&first.files[0].location).unwrap();
    let original = XtcFile::parse(&original).unwrap();
    for (ci, output) in second.files.iter().enumerate() {
        let bytes = fs::read(&output.location).unwrap();
        let file = XtcFile::parse(&bytes).unwrap();
        for pi in 0..file.page_count() {
            assert_eq!(
                file.decode_page(pi).unwrap(),
                original.decode_page(ci * 3 + pi).unwrap()
            );
        }
    }
    let mut subset = job(&lib, &series, Grouping::Single);
    subset.chapter_keys = vec!["c2".into()];
    let subset = export(&lib, &subset).await.unwrap();
    assert_eq!(
        (subset.cached_chapters, subset.chapters, subset.pages),
        (1, 1, 3)
    );
    let mut forced = job(&lib, &series, Grouping::Single);
    forced.force_conversion = true;
    let forced = export(&lib, &forced).await.unwrap();
    assert_eq!((forced.cached_chapters, forced.pages), (0, first.pages));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn conversion_cache_detects_source_changes_even_with_identical_size_and_timestamp() {
    let (lib, series) = three_chapters("ex-cache-source").await;
    let path = lib.files.page_path(&lib.id, "c2", 0);
    let mut original = fs::read(&path).unwrap();
    let mut replacement = png(300, 450, 200);
    let len = original.len().max(replacement.len());
    original.resize(len, 0);
    replacement.resize(len, 0);
    fs::write(&path, original).unwrap();
    let j = job(&lib, &series, Grouping::Single);
    let first = export(&lib, &j).await.unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, &replacement).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let second = export(&lib, &j).await.unwrap();
    assert_eq!(
        second.cached_chapters, 2,
        "only the changed chapter must be rebuilt"
    );
    let before = fs::read(&first.files[0].location).unwrap();
    let after = fs::read(&second.files[0].location).unwrap();
    let before = XtcFile::parse(&before).unwrap();
    let after = XtcFile::parse(&after).unwrap();
    assert_ne!(
        before.decode_page(3).unwrap(),
        after.decode_page(3).unwrap()
    );
    let gray = decode_to_gray(&replacement).unwrap();
    let expected = render_levels(&gray, &process_options_for(&j.device, &series.settings));
    assert_eq!(after.decode_page(3).unwrap().image, expected[0]);
    fs::remove_file(lib.files.page_path(&lib.id, "c2", 2)).unwrap();
    let third = export(&lib, &j).await.unwrap();
    assert_eq!((third.cached_chapters, third.pages), (2, 8));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn conversion_cache_invalidates_for_settings_device_and_format() {
    let (lib, series) = three_chapters("ex-cache-options").await;
    let mut j = job(&lib, &series, Grouping::Single);
    export(&lib, &j).await.unwrap();
    j.series.settings.contrast = 90;
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 0);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);
    j.device.width = 320;
    j.device.height = 512;
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 0);
    j.series.settings.format = Format::Xtch;
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 0);
    let final_run = export(&lib, &j).await.unwrap();
    assert_eq!(final_run.cached_chapters, 3);
    let bytes = fs::read(&final_run.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.format, Format::Xtch);
    let image = file.decode_page(0).unwrap().image;
    assert_eq!((image.width, image.height), (320, 512));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn conversion_cache_recovers_from_corruption_and_failed_cache_writes() {
    let (lib, series) = three_chapters("ex-cache-damage").await;
    let j = job(&lib, &series, Grouping::Single);
    let first = export(&lib, &j).await.unwrap();
    let cache = lib
        .files
        .chapter_dir(&lib.id, "c2")
        .join(".mangapoint-converted-v1.bin");
    let mut data = fs::read(&cache).unwrap();
    data[100] ^= 1;
    fs::write(&cache, data).unwrap();
    let repaired = export(&lib, &j).await.unwrap();
    assert_eq!(repaired.cached_chapters, 2);
    let original_bytes = fs::read(&first.files[0].location).unwrap();
    let repaired_bytes = fs::read(&repaired.files[0].location).unwrap();
    let original = XtcFile::parse(&original_bytes).unwrap();
    let repaired = XtcFile::parse(&repaired_bytes).unwrap();
    for pi in 0..original.page_count() {
        assert_eq!(
            original.decode_page(pi).unwrap(),
            repaired.decode_page(pi).unwrap()
        );
    }
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);
    fs::write(&cache, b"incomplete").unwrap();
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 2);
    fs::remove_file(&cache).unwrap();
    fs::create_dir(&cache).unwrap(); // Writing a file here fails, but export must work.
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 2);
    assert!(fs::read_dir(cache.parent().unwrap()).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|ext| ext != "part")
    }));
    let _ = fs::remove_dir_all(lib.root);
}

async fn three_chapters(tag: &str) -> (Library, DownloadedSeries) {
    let lib = library(tag);
    let c = FakeCatalog::new();
    for id in 1..=3 {
        add_chapter(&c, id, 3);
    }
    let reqs: Vec<_> = (1..=3).map(|n| request(n as f64, &[("s", n)])).collect();
    let out = download(&c, &lib, &reqs, PageQuality::Full).await.unwrap();
    let series = series_from(&out, ConversionSettings::default());
    (lib, series)
}

#[tokio::test]
async fn one_file_holds_every_chapter_with_a_working_chapter_menu() {
    let (lib, series) = three_chapters("ex-single").await;
    let out = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap();

    assert_eq!(names(&out), ["Test Series - Ch 1-3.xtc"]);
    assert_eq!((out.pages, out.chapters), (9, 3));
    let bytes = fs::read(&out.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    assert_eq!(file.format, Format::Xtc);
    assert_eq!(file.title, TITLE);
    assert_eq!(file.read_direction(), Some(ReadDirection::RightToLeft));
    let menu: Vec<_> = file
        .chapters
        .iter()
        .map(|c| (c.name.as_str(), c.start_page, c.end_page))
        .collect();
    assert_eq!(
        menu,
        [
            ("Chapter 1", 0, 2),
            ("Chapter 2", 3, 5),
            ("Chapter 3", 6, 8)
        ]
    );
    for i in 0..file.page_count() {
        let page = file.decode_page(i).unwrap();
        assert_eq!((page.image.width, page.image.height), (480, 800));
    }
    assert_eq!(lib.leftover_scratch(), 0, "temporary files are cleaned up");
    let _ = fs::remove_dir_all(lib.root);
}

/// Read an EPUB's text file, and list its picture names.
fn epub_text(path: &str, name: &str) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn epub_pictures(path: &str) -> Vec<String> {
    let zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .filter(|n| n.ends_with(".png") || n.ends_with(".jpg"))
        .map(String::from)
        .collect();
    names.sort_by_key(|n| n.trim_start_matches("OEBPS/").split('.').next().unwrap().parse::<usize>().unwrap());
    names
}

#[tokio::test]
async fn an_epub_has_every_page_the_chapters_and_the_readers_screen() {
    let (lib, series) = three_chapters("ex-epub").await;
    let mut j = job(&lib, &series, Grouping::Single);
    j.series.settings.set_output(library::OutputFormat::Epub);
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(names(&out), ["Test Series - Ch 1-3.epub"]);
    assert_eq!((out.pages, out.chapters), (9, 3));
    let path = out.files[0].location.clone();

    // An Xteink shows four grays, so its pages are 2-bit PNGs.
    let pictures = epub_pictures(&path);
    assert_eq!(pictures.len(), 9);
    assert!(pictures.iter().all(|n| n.ends_with(".png")), "{pictures:?}");
    let opf = epub_text(&path, "OEBPS/c.opf");
    assert!(opf.contains("page-progression-direction=\"rtl\""), "manga reads right to left");
    assert!(opf.contains("pre-paginated"));
    assert_eq!(opf.matches("<itemref").count(), 9);
    let nav = epub_text(&path, "OEBPS/n.xhtml");
    for (name, page) in [("Chapter 1", 1), ("Chapter 2", 4), ("Chapter 3", 7)] {
        assert!(nav.contains(&format!("<a href=\"{page}.xhtml\">{name}</a>")), "{nav}");
    }
    assert!(epub_text(&path, "OEBPS/1.xhtml").contains("width=480,height=800"));
    assert_eq!(lib.leftover_scratch(), 0, "temporary files are cleaned up");

    // The same export again comes from the cached pictures and is the same book.
    let again = export(&lib, &j).await.unwrap();
    assert_eq!(again.cached_chapters, 3);
    assert_eq!(epub_pictures(&again.files[0].location), pictures);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_cbz_has_every_page_in_order_and_a_comic_info_with_the_chapters() {
    let (lib, mut series) = three_chapters("ex-cbz").await;
    series.info = Some(library::SeriesInfo {
        description: "A test story.".into(),
        author: "A. Writer".into(),
        genres: vec!["Drama".into()],
        status: "Ongoing".into(),
        ..Default::default()
    });
    let mut j = job(&lib, &series, Grouping::Single);
    j.series.settings.set_output(library::OutputFormat::Cbz);
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(names(&out), ["Test Series - Ch 1-3.cbz"]);
    assert_eq!((out.pages, out.chapters), (9, 3));
    let path = out.files[0].location.clone();

    let zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let mut pictures: Vec<String> = zip.file_names().filter(|n| n.ends_with(".png") || n.ends_with(".jpg")).map(String::from).collect();
    pictures.sort();
    let expected: Vec<String> = (1..=9).map(|n| format!("{n:05}.png")).collect();
    assert_eq!(pictures, expected, "an Xteink shows four grays, so 2-bit PNGs, named to sort in order");
    let xml = epub_text(&path, "ComicInfo.xml");
    for want in [
        "<Series>Test Series</Series>",
        "<Summary>A test story.</Summary>",
        "<Writer>A. Writer</Writer>",
        "<Genre>Drama</Genre>",
        "<PageCount>9</PageCount>",
        "<Manga>YesAndRightToLeft</Manga>",
        "<Page Image=\"0\" Type=\"FrontCover\" Bookmark=\"Chapter 1\"",
        "<Page Image=\"3\" Bookmark=\"Chapter 2\"",
        "<Page Image=\"6\" Bookmark=\"Chapter 3\"",
        "ImageWidth=\"480\" ImageHeight=\"800\"",
    ] {
        assert!(xml.contains(want), "missing {want} in
{xml}");
    }
    assert_eq!(lib.leftover_scratch(), 0, "temporary files are cleaned up");

    // A second export comes from cached pictures and is the same book.
    let again = export(&lib, &j).await.unwrap();
    assert_eq!(again.cached_chapters, 3);
    assert_eq!(epub_text(&again.files[0].location, "ComicInfo.xml").matches("<Page ").count(), 9);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_cbz_for_each_chapter_is_that_issue_and_a_long_series_is_not_cut_at_800_pages() {
    let (lib, series) = three_chapters("ex-cbz-each").await;
    let mut j = job(&lib, &series, Grouping::PerChapter);
    j.series.settings.set_output(library::OutputFormat::Cbz);
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(out.files.len(), 3);
    let xml = epub_text(&out.files[1].location, "ComicInfo.xml");
    assert!(xml.contains("<Title>Chapter 2</Title>") && xml.contains("<Number>2</Number>"), "{xml}");
    assert!(xml.contains("<Series>Test Series</Series>"));

    // The EPUB limit of 800 pages does not apply to a CBZ.
    let mut j = job(&lib, &series, Grouping::Single);
    j.series.settings.set_output(library::OutputFormat::Cbz);
    j.max_pages_per_file = 5;
    let split = export(&lib, &j).await.unwrap();
    assert!(split.files.len() >= 2, "the job's own page limit still applies");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn an_epub_for_a_reader_with_many_grays_is_made_of_jpegs_at_its_size() {
    let (lib, series) = three_chapters("ex-epub-kindle").await;
    let mut j = job(&lib, &series, Grouping::PerChapter);
    j.device = device_profiles::known("kindle-paperwhite-12").unwrap();
    j.series.settings.set_output(library::OutputFormat::Epub);
    j.cover = true;
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(names(&out).len(), 3);
    let path = out.files[0].location.clone();
    let pictures = epub_pictures(&path);
    // The cover first, then the chapter's three pages.
    assert_eq!(pictures.len(), 4);
    assert!(pictures.iter().all(|n| n.ends_with(".jpg")), "{pictures:?}");
    let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name(&pictures[1]).unwrap(), &mut bytes).unwrap();
    let page = image::load_from_memory(&bytes).unwrap();
    assert_eq!((page.width(), page.height()), (1264, 1680));
    assert!(epub_text(&path, "OEBPS/1.xhtml").contains("width=1264,height=1680"));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_long_epub_is_cut_into_parts_a_reader_can_open() {
    let (lib, series) = three_chapters("ex-epub-parts").await;
    let mut j = job(&lib, &series, Grouping::Single);
    j.series.settings.set_output(library::OutputFormat::Epub);
    // Parts of at most four pages: three chapters of three pages each.
    j.max_pages_per_file = 4;
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(
        names(&out),
        [
            "Test Series - Part 1 - Ch 1.epub",
            "Test Series - Part 2 - Ch 2.epub",
            "Test Series - Part 3 - Ch 3.epub"
        ]
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn one_file_per_chapter_and_one_file_every_n_chapters() {
    let (lib, series) = three_chapters("ex-group").await;
    let per = export(&lib, &job(&lib, &series, Grouping::PerChapter))
        .await
        .unwrap();
    assert_eq!(
        names(&per),
        [
            "Test Series - Ch 1.xtc",
            "Test Series - Ch 2.xtc",
            "Test Series - Ch 3.xtc"
        ]
    );
    let one = XtcFile::parse(&fs::read(&per.files[1].location).unwrap())
        .map(|f| (f.page_count(), f.chapters.len()))
        .unwrap();
    assert_eq!(one, (3, 1));

    // Every 2 chapters: "Ch 1-2" and "Ch 3". The per-chapter run above already
    // wrote "Ch 3.xtc" into the same folder, so the new copy is numbered
    // rather than overwriting it.
    let every2 = export(&lib, &job(&lib, &series, Grouping::EveryN(2)))
        .await
        .unwrap();
    assert_eq!(
        names(&every2),
        ["Test Series - Ch 1-2.xtc", "Test Series - Ch 3 (2).xtc"]
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_subset_of_the_downloaded_chapters_can_be_exported() {
    let (lib, series) = three_chapters("ex-subset").await;
    let mut j = job(&lib, &series, Grouping::Single);
    j.chapter_keys = vec!["c3".into(), "c1".into()]; // order given does not matter
    let out = export(&lib, &j).await.unwrap();
    let file = XtcFile::parse(&fs::read(&out.files[0].location).unwrap())
        .map(|f| {
            f.chapters
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(file, ["Chapter 1", "Chapter 3"]);
    assert_eq!(names(&out), ["Test Series - Ch 1-3.xtc"]);

    j.chapter_keys = vec!["nope".into()];
    assert!(matches!(
        export(&lib, &j).await,
        Err(PipelineError::EmptyJob)
    ));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_page_limit_starts_a_new_file_with_part_numbers() {
    let (lib, series) = three_chapters("ex-limit").await;
    let mut j = job(&lib, &series, Grouping::Single);
    j.max_pages_per_file = 4; // two 3-page chapters would be 6
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(
        names(&out),
        [
            "Test Series - Part 1 - Ch 1.xtc",
            "Test Series - Part 2 - Ch 2.xtc",
            "Test Series - Part 3 - Ch 3.xtc"
        ]
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn series_wide_settings_decide_the_format_and_direction() {
    let (lib, mut series) = three_chapters("ex-settings").await;
    series.settings.format = Format::Xtc;
    series.settings.direction = ReadDirection::LeftToRight;
    let out = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap();
    assert!(names(&out)[0].ends_with(".xtc"));
    let file = XtcFile::parse(&fs::read(&out.files[0].location).unwrap())
        .map(|f| (f.format, f.read_direction()))
        .unwrap();
    assert_eq!(file, (Format::Xtc, Some(ReadDirection::LeftToRight)));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn the_preview_shows_exactly_the_pixels_the_export_writes() {
    let (lib, mut series) = three_chapters("ex-preview").await;
    series.settings.contrast = 70;
    series.settings.dither = converter::Dither::Atkinson;
    let out = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap();
    let file_bytes = fs::read(&out.files[0].location).unwrap();
    let file = XtcFile::parse(&file_bytes).unwrap();

    // Preview chapter 2, page 1 with the same settings.
    let saved = fs::read(lib.files.page_path(&lib.id, "c2", 0)).unwrap();
    let gray = decode_to_gray(&saved).unwrap();
    let levels = render_levels(
        &gray,
        &process_options_for(&device_profiles::x4_pro(), &series.settings),
    );
    assert_eq!(levels.len(), 1);
    // Chapter 2 starts at page 3 of the file.
    assert_eq!(file.decode_page(3).unwrap().image, levels[0]);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn held_sideways_an_xtc_book_keeps_the_readers_frame_and_a_cbz_has_wide_pictures() {
    let (lib, series) = three_chapters("ex-landscape").await;
    let mut j = job(&lib, &series, Grouping::Single);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 0);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);

    // Sideways is its own conversion: nothing cached for upright is reused.
    j.series.settings.orientation = library::Orientation::Landscape;
    let sideways = export(&lib, &j).await.unwrap();
    assert_eq!(sideways.cached_chapters, 0);
    let bytes = fs::read(&sideways.files[0].location).unwrap();
    let image = XtcFile::parse(&bytes).unwrap().decode_page(0).unwrap().image;
    assert_eq!((image.width, image.height), (480, 800), "the file holds the reader's own frame");
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);

    // A CBZ made for a sideways reader has pictures the shape of its screen.
    j.series.settings.set_output(library::OutputFormat::Cbz);
    let out = export(&lib, &j).await.unwrap();
    let xml = epub_text(&out.files[0].location, "ComicInfo.xml");
    assert!(xml.contains("ImageWidth=\"800\" ImageHeight=\"480\""), "{xml}");

    // Back upright, the pages are made again (a chapter keeps one conversion)
    // and then reused.
    j.series.settings = ConversionSettings::default();
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 0);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_double_page_spread_is_exported_as_two_pages() {
    let lib = library("ex-spread");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/spread"]);
    c.set_bytes("/spread", png(900, 450, 0));
    let out = download(&c, &lib, &[request(1.0, &[("s", 1)])], PageQuality::Full)
        .await
        .unwrap();
    let series = series_from(&out, ConversionSettings::default());
    let exported = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap();
    assert_eq!(exported.pages, 2);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn an_unreadable_saved_page_says_which_one_and_what_to_do() {
    let (lib, series) = three_chapters("ex-corrupt").await;
    fs::write(lib.files.page_path(&lib.id, "c2", 1), b"garbage").unwrap();
    let err = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("page 2 of Chapter 2"), "{message}");
    assert!(message.contains("download it again"), "{message}");
    assert_eq!(
        lib.leftover_scratch(),
        0,
        "a failed export leaves no scratch files"
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn deleted_pages_are_reported_not_silently_skipped() {
    let (lib, series) = three_chapters("ex-missing").await;
    lib.files.remove_chapter(&lib.id, "c2").unwrap();
    let err = export(&lib, &job(&lib, &series, Grouping::Single))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Chapter 2 has no saved pages"),
        "{err}"
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn cancelling_an_export_leaves_nothing_behind() {
    let (lib, series) = three_chapters("ex-cancel").await;
    let cancel = CancelFlag::new();
    cancel.cancel();
    let r = export_series(
        &lib.files,
        &job(&lib, &series, Grouping::Single),
        None,
        &cancel,
    )
    .await;
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    assert!(!lib.out().exists() || fs::read_dir(lib.out()).unwrap().next().is_none());
    assert_eq!(lib.leftover_scratch(), 0);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn exporting_to_a_reader_puts_the_file_in_its_manga_folder() {
    let (lib, series) = three_chapters("ex-device").await;
    let card = lib.root.join("card");
    fs::create_dir_all(&card).unwrap();
    let mut j = job(&lib, &series, Grouping::Single);
    j.target = ExportTarget::Device {
        mount: card.clone(),
        folder: "Manga".into(),
    };
    let out = export(&lib, &j).await.unwrap();
    assert!(PathBuf::from(&out.files[0].location).starts_with(card.join("Manga")));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn export_reports_progress_for_every_page_and_file() {
    let (lib, series) = three_chapters("ex-events").await;
    let (tx, mut rx) = unbounded_channel();
    export_series(
        &lib.files,
        &job(&lib, &series, Grouping::PerChapter),
        Some(tx),
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    let (mut pages, mut writes, mut starts) = (0, 0, 0);
    while let Ok(ev) = rx.try_recv() {
        match ev {
            JobEvent::PageDone { .. } => pages += 1,
            JobEvent::Writing { .. } => writes += 1,
            JobEvent::ChapterStarted { .. } => starts += 1,
            _ => {}
        }
    }
    assert_eq!((pages, writes, starts), (9, 3, 3));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_lone_unnumbered_chapter_is_named_after_itself() {
    let lib = library("ex-unnumbered");
    let c = FakeCatalog::new();
    add_chapter(&c, 5, 1);
    let mut req = request(1.0, &[("s", 5)]);
    req.number = None;
    req.name = "Author's note".into();
    let out = download(&c, &lib, &[req], PageQuality::Full).await.unwrap();
    let series = series_from(&out, ConversionSettings::default());
    let exported = export(&lib, &job(&lib, &series, Grouping::PerChapter))
        .await
        .unwrap();
    assert_eq!(names(&exported), ["Test Series - Author's note.xtc"]);
    let _ = fs::remove_dir_all(lib.root);
}

// ---- Parallel downloads ----------------------------------------------------

fn limits(chapters: usize, pages: usize) -> DownloadLimits {
    DownloadLimits {
        chapters,
        pages,
        spread_sources: true,
        race_sources: false,
    }
}

async fn download_with(
    c: &FakeCatalog,
    lib: &Library,
    reqs: &[ChapterRequest],
    limits: DownloadLimits,
    events: Option<tokio::sync::mpsc::UnboundedSender<JobEvent>>,
) -> Result<pipeline::DownloadOutcome, PipelineError> {
    download_chapters(
        c,
        &lib.files,
        &lib.id,
        reqs,
        PageQuality::Full,
        limits,
        &quick(),
        events,
        &CancelFlag::new(),
    )
    .await
}

#[tokio::test]
async fn pages_download_at_once_up_to_the_chosen_number_and_no_more() {
    for pages in [1, 2, 4] {
        let lib = library(&format!("par-pages-{pages}"));
        let c = FakeCatalog::new();
        c.set_latency_ms(15);
        add_chapter(&c, 1, 12);
        let out = download_with(
            &c,
            &lib,
            &[request(1.0, &[("s", 1)])],
            limits(1, pages),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.chapters[0].pages, 12);
        assert_eq!(c.max_in_flight(), pages, "pages at once = {pages}");
        let _ = fs::remove_dir_all(lib.root);
    }
}

#[tokio::test]
async fn the_page_allowance_is_shared_by_chapters_downloading_together() {
    let lib = library("par-shared");
    let c = FakeCatalog::new();
    c.set_latency_ms(15);
    for id in 1..=3 {
        add_chapter(&c, id, 8);
    }
    let reqs = [
        request(1.0, &[("s", 1)]),
        request(2.0, &[("s", 2)]),
        request(3.0, &[("s", 3)]),
    ];
    let (tx, mut rx) = unbounded_channel();
    let out = download_with(&c, &lib, &reqs, limits(3, 6), Some(tx))
        .await
        .unwrap();
    assert_eq!(
        c.max_in_flight(),
        6,
        "six requests in all, not six per chapter"
    );
    let keys: Vec<_> = out.chapters.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(
        keys,
        ["c1", "c2", "c3"],
        "reading order, however they finished"
    );

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    let first_saved = events
        .iter()
        .position(|e| matches!(e, JobEvent::ChapterSaved { .. }))
        .unwrap();
    let started_before = events[..first_saved]
        .iter()
        .filter(|e| matches!(e, JobEvent::ChapterStarted { .. }))
        .count();
    assert_eq!(
        started_before, 3,
        "all three chapters were under way together"
    );
    let done = events
        .iter()
        .filter(|e| matches!(e, JobEvent::ChapterDone { .. }))
        .count();
    assert_eq!(done, 3);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn one_chapter_at_a_time_means_one_at_a_time() {
    let lib = library("par-serial");
    let c = FakeCatalog::new();
    c.set_latency_ms(10);
    add_chapter(&c, 1, 4);
    add_chapter(&c, 2, 4);
    let (tx, mut rx) = unbounded_channel();
    download_with(
        &c,
        &lib,
        &[request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])],
        limits(1, 4),
        Some(tx),
    )
    .await
    .unwrap();
    let mut order = Vec::new();
    while let Ok(e) = rx.try_recv() {
        match e {
            JobEvent::ChapterStarted { index, .. } => order.push(format!("start {index}")),
            JobEvent::ChapterDone { index } => order.push(format!("done {index}")),
            _ => {}
        }
    }
    assert_eq!(order, ["start 0", "done 0", "start 1", "done 1"]);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_source_that_pushes_back_makes_the_download_ease_off_and_still_finish() {
    let lib = library("par-throttle");
    let c = FakeCatalog::new();
    c.set_latency_ms(5);
    add_chapter(&c, 1, 10);
    // Several pages answer "overloaded" once before working.
    for i in [0, 1, 2, 3] {
        c.flaky_path(&format!("/c1/p{i}"), 1);
    }
    let (tx, mut rx) = unbounded_channel();
    let out = download_with(
        &c,
        &lib,
        &[request(1.0, &[("s", 1)])],
        limits(1, 8),
        Some(tx),
    )
    .await
    .unwrap();
    assert_eq!(out.chapters[0].pages, 10, "every page still arrived");

    let mut lowest = usize::MAX;
    while let Ok(e) = rx.try_recv() {
        if let JobEvent::PagesAtOnce { now, chosen } = e {
            assert_eq!(chosen, 8);
            lowest = lowest.min(now);
        }
    }
    assert!(lowest < 8, "the download eased off (lowest was {lowest})");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_cancel_during_parallel_downloads_keeps_what_finished() {
    let lib = library("par-cancel");
    let c = FakeCatalog::new();
    c.set_latency_ms(10);
    add_chapter(&c, 1, 3);
    add_chapter(&c, 2, 30);
    let cancel = CancelFlag::new();
    let (tx, mut rx) = unbounded_channel();
    let reqs = [request(1.0, &[("s", 1)]), request(2.0, &[("s", 2)])];
    let stopper = cancel.clone();
    let watch = async move {
        // Cancel as soon as the first chapter is saved.
        while let Some(e) = rx.recv().await {
            if matches!(e, JobEvent::ChapterSaved { .. }) {
                stopper.cancel();
            }
        }
    };
    let policy = quick();
    let run = download_chapters(
        &c,
        &lib.files,
        &lib.id,
        &reqs,
        PageQuality::Full,
        limits(2, 4),
        &policy,
        Some(tx),
        &cancel,
    );
    let (result, ()) = tokio::join!(run, watch);
    assert!(matches!(result, Err(PipelineError::Cancelled)));
    assert!(
        lib.files.page_files(&lib.id, "c1").len() == 3,
        "the chapter that finished is on disk"
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn chapters_offered_by_two_sources_are_shared_between_them() {
    let lib = library("spread");
    let c = FakeCatalog::new();
    c.set_latency_ms(10);
    // Chapters 1-3 exist on source "a" (ids 1-3) and on source "b" (ids 11-13).
    for id in [1, 2, 3, 11, 12, 13] {
        add_chapter(&c, id, 4);
    }
    let reqs = [
        request(1.0, &[("a", 1), ("b", 11)]),
        request(2.0, &[("a", 2), ("b", 12)]),
        request(3.0, &[("a", 3), ("b", 13)]),
    ];
    let out = download_with(&c, &lib, &reqs, limits(3, 6), None)
        .await
        .unwrap();
    let sources: Vec<_> = out.chapters.iter().map(|c| c.source_id.as_str()).collect();
    assert_eq!(
        sources,
        ["a", "b", "a"],
        "the second chapter went to the free source"
    );
    let _ = fs::remove_dir_all(lib.root);

    // Switched off, everything comes from the best source.
    let lib = library("spread-off");
    let off = DownloadLimits {
        spread_sources: false,
        ..limits(3, 6)
    };
    let out = download_with(&c, &lib, &reqs, off, None).await.unwrap();
    assert!(out.chapters.iter().all(|c| c.source_id == "a"));
    let _ = fs::remove_dir_all(lib.root);
}

// ---- Racing every source, and cancelling ------------------------------------

fn racing(chapters: usize, pages: usize) -> DownloadLimits {
    DownloadLimits {
        race_sources: true,
        ..limits(chapters, pages)
    }
}

#[tokio::test]
async fn the_fastest_source_wins_and_the_slow_one_is_stopped_and_cleaned_up() {
    let lib = library("race-win");
    let c = FakeCatalog::new();
    // Source "slow" (chapter 1) answers in 60 ms a page, "quick" (chapter 11) in 2.
    for id in [1, 11] {
        add_chapter(&c, id, 8);
    }
    c.set_path_latency_ms("/c1/", 60);
    c.set_path_latency_ms("/c11/", 2);
    let (tx, mut rx) = unbounded_channel();
    let out = download_with(
        &c,
        &lib,
        &[request(1.0, &[("slow", 1), ("quick", 11)])],
        racing(1, 2),
        Some(tx),
    )
    .await
    .unwrap();

    assert_eq!(out.chapters[0].source_id, "quick");
    assert_eq!(out.chapters[0].pages, 8);
    assert_eq!(
        lib.files.page_files(&lib.id, "c1").len(),
        8,
        "pages are in the chapter's own folder"
    );
    let slow_requests = c
        .calls()
        .iter()
        .filter(|x| x.starts_with("get /c1/"))
        .count();
    assert!(
        slow_requests < 8,
        "the slow source was stopped early ({slow_requests} requests)"
    );

    // Only the finished chapter's folder is left behind: no staging folders.
    let series_dir = lib.files.series_dir(&lib.id);
    let folders: Vec<_> = fs::read_dir(&series_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(folders, ["c1"], "{folders:?}");

    // Losing a race is not held against the slow source.
    assert!(out.source_use.iter().all(|u| u.ok));
    // Progress never goes backwards while the sources race.
    let mut last = 0;
    while let Ok(e) = rx.try_recv() {
        if let JobEvent::PageDone { done, .. } = e {
            assert!(done > last, "{done} after {last}");
            last = done;
        }
    }
    assert_eq!(last, 8);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_race_still_succeeds_when_all_but_one_source_fail() {
    let lib = library("race-fail");
    let c = FakeCatalog::new();
    // "bad" lists a page that does not exist; "good" is complete.
    c.set_pages(10, &["/bad/p0"]);
    add_chapter(&c, 20, 3);
    let out = download_with(
        &c,
        &lib,
        &[request(1.0, &[("bad", 10), ("good", 20)])],
        racing(1, 4),
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.chapters[0].source_id, "good");
    let bad = out
        .source_use
        .iter()
        .find(|u| u.source_id == "bad")
        .unwrap();
    assert!(
        !bad.ok,
        "a source that really failed is still recorded as failed"
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_race_where_every_source_fails_skips_the_chapter() {
    let lib = library("race-none");
    let c = FakeCatalog::new();
    c.set_pages(10, &["/a/p0"]);
    c.set_pages(20, &["/b/p0"]);
    let r = download_with(
        &c,
        &lib,
        &[request(1.0, &[("a", 10), ("b", 20)])],
        racing(1, 4),
        None,
    )
    .await;
    assert!(matches!(r, Err(PipelineError::NothingDownloaded)));
    assert!(
        fs::read_dir(lib.files.series_dir(&lib.id))
            .map(|d| d.count())
            .unwrap_or(0)
            == 0
            || lib.files.page_files(&lib.id, "c1").is_empty()
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn cancelling_stops_at_once_even_with_requests_in_flight() {
    let lib = library("cancel-fast");
    let c = FakeCatalog::new();
    c.set_latency_ms(1500); // every page takes 1.5 s
    add_chapter(&c, 1, 6);
    let cancel = CancelFlag::new();
    let stopper = cancel.clone();
    let policy = quick();
    let reqs = [request(1.0, &[("s", 1)])];
    let run = download_chapters(
        &c,
        &lib.files,
        &lib.id,
        &reqs,
        PageQuality::Full,
        limits(1, 6),
        &policy,
        None,
        &cancel,
    );
    let started = std::time::Instant::now();
    let press = async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        stopper.cancel();
    };
    let (result, ()) = tokio::join!(run, press);
    assert!(matches!(result, Err(PipelineError::Cancelled)));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(600),
        "took {:?}",
        started.elapsed()
    );
    let _ = fs::remove_dir_all(lib.root);
}

// ---- One source per line of chapters ----------------------------------------

/// The limits a normal download uses: no mixing of sources.
fn separate(chapters: usize, pages: usize) -> DownloadLimits {
    DownloadLimits {
        spread_sources: false,
        ..limits(chapters, pages)
    }
}

#[tokio::test]
async fn a_slow_source_does_not_hold_back_a_fast_one() {
    // Interleaved like a real series: whole numbers on a slow source, ".1"
    // chapters on a fast one. Every slow page takes 400 ms.
    let lib = library("lines");
    let c = FakeCatalog::new();
    for id in [1, 3, 5] {
        add_chapter(&c, id, 30);
        c.set_path_latency_ms(&format!("/c{id}/"), 400);
    }
    for id in [2, 4, 6] {
        add_chapter(&c, id, 10);
        c.set_path_latency_ms(&format!("/c{id}/"), 5);
    }
    let reqs = [
        request(1.0, &[("slow", 1)]),
        request(1.5, &[("fast", 2)]),
        request(2.0, &[("slow", 3)]),
        request(2.5, &[("fast", 4)]),
        request(3.0, &[("slow", 5)]),
        request(3.5, &[("fast", 6)]),
    ];
    let (tx, mut rx) = unbounded_channel();
    let started = std::time::Instant::now();
    let run = download_with(&c, &lib, &reqs, separate(6, 12), Some(tx));
    let watch = async {
        let mut fast_done = Vec::new();
        while let Some(e) = rx.recv().await {
            if let JobEvent::ChapterSaved { chapter } = e {
                if ["c1_5", "c2_5", "c3_5"].contains(&chapter.key.as_str()) {
                    fast_done.push(started.elapsed());
                }
            }
        }
        fast_done
    };
    let (out, fast_done) = tokio::join!(run, watch);
    out.unwrap();
    assert_eq!(fast_done.len(), 3);
    let last_fast = fast_done.iter().max().unwrap();
    assert!(
        *last_fast < std::time::Duration::from_millis(500),
        "the fast chapters waited for the slow source: {last_fast:?}"
    );
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn chapters_start_only_when_their_sources_line_has_room() {
    // Three chapters from one source, one at a time: they run one after another
    // even though another source is idle.
    let lib = library("lines-serial");
    let c = FakeCatalog::new();
    c.set_latency_ms(10);
    for id in [1, 2, 3, 4] {
        add_chapter(&c, id, 4);
    }
    let reqs = [
        request(1.0, &[("a", 1)]),
        request(2.0, &[("a", 2)]),
        request(3.0, &[("b", 3)]),
        request(4.0, &[("a", 4)]),
    ];
    let (tx, mut rx) = unbounded_channel();
    download_with(&c, &lib, &reqs, separate(1, 4), Some(tx))
        .await
        .unwrap();
    let mut order = Vec::new();
    while let Ok(e) = rx.try_recv() {
        match e {
            JobEvent::ChapterStarted { index, .. } => order.push(format!("start {index}")),
            JobEvent::ChapterDone { index } => order.push(format!("done {index}")),
            _ => {}
        }
    }
    let position = |what: &str| order.iter().position(|e| e == what).unwrap();
    // Source "a" never has two chapters under way at once.
    assert!(position("done 0") < position("start 1"), "{order:?}");
    assert!(position("done 1") < position("start 3"), "{order:?}");
    // Source "b" did not have to wait for "a" to finish.
    assert!(position("start 2") < position("done 1"), "{order:?}");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn manual_crop_persists_matches_export_and_invalidates_only_affected_cache() {
    use library::{CropRect, CropScope};
    let (lib, series) = three_chapters("manual-crop-export").await;
    let mut j = job(&lib, &series, Grouping::Single);
    export(&lib, &j).await.unwrap();
    let path = lib.files.page_path(&lib.id, "c2", 0);
    let original = fs::read(&path).unwrap();
    let opts = process_options_for(&j.device, &j.series.settings);
    let chapter = pipeline::prepare_chapter(&[decode_to_gray(&original).unwrap()], &opts);
    let page = &chapter.plan.pages[0];
    let crop = CropRect {
        left: 0.2,
        top: 0.1,
        ..Default::default()
    };
    j.series.crops.set(
        CropScope::Page,
        "c2",
        &pipeline::crop_page_key(page),
        Some(crop),
    );
    j.series = serde_json::from_str(&serde_json::to_string(&j.series).unwrap()).unwrap();
    let out = export(&lib, &j).await.unwrap();
    assert_eq!(out.cached_chapters, 2);
    let bytes = fs::read(&out.files[0].location).unwrap();
    let file = XtcFile::parse(&bytes).unwrap();
    let actual = file.decode_page(3).unwrap().image;
    let expected =
        converter::plan::render_page_cropped(&chapter.prepared, page, &opts, crop.as_array());
    assert_eq!(actual, expected);
    let preview = pipeline::render_planned_with_crop(&chapter.prepared, page, &opts, Some(crop));
    assert_eq!(preview, pipeline::levels_to_previews(&[actual])[0]);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 3);
    j.series
        .crops
        .set(CropScope::Page, "c2", &pipeline::crop_page_key(page), None);
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 2);
    j.series.crops.set(CropScope::Chapter, "c2", "", Some(crop));
    assert_eq!(export(&lib, &j).await.unwrap().cached_chapters, 2);
    j.series.crops.set(
        CropScope::Series,
        "",
        "",
        Some(CropRect {
            right: 0.1,
            ..Default::default()
        }),
    );
    assert_eq!(
        export(&lib, &j).await.unwrap().cached_chapters,
        1,
        "chapter override masks series changes"
    );
    let _ = fs::remove_dir_all(lib.root);
}
