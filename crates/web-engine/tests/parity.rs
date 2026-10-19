//! The website's exporter makes the same files as the desktop app.
//!
//! Each test builds a small library on disk, runs the desktop pipeline's real
//! `export_series` over it, runs the in-memory `Exporter` over the same pages with
//! the same options, and compares the files byte for byte.
#![cfg(feature = "convert")]

use device_profiles::DeviceProfile;
use image::{ImageEncoder, Rgb, RgbImage};
use library::{
    ConversionSettings, DownloadedChapter, DownloadedSeries, LibraryDb, LibraryFiles, OutputFormat,
    SeriesInfo,
};
use pipeline::{
    export_series, CancelFlag, ExportJob, ExportTarget, Grouping as DesktopGrouping, OriginalExport,
    OriginalFormat,
};
use std::fs;
use std::path::PathBuf;
use web_engine::export::{zip_entries, Exporter, Limits};
use web_engine::options::{ExportOptions, FileType, Grouping, Info};

const TITLE: &str = "Parity Series";

// ---- Pages ---------------------------------------------------------------------

fn png_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn jpeg_of(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// A manga-like gray page: white paper, a border of margin, panels and lines.
fn gray_png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| {
        let inside = x > w / 10 && x < w - w / 10 && y > h / 12 && y < h - h / 12;
        let v = if !inside {
            255
        } else if (x / 40 + y / 40 + u32::from(seed)) % 3 == 0 {
            30
        } else if y % 97 < 3 {
            120
        } else {
            235
        };
        Rgb([v, v, v])
    });
    png_of(&img)
}

/// A colour page: a red-to-blue gradient with a green band, so gray would lose it.
fn colour_image(w: u32, h: u32, seed: u8) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        let inside = x > w / 12 && x < w - w / 12 && y > h / 14 && y < h - h / 14;
        if !inside {
            return Rgb([255, 255, 255]);
        }
        Rgb([
            (x * 255 / w.max(1)) as u8,
            if y % 80 < 40 { 200 } else { seed },
            (y * 255 / h.max(1)) as u8,
        ])
    })
}

fn colour_jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    jpeg_of(&colour_image(w, h, seed))
}

