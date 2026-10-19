//! Keeping colour: a colour reader's EPUB and CBZ are made of colour JPEGs, an
//! XTC is never changed by the choice, gray and colour conversions never share
//! a cache, and a download can keep the colour of the pages it shrinks.

use catalog::fake::FakeCatalog;
use converter::decode::{decode_to_ycc, image_dimensions};
use converter::process::CropMode;
use converter::xtc::XtcFile;
use image::{ImageEncoder, Rgb, RgbImage};
use library::{
    ConversionSettings, DownloadLimits, DownloadedSeries, LibraryDb, LibraryFiles, OutputFormat,
    PageQuality,
};
use pipeline::{
    download_chapters, export_series, CancelFlag, ChapterRef, ChapterRequest, ExportJob,
    ExportTarget, Grouping, RetryPolicy,
};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

const TITLE: &str = "Colour Series";

/// A portrait page: red above, blue below, a green stripe across the middle
/// and nothing else, so what a reader should show is easy to name.
fn colour_page(w: u32, h: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |_, y| {
        if y.abs_diff(h / 2) < h / 40 {
            Rgb([20, 200, 40])
        } else if y < h / 2 {
            Rgb([220, 30, 30])
        } else {
            Rgb([30, 30, 220])
        }
    });
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mangapoint-color-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn quick() -> RetryPolicy {
    RetryPolicy {
        attempts: 2,
        base_delay: std::time::Duration::from_millis(1),
    }
}

struct Lib {
    root: PathBuf,
    files: LibraryFiles,
    id: String,
}

impl Lib {
    fn new(tag: &str) -> Lib {
        let root = temp(tag);
        Lib {
            files: LibraryFiles::new(root.join("library")),
            id: library::series_id(TITLE),
            root,
        }
    }
    fn out(&self) -> PathBuf {
        self.root.join("out")
    }
    fn work(&self) -> PathBuf {
        self.root.join("work")
    }
}

fn request(number: f64, id: i64) -> ChapterRequest {
    ChapterRequest {
        number: Some(number),
        name: format!("Chapter {number}"),
        key: None,
        options: vec![ChapterRef {
            source_id: "s".into(),
            chapter_id: id,
        }],
    }
}

async fn download(
    c: &FakeCatalog,
    lib: &Lib,
    quality: PageQuality,
    keep_color: bool,
    chapters: &[ChapterRequest],
) -> pipeline::DownloadOutcome {
    let limits = DownloadLimits {
        keep_color,
        ..DownloadLimits::default()
    };
    download_chapters(
        c,
        &lib.files,
        &lib.id,
        chapters,
        quality,
        limits,
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap()
}

/// Two chapters of three colour pages, saved exactly as downloaded.
async fn colour_series(tag: &str) -> (Lib, DownloadedSeries) {
    let lib = Lib::new(tag);
    let c = FakeCatalog::new();
    for id in 1..=2 {
        let paths: Vec<String> = (0..3).map(|i| format!("/c{id}/p{i}")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        c.set_pages(id, &refs);
        for p in &paths {
            c.set_bytes(p, colour_page(300, 450));
        }
    }
    let out = download(&c, &lib, PageQuality::Full, false, &[request(1.0, 1), request(2.0, 2)]).await;
    let settings = ConversionSettings {
        crop: CropMode::Full,
        ..ConversionSettings::default()
    };
    let mut db = LibraryDb::default();
    for ch in &out.chapters {
        db.record_chapter(TITLE, &settings, ch.clone(), 1);
    }
    (lib, db.series.remove(0))
}

fn job(lib: &Lib, series: &DownloadedSeries, output: OutputFormat, device: &str) -> ExportJob {
    let mut j = ExportJob::new(
        series.clone(),
        series.chapters.iter().map(|c| c.key.clone()).collect(),
        device_profiles::known(device).unwrap(),
        ExportTarget::Folder { path: lib.out() },
        lib.work(),
    );
    j.grouping = Grouping::Single;
    j.cover = false;
    j.series.settings.set_output(output);
    j
}

async fn export(lib: &Lib, j: &ExportJob) -> pipeline::ExportOutcome {
    export_series(&lib.files, j, None, &CancelFlag::new()).await.unwrap()
}

/// The pictures of an EPUB or CBZ in page order, as their file names and bytes.
fn pictures(path: &str) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .flatten()
        .filter(|n| n.ends_with(".jpg") || n.ends_with(".png"))
        .map(String::from)
        .collect();
    let number = |n: &String| -> usize {
        n.rsplit('/').next().unwrap().split('.').next().unwrap().parse().unwrap()
    };
    names.sort_by_key(number);
    names
        .into_iter()
        .map(|n| {
            let mut bytes = Vec::new();
            zip.by_name(&n).unwrap().read_to_end(&mut bytes).unwrap();
            (n, bytes)
        })
        .collect()
}

fn rgb_at(picture: &[u8], x: u32, y: u32) -> [u8; 3] {
    image::load_from_memory(picture).unwrap().to_rgb8().get_pixel(x, y).0
}

/// The libra's 1264 x 1680 screen shows the 300 x 450 page 1120 wide with
/// white bars at the sides: red at the top, a green stripe, blue at the bottom.
fn check_colour_page(picture: &[u8], name: &str) {
    let (w, h) = image_dimensions(picture).unwrap();
    assert_eq!((w, h), (1264, 1680), "{name}");
    let [r, g, b] = rgb_at(picture, 632, 300);
    assert!(r > 190 && g < 70 && b < 70, "{name}: red at the top, got {:?}", [r, g, b]);
    let [r, g, b] = rgb_at(picture, 632, 1400);
    assert!(b > 190 && r < 70 && g < 70, "{name}: blue at the bottom, got {:?}", [r, g, b]);
    let [r, g, b] = rgb_at(picture, 632, 840);
    assert!(g > 150 && r < 90 && b < 110, "{name}: green in the middle, got {:?}", [r, g, b]);
    let [r, g, b] = rgb_at(picture, 12, 800);
    assert!(r > 245 && g > 245 && b > 245, "{name}: the bars are white, not tinted: {:?}", [r, g, b]);
}

fn check_gray_page(picture: &[u8], name: &str) {
    for (x, y) in [(632, 300), (632, 1400), (632, 840), (12, 800)] {
        let [r, g, b] = rgb_at(picture, x, y);
        assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "{name}: gray at {x},{y}, got {:?}", [r, g, b]);
    }
}