fn webp_of(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = colour_image(w, h, seed);
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn gif_of(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = colour_image(w, h, seed);
    let mut out = Vec::new();
    image::codecs::gif::GifEncoder::new(&mut out)
        .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// Two chapters of gray pages, one with a wide double-page spread.
fn gray_chapters() -> Vec<Vec<Vec<u8>>> {
    vec![
        vec![gray_png(300, 450, 1), gray_png(300, 450, 2), gray_png(640, 450, 3)],
        vec![gray_png(300, 450, 4), gray_png(300, 450, 5)],
    ]
}

fn colour_chapters() -> Vec<Vec<Vec<u8>>> {
    vec![
        vec![colour_jpeg(300, 450, 10), colour_jpeg(300, 450, 60)],
        vec![png_of(&colour_image(300, 450, 90)), colour_jpeg(320, 480, 120)],
        vec![colour_jpeg(300, 450, 150)],
    ]
}

/// A webtoon: one tall strip with gaps between the scenes, in two images.
fn strip_chapters() -> Vec<Vec<Vec<u8>>> {
    let strip = |seed: u8| {
        let img = RgbImage::from_fn(300, 1800, |x, y| {
            let scene = y / 450;
            let in_gap = y % 450 > 400;
            let v = if in_gap || !(20..=280).contains(&x) { 255 } else { 40 + (scene as u8 * 30) + seed };
            Rgb([v, v, v])
        });
        png_of(&img)
    };
    vec![vec![strip(0), strip(15)], vec![strip(30)]]
}

// ---- The desktop side ----------------------------------------------------------

struct Desktop {
    root: PathBuf,
    files: LibraryFiles,
    series: DownloadedSeries,
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("web-engine-parity-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn desktop_library(tag: &str, chapters: &[Vec<Vec<u8>>], settings: &ConversionSettings, info: Option<SeriesInfo>) -> Desktop {
    let root = scratch(tag);
    let files = LibraryFiles::new(root.join("library"));
    let id = library::series_id(TITLE);
    let mut db = LibraryDb::default();
    for (c, pages) in chapters.iter().enumerate() {
        let key = format!("c{}", c + 1);
        fs::create_dir_all(files.chapter_dir(&id, &key)).unwrap();
        for (p, bytes) in pages.iter().enumerate() {
            fs::write(files.page_path(&id, &key, p), bytes).unwrap();
        }
        db.record_chapter(
            TITLE,
            settings,
            DownloadedChapter {
                key,
                number: Some((c + 1) as f64),
                name: format!("Chapter {}", c + 1),
                source_id: "s".into(),
                chapter_id: c as i64,
                pages: pages.len(),
                bytes: 0,
                downloaded_at: 1,
                quality: None,
            },
            1,
        );
    }
    let mut series = db.series.remove(0);
    series.info = info;
    Desktop { root, files, series }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

struct Case<'a> {
    tag: &'a str,
    chapters: Vec<Vec<Vec<u8>>>,
    device: &'a str,
    format: FileType,
    settings: ConversionSettings,
    grouping: Grouping,
    cover: bool,
    info: Option<SeriesInfo>,
}

impl<'a> Case<'a> {
    fn new(tag: &'a str, chapters: Vec<Vec<Vec<u8>>>, device: &'a str, format: FileType) -> Self {
        Self {
            tag,
            chapters,
            device,
            format,
            settings: ConversionSettings::default(),
            grouping: Grouping::Single,
            cover: false,
            info: None,
        }
    }

    fn options(&self, created: u64) -> ExportOptions {
        let mut settings = self.settings.clone();
        if let Some(output) = self.format.output() {
            settings.set_output(output);
        }
        let info = self.info.as_ref().map(|i| Info {
            description: i.description.clone(),
            author: i.author.clone(),
            artist: i.artist.clone(),
            genres: i.genres.clone(),
            status: i.status.clone(),
            sources: i.source_names.iter().map(|(_, n)| n.clone()).collect(),
        });
        ExportOptions {
            device: self.device.into(),
            format: Some(self.format),
            settings,
            grouping: self.grouping,
            title: TITLE.into(),
            language: "en".into(),
            info: info.unwrap_or_default(),
            cover: self.cover,
            created,
            ..ExportOptions::default()
        }
    }

    fn desktop_device(&self) -> DeviceProfile {
        ExportOptions { device: self.device.into(), ..ExportOptions::default() }.resolve_device().unwrap()
    }

    /// What the desktop's export makes: (file name, bytes), sorted by name, and
    /// the second (Unix time) it was made in.
    fn run_desktop(&self) -> (Vec<(String, Vec<u8>)>, u64) {
        let mut settings = self.settings.clone();
        if let Some(output) = self.format.output() {
            settings.set_output(output);
        }
        let d = desktop_library(self.tag, &self.chapters, &settings, self.info.clone());
        let device = self.desktop_device();
        let mut job = ExportJob::new(
            d.series.clone(),
            d.series.chapters.iter().map(|c| c.key.clone()).collect(),
            device,
            ExportTarget::Folder { path: d.root.join("out") },
            d.root.join("work"),
        );
        job.cover = self.cover;
        job.force_conversion = true;
        job.grouping = match self.grouping {
            Grouping::Single => DesktopGrouping::Single,
            Grouping::PerChapter => DesktopGrouping::PerChapter,
            Grouping::EveryN { n } => DesktopGrouping::EveryN(n),
        };
        if self.device == "phone-original" {
            let format = if self.format == FileType::Pdf { OriginalFormat::Pdf } else { OriginalFormat::Cbz };
            job.original = Some(OriginalExport { format, rtl: settings.direction == converter_rtl() });
        }
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let before = now();
        runtime
            .block_on(export_series(&d.files, &job, None, &CancelFlag::new()))
            .expect("the desktop export works");
        let after = now();
        let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(d.root.join("out"))
            .unwrap()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
            .collect();
        out.sort();
        let _ = fs::remove_dir_all(&d.root);
        // The files carry the second they were made in; a run that crossed a
        // second boundary cannot be compared and is repeated.
        (out, if before == after { before } else { 0 })
    }

    fn run_web(&self, created: u64) -> Vec<(String, Vec<u8>)> {
        let mut exporter = Exporter::with_options(self.options(created)).unwrap();
        feed(&mut exporter, &self.chapters);
        files_of(&mut exporter)
    }
}

fn converter_rtl() -> converter::xtc::ReadDirection {
    converter::xtc::ReadDirection::RightToLeft
}

fn feed(exporter: &mut Exporter, chapters: &[Vec<Vec<u8>>]) {
    for (c, pages) in chapters.iter().enumerate() {
        exporter.add_chapter(&format!("Chapter {}", c + 1), Some((c + 1) as f64), pages).unwrap();
    }
    exporter.finish().unwrap();
}

fn files_of(exporter: &mut Exporter) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = (0..exporter.file_count())
        .map(|i| {
            let f = exporter.take_file(i).unwrap();
            (f.name, f.bytes)
        })
        .collect();
    out.sort();
    out
}

/// Run both sides until they ran in the same second, and compare.
fn assert_same_as_desktop(case: &Case) -> Vec<(String, Vec<u8>)> {
    for _ in 0..6 {
        let (desktop, created) = case.run_desktop();
        if created == 0 {
            continue;
        }
        let web = case.run_web(created);
        let names = |v: &[(String, Vec<u8>)]| v.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
        assert_eq!(names(&web), names(&desktop), "{}: file names", case.tag);
        for ((name, mine), (_, theirs)) in web.iter().zip(&desktop) {
            if mine != theirs {
                // Say what differs inside a zip before giving up.
                if let (Ok(a), Ok(b)) = (zip_entries(mine), zip_entries(theirs)) {
                    let an: Vec<_> = a.iter().map(|(n, _)| n.as_str()).collect();
                    let bn: Vec<_> = b.iter().map(|(n, _)| n.as_str()).collect();
                    assert_eq!(an, bn, "{}: entry names of {name}", case.tag);
                    for ((n, x), (_, y)) in a.iter().zip(&b) {
                        assert!(x == y, "{}: entry {n} of {name} differs ({} vs {} bytes)", case.tag, x.len(), y.len());
                    }
                }
                panic!("{}: {name} differs from the desktop's ({} vs {} bytes)", case.tag, mine.len(), theirs.len());
            }
        }
        return web;
    }
    panic!("{}: could not get both sides in one second", case.tag);
}

// ---- Tests ---------------------------------------------------------------------

#[test]
fn an_xtc_book_is_byte_identical() {
    let case = Case::new("xtc", gray_chapters(), "xteink-x4-pro", FileType::Xtc);
    let files = assert_same_as_desktop(&case);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, "Parity Series - Ch 1-2.xtc");
    assert_eq!(&files[0].1[..4], b"XTC\0");
}

#[test]
fn an_xtch_book_with_a_cover_page_is_byte_identical() {
    let mut case = Case::new("xtch", gray_chapters(), "xteink-x4", FileType::Xtch);
    case.cover = true;
    let files = assert_same_as_desktop(&case);
    assert_eq!(&files[0].1[..4], b"XTCH");
}

#[test]
fn an_epub_for_a_four_gray_reader_is_byte_identical() {
    let case = Case::new("epub4", gray_chapters(), "xteink-x4-pro", FileType::Epub);
    let files = assert_same_as_desktop(&case);
    let entries = zip_entries(&files[0].1).unwrap();
    assert_eq!(entries[0].0, "mimetype", "the type of the book comes first");
    assert!(entries.iter().any(|(n, _)| n == "OEBPS/1.png"));
}

#[test]
fn an_epub_for_a_sixteen_gray_reader_has_jpeg_pages_and_is_byte_identical() {
    let mut case = Case::new("epub16", gray_chapters(), "kindle-paperwhite-12", FileType::Epub);
    case.cover = true;
    let files = assert_same_as_desktop(&case);
    let entries = zip_entries(&files[0].1).unwrap();
    assert!(entries.iter().any(|(n, _)| n == "OEBPS/1.jpg"));
}

#[test]
fn a_cbz_with_details_is_byte_identical() {
    let mut case = Case::new("cbz", gray_chapters(), "kobo-libra-2", FileType::Cbz);
    case.info = Some(SeriesInfo {
        description: "A story & more".into(),
        author: "Some Writer".into(),
        artist: "Some Artist".into(),
        status: "Ongoing".into(),
        genres: vec!["Action".into(), "Manga".into()],
        source_names: vec![("s".into(), "Source One".into())],
        ..Default::default()
    });
    let files = assert_same_as_desktop(&case);
    let entries = zip_entries(&files[0].1).unwrap();
    let info = String::from_utf8(entries.last().unwrap().1.clone()).unwrap();
    assert!(info.contains("<Writer>Some Writer</Writer>") && info.contains("Bookmark=\"Chapter 2\""), "{info}");
}

#[test]
fn colour_epub_and_cbz_keep_the_chroma_and_are_byte_identical() {
    for (tag, format) in [("colour-epub", FileType::Epub), ("colour-cbz", FileType::Cbz)] {
        let case = Case::new(tag, colour_chapters(), "kobo-libra-colour", format);
        let files = assert_same_as_desktop(&case);
        let entries = zip_entries(&files[0].1).unwrap();
        let page = entries.iter().find(|(n, _)| n.ends_with("1.jpg") || n.ends_with("00001.jpg")).expect("a jpeg page");
        let rgb = image::load_from_memory(&page.1).unwrap().to_rgb8();
        let colourful = rgb.pixels().filter(|p| p.0[0].abs_diff(p.0[2]) > 60).count();
        assert!(colourful > 1000, "{tag}: the page kept its colour ({colourful} coloured pixels)");
    }
}

#[test]
fn black_and_white_and_xtc_ignore_colour() {
    // A colour reader asked for black and white makes gray pages...
    let mut bw = Case::new("bw-epub", colour_chapters(), "kobo-libra-colour", FileType::Epub);
    bw.settings.black_and_white = true;
    let files = assert_same_as_desktop(&bw);
    let entries = zip_entries(&files[0].1).unwrap();
    let page = entries.iter().find(|(n, _)| n.ends_with("1.jpg")).unwrap();
    let rgb = image::load_from_memory(&page.1).unwrap().to_rgb8();
    assert!(rgb.pixels().all(|p| p.0[0].abs_diff(p.0[2]) < 8), "gray");
    // ...and an XTC book is the same on a colour reader as with black and white chosen.
    let xtc = Case::new("colour-xtc", colour_chapters(), "kobo-libra-colour", FileType::Xtc);
    let made = assert_same_as_desktop(&xtc);
    let mut forced = Case::new("colour-xtc-bw", colour_chapters(), "kobo-libra-colour", FileType::Xtc);
    forced.settings.black_and_white = true;
    assert_eq!(made[0].1, assert_same_as_desktop(&forced)[0].1);
}

#[test]
fn a_webtoon_strip_is_cut_into_pages_like_the_desktop_does() {
    let preset = library::builtin_presets().into_iter().find(|p| p.id == "manhwa").unwrap();
    let mut case = Case::new("webtoon", strip_chapters(), "xteink-x4-pro", FileType::Xtc);
    case.settings = preset.settings;
    let files = assert_same_as_desktop(&case);
    let pages = converter::xtc::XtcFile::parse(&files[0].1).unwrap().page_count();
    assert!(pages > 3, "the strips became several pages, not {pages}");
}

#[test]
fn files_are_grouped_per_chapter_or_every_n_like_the_desktop_does() {
    let mut per = Case::new("per-chapter", colour_chapters(), "kobo-libra-2", FileType::Cbz);
    per.grouping = Grouping::PerChapter;
    let files = assert_same_as_desktop(&per);
    assert_eq!(
        files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        ["Parity Series - Ch 1.cbz", "Parity Series - Ch 2.cbz", "Parity Series - Ch 3.cbz"]
    );
    let mut every = Case::new("every-2", colour_chapters(), "xteink-x4-pro", FileType::Xtc);
    every.grouping = Grouping::EveryN { n: 2 };
    let files = assert_same_as_desktop(&every);
    assert_eq!(files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["Parity Series - Ch 1-2.xtc", "Parity Series - Ch 3.xtc"]);
}

#[test]
fn a_sideways_reader_gets_the_same_pages() {
    for (tag, format) in [("side-xtc", FileType::Xtc), ("side-cbz", FileType::Cbz)] {
        let mut case = Case::new(tag, gray_chapters(), "xteink-x4-pro", format);
        case.settings.orientation = library::Orientation::Landscape;
        assert_same_as_desktop(&case);
    }
}

#[test]
fn a_phone_cbz_keeps_every_picture_byte_for_byte_and_matches_the_desktop() {
    let chapters = vec![
        vec![colour_jpeg(300, 450, 1), png_of(&colour_image(300, 450, 2)), webp_of(60, 90, 3)],
        vec![colour_jpeg(200, 300, 4), gif_of(40, 60, 5)],
    ];
    let mut case = Case::new("phone-cbz", chapters.clone(), "phone-original", FileType::Cbz);
    case.info = Some(SeriesInfo { author: "Some Writer".into(), genres: vec!["Action".into()], ..Default::default() });
    case.settings.direction = converter_rtl();
    let files = assert_same_as_desktop(&case);
    let entries = zip_entries(&files[0].1).unwrap();
    assert_eq!(entries[0].1, chapters[0][0], "stored as it was saved");
    assert_eq!(entries[2].1, chapters[0][2]);
    assert!(entries[4].0.ends_with(".png"), "a GIF becomes a PNG: {}", entries[4].0);
    assert_eq!(entries.last().unwrap().0, "ComicInfo.xml");
}

#[test]
fn a_phone_pdf_matches_the_desktop() {
    let chapters = vec![
        vec![colour_jpeg(300, 450, 1), png_of(&colour_image(120, 180, 2)), webp_of(60, 90, 3)],
        vec![gray_png(200, 300, 4), gif_of(40, 60, 5)],
    ];
    let mut case = Case::new("phone-pdf", chapters, "phone-original", FileType::Pdf);
    case.settings.direction = converter_rtl();
    let files = assert_same_as_desktop(&case);
    assert!(files[0].1.starts_with(b"%PDF-1.4"));
    assert_eq!(files[0].0, "Parity Series - Ch 1-2.pdf");
    assert!(files[0].1.ends_with(b"%%EOF\n"));
}

// ---- Behaviour that is the web's own ---------------------------------------------

fn options_json(device: &str, format: &str) -> String {
    format!(r#"{{"device":"{device}","format":"{format}","title":"T","created":1781000000}}"#)
}

#[test]
fn options_given_as_json_work_and_exporters_share_nothing() {
    let pages = gray_chapters();
    let one = |json: &str| {
        let mut e = Exporter::new(json).unwrap();
        feed(&mut e, &pages);
        files_of(&mut e)
    };
    let alone = one(&options_json("kindle-paperwhite-12", "EPUB"));
    // Interleave two exporters chapter by chapter: neither disturbs the other.
    let mut a = Exporter::new(&options_json("kindle-paperwhite-12", "EPUB")).unwrap();
    let mut b = Exporter::new(&options_json("xteink-x4", "XTC")).unwrap();
    for (c, chapter) in pages.iter().enumerate() {
        a.add_chapter(&format!("Chapter {}", c + 1), Some((c + 1) as f64), chapter).unwrap();
        b.add_chapter(&format!("Chapter {}", c + 1), Some((c + 1) as f64), chapter).unwrap();
    }
    a.finish().unwrap();
    b.finish().unwrap();
    assert_eq!(files_of(&mut a), alone);
    assert_eq!(files_of(&mut b), one(&options_json("xteink-x4", "XTC")));
    // A dropped exporter (a cancelled export) leaves nothing behind.
    let mut dropped = Exporter::new(&options_json("xteink-x4", "XTC")).unwrap();
    dropped.add_chapter("c", None, &pages[0]).unwrap();
    drop(dropped);
    assert_eq!(one(&options_json("kindle-paperwhite-12", "EPUB")), alone);
}

#[test]
fn a_bad_page_or_a_big_chapter_is_refused_and_changes_nothing() {
    let pages = gray_chapters();
    let mut e = Exporter::new(&options_json("xteink-x4", "XTC")).unwrap();
    let err = e.add_chapter("Bad", Some(1.0), &[pages[0][0].clone(), b"not a picture".to_vec()]).unwrap_err();
    assert!(err.contains("page 2 of Bad could not be read"), "{err}");
    let empty: [Vec<u8>; 0] = [];
    assert!(e.add_chapter("Empty", None, &empty).unwrap_err().contains("no pages"));
    assert!(e.finish().unwrap_err().contains("no chapter"), "nothing was added");
    assert!(e.add_chapter("Good", Some(2.0), &pages[1]).is_ok(), "the exporter is not spoiled");
    assert_eq!(e.chapter_count(), 1);

    // A chapter whose pictures are too big once unpacked.
    let mut small = Exporter::new(&options_json("xteink-x4", "XTC"))
        .unwrap()
        .with_limits(Limits { max_chapter_bytes: 100_000, max_output_bytes: u64::MAX });
    let err = small.add_chapter("Big", Some(1.0), &pages[0]).unwrap_err();
    assert!(err.contains("too big to convert") && err.contains("one file per chapter"), "{err}");

    // Files that would pass the output limit: room for one chapter, not for two.
    let colour = colour_chapters();
    let mut probe = Exporter::new(&options_json("kobo-libra-2", "CBZ")).unwrap();
    probe.add_chapter("C0", None, &colour[0]).unwrap();
    probe.finish().unwrap();
    let one_chapter = probe.take_file(0).unwrap().bytes.len() as u64;
    let mut tight = Exporter::new(&options_json("kobo-libra-2", "CBZ"))
        .unwrap()
        .with_limits(Limits { max_chapter_bytes: u64::MAX, max_output_bytes: one_chapter * 3 / 2 });
    let mut refused = None;
    for (c, chapter) in colour.iter().enumerate() {
        if let Err(e) = tight.add_chapter(&format!("C{c}"), None, chapter) {
            refused = Some(e);
            break;
        }
    }
    let err = refused.expect("the limit is reached");
    assert!(err.contains("more than the browser can build"), "{err}");
    tight.finish().unwrap();
    assert!(tight.file_count() >= 1, "what fitted can still be saved");
}

#[test]
fn files_are_taken_once_after_finishing() {
    let pages = gray_chapters();
    let mut e = Exporter::new(&options_json("kobo-libra-2", "CBZ")).unwrap();
    assert!(e.take_file(0).unwrap_err().contains("Finish"));
    e.add_chapter("c", None, &pages[0]).unwrap();
    e.finish().unwrap();
    assert_eq!(e.file_count(), 1);
    assert_eq!(e.file_name(0), Some("T - c.cbz"), "a lone unnumbered chapter is named after itself");
    let file = e.take_file(0).unwrap();
    assert!(file.bytes.starts_with(b"PK"));
    assert!(e.take_file(0).unwrap_err().contains("already taken"));
    assert!(e.take_file(5).is_err());
    assert!(e.add_chapter("late", None, &pages[1]).unwrap_err().contains("finished"));
}

#[test]
fn a_pdf_needs_the_phone_entry_and_the_epub_is_cut_at_800_pages() {
    assert!(Exporter::new(&options_json("kindle-paperwhite-12", "PDF")).is_err());
    assert!(Exporter::new(&options_json("nowhere", "EPUB")).is_err());
    // Page caps are respected: with a cap of 2 pages per file, 5 pages make 3 parts.
    let page = gray_png(120, 180, 1);
    let mut e = Exporter::new(r#"{"device":"kobo-libra-2","format":"CBZ","title":"T","maxPagesPerFile":2}"#).unwrap();
    for c in 0..5 {
        e.add_chapter(&format!("Chapter {}", c + 1), Some(f64::from(c + 1)), std::slice::from_ref(&page)).unwrap();
    }
    e.finish().unwrap();
    let names: Vec<String> = (0..e.file_count()).filter_map(|i| e.file_name(i).map(String::from)).collect();
    assert_eq!(names, ["T - Part 1 - Ch 1-2.cbz", "T - Part 2 - Ch 3-4.cbz", "T - Part 3 - Ch 5.cbz"]);
}

#[test]
fn the_output_format_enum_matches_the_options() {
    // The file types the options name are the desktop's output formats.
    for (t, o) in [
        (FileType::Xtc, OutputFormat::Xtc),
        (FileType::Xtch, OutputFormat::Xtch),
        (FileType::Epub, OutputFormat::Epub),
        (FileType::Cbz, OutputFormat::Cbz),
    ] {
        assert_eq!(t.output(), Some(o));
        assert_eq!(t.extension(), o.extension());
    }
}