#[tokio::test]
async fn a_colour_epub_and_cbz_are_colour_jpegs_with_the_same_pages_as_the_gray_ones() {
    let (lib, series) = colour_series("both").await;
    for (output, ext) in [(OutputFormat::Epub, "epub"), (OutputFormat::Cbz, "cbz")] {
        let mut colour = job(&lib, &series, output, "kobo-libra-colour");
        assert!(colour.series.settings.keeps_color(&colour.device), "colour is the default on a colour reader");
        let coloured = export(&lib, &colour).await;
        colour.series.settings.black_and_white = true;
        let gray = export(&lib, &colour).await;
        assert_eq!(coloured.pages, 6);
        assert_eq!(coloured.pages, gray.pages, "{ext}: the same pages either way");
        assert!(coloured.files[0].location.ends_with(ext));

        let c = pictures(&coloured.files[0].location);
        let g = pictures(&gray.files[0].location);
        assert_eq!(c.len(), 6, "{ext}");
        assert_eq!(c.len(), g.len(), "{ext}");
        for (name, bytes) in &c {
            assert!(name.ends_with(".jpg") && bytes.starts_with(&[0xFF, 0xD8]), "{ext}: {name}");
            check_colour_page(bytes, &format!("{ext} {name}"));
        }
        for (name, bytes) in &g {
            check_gray_page(bytes, &format!("{ext} gray {name}"));
        }
        // Not tone-tuned: the gray page has had its levels stretched and its
        // contrast raised, the colour one has not, so the same spot differs.
        assert_ne!(rgb_at(&c[0].1, 632, 300), rgb_at(&g[0].1, 632, 300), "{ext}");
    }
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn the_cover_of_a_colour_book_is_in_colour_too() {
    let (lib, series) = colour_series("cover").await;
    let mut j = job(&lib, &series, OutputFormat::Epub, "kobo-libra-colour");
    j.cover = true;
    let out = export(&lib, &j).await;
    let pics = pictures(&out.files[0].location);
    assert_eq!(pics.len(), 7, "the cover and the six pages");
    check_colour_page(&pics[0].1, "cover");
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn an_xtc_is_the_same_whether_or_not_colour_is_asked_for() {
    let (lib, series) = colour_series("xtc").await;
    for format in [OutputFormat::Xtc, OutputFormat::Xtch] {
        let mut j = job(&lib, &series, format, "kobo-libra-colour");
        j.series.settings.black_and_white = true;
        let gray = export(&lib, &j).await;
        j.series.settings.black_and_white = false;
        assert!(!j.series.settings.keeps_color(&j.device), "{format:?} cannot carry colour");
        let asked = export(&lib, &j).await;
        // The same conversion key, so the second export is the first one's cache:
        // the very same packed pages.
        assert_eq!(asked.cached_chapters, 2, "{format:?}");
        let parse = |path: &str| {
            let bytes = fs::read(path).unwrap();
            let file = XtcFile::parse(&bytes).unwrap();
            (0..file.page_count()).map(|i| file.decode_page(i).unwrap().image).collect::<Vec<_>>()
        };
        let (a, b) = (parse(&gray.files[0].location), parse(&asked.files[0].location));
        assert_eq!(a.len(), 6);
        assert_eq!(a, b, "{format:?}");
        assert_eq!(
            fs::metadata(&gray.files[0].location).unwrap().len(),
            fs::metadata(&asked.files[0].location).unwrap().len()
        );
    }
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn gray_and_colour_conversions_never_share_a_cache() {
    let (lib, series) = colour_series("cache").await;
    let mut j = job(&lib, &series, OutputFormat::Epub, "kobo-libra-colour");
    let first = export(&lib, &j).await;
    assert_eq!(first.cached_chapters, 0);
    assert_eq!(export(&lib, &j).await.cached_chapters, 2, "the same again is reused");
    // Black and white is a different conversion: nothing is taken from the colour cache.
    j.series.settings.black_and_white = true;
    let gray = export(&lib, &j).await;
    assert_eq!(gray.cached_chapters, 0);
    check_gray_page(&pictures(&gray.files[0].location)[0].1, "gray after colour");
    assert_eq!(export(&lib, &j).await.cached_chapters, 2);
    // And back: the gray cache does not answer for colour.
    j.series.settings.black_and_white = false;
    let again = export(&lib, &j).await;
    assert_eq!(again.cached_chapters, 0);
    check_colour_page(&pictures(&again.files[0].location)[0].1, "colour after gray");
    // The tone settings do not touch colour pages, so changing one keeps the cache.
    j.series.settings.contrast = 80;
    assert_eq!(export(&lib, &j).await.cached_chapters, 2);
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn a_gray_reader_never_gets_colour_pages() {
    let (lib, series) = colour_series("grayreader").await;
    let j = job(&lib, &series, OutputFormat::Epub, "kobo-libra-2");
    assert!(!j.series.settings.keeps_color(&j.device));
    let out = export(&lib, &j).await;
    for (name, bytes) in pictures(&out.files[0].location) {
        let [r, g, b] = rgb_at(&bytes, 632, 300);
        assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "{name}");
    }
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn keeping_colour_in_a_download_keeps_it_through_the_shrinking_and_off_it_does_not() {
    let c = FakeCatalog::new();
    c.set_pages(1, &["/big"]);
    c.set_bytes("/big", colour_page(1200, 3200));
    for (keep, quality) in [(true, PageQuality::Save), (false, PageQuality::Save), (true, PageQuality::Ultra)] {
        let lib = Lib::new(&format!("dl-{keep}-{quality:?}"));
        download(&c, &lib, quality, keep, &[request(1.0, 1)]).await;
        let saved = fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap();
        assert_eq!(&saved[..3], &[0xFF, 0xD8, 0xFF], "shrunk to a JPEG");
        let (_, [cb, cr]) = decode_to_ycc(&saved).unwrap();
        let coloured = cb.pixels.iter().chain(&cr.pixels).any(|&v| v.abs_diff(128) > 30);
        assert_eq!(coloured, keep, "keep_color {keep} at {quality:?}");
        let _ = fs::remove_dir_all(lib.root);
    }
    // Full keeps the original either way.
    let lib = Lib::new("dl-full");
    download(&c, &lib, PageQuality::Full, true, &[request(1.0, 1)]).await;
    assert_eq!(fs::read(lib.files.page_path(&lib.id, "c1", 0)).unwrap(), colour_page(1200, 3200));
    let _ = fs::remove_dir_all(lib.root);
}

#[tokio::test]
async fn pages_saved_with_colour_export_in_colour_end_to_end() {
    // Download shrunk but colour-keeping, then export for a colour reader.
    let lib = Lib::new("e2e");
    let c = FakeCatalog::new();
    c.set_pages(1, &["/a", "/b"]);
    c.set_bytes("/a", colour_page(1200, 1800));
    c.set_bytes("/b", colour_page(1200, 1800));
    let out = download(&c, &lib, PageQuality::Save, true, &[request(1.0, 1)]).await;
    let settings = ConversionSettings {
        crop: CropMode::Full,
        ..ConversionSettings::default()
    };
    let mut db = LibraryDb::default();
    for ch in &out.chapters {
        db.record_chapter(TITLE, &settings, ch.clone(), 1);
    }
    let series = db.series.remove(0);
    let j = job(&lib, &series, OutputFormat::Cbz, "kobo-libra-colour");
    let made = export(&lib, &j).await;
    let pics = pictures(&made.files[0].location);
    assert_eq!(pics.len(), 2);
    for (name, bytes) in &pics {
        check_colour_page(bytes, name);
    }
    let _ = fs::remove_dir_all(lib.root);
}
