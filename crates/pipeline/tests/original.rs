//! Original-quality downloads and exports: pages kept exactly as downloaded,
//! and books (CBZ, PDF) made from them with no e-ink processing at all.

use catalog::fake::FakeCatalog;
use image::{ImageEncoder, Rgb, RgbImage};
use library::{
    ConversionSettings, DownloadLimits, DownloadedSeries, LibraryDb, LibraryFiles, PageQuality,
};
use pipeline::{
    download_chapters, download_chapters_as, export_series, CancelFlag, ChapterRef, ChapterRequest,
    ExportJob, ExportTarget, Grouping, JobEvent, OriginalExport, OriginalFormat, PipelineError,
    RetryPolicy,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TITLE: &str = "Original Series";

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mangapoint-orig-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A color picture: a red-to-blue gradient with a green band, so gray would lose it.
fn color_image(w: u32, h: u32, seed: u8) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| {
        Rgb([
            (x * 255 / w.max(1)) as u8,
            if y % 40 < 20 { 200 } else { seed },
            (y * 255 / h.max(1)) as u8,
        ])
    })
}

fn jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = color_image(w, h, seed);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = color_image(w, h, seed);
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn webp(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = color_image(w, h, seed);
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn gif(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = color_image(w, h, seed);
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        enc.encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    out
}

fn quick() -> RetryPolicy {
    RetryPolicy {
        attempts: 1,
        base_delay: Duration::from_millis(1),
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

struct Lib {
    root: PathBuf,
    files: LibraryFiles,
    id: String,
}

fn lib(tag: &str) -> Lib {
    let root = temp(tag);
    Lib {
        files: LibraryFiles::new(root.join("library")),
        id: library::series_id(TITLE),
        root,
    }
}

impl Lib {
    fn out(&self) -> PathBuf {
        self.root.join("out")
    }
    fn work(&self) -> PathBuf {
        self.root.join("work")
    }
}

fn limits() -> DownloadLimits {
    DownloadLimits {
        chapters: 1,
        pages: 2,
        spread_sources: false,
        race_sources: false,
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn add_chapter(c: &FakeCatalog, id: i64, pages: Vec<Vec<u8>>) {
    let paths: Vec<String> = (0..pages.len()).map(|i| format!("/c{id}/p{i}")).collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    c.set_pages(id, &refs);
    for (p, bytes) in paths.iter().zip(pages) {
        c.set_bytes(p, bytes);
    }
}

// ---- Downloads ---------------------------------------------------------------

#[tokio::test]
async fn original_quality_keeps_the_downloaded_bytes_and_the_color_while_save_shrinks_them() {
    let c = FakeCatalog::new();
    // Taller than the save limit, so Save has to shrink it.
    let page = jpeg(1400, 2800, 30);
    add_chapter(&c, 1, vec![page.clone()]);

    let full = lib("dl-full");
    let out = download_chapters(
        &c,
        &full.files,
        &full.id,
        &[request(1.0, 1)],
        PageQuality::Full,
        limits(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    let kept = fs::read(full.files.page_path(&full.id, "c1", 0)).unwrap();
    assert_eq!(kept, page, "stored exactly as downloaded");
    assert_eq!(out.chapters[0].quality, Some(PageQuality::Full));
    let img = image::load_from_memory(&kept).unwrap().to_rgb8();
    let (r, g, b) = img.pixels().fold((0u64, 0u64, 0u64), |a, p| {
        (a.0 + u64::from(p[0]), a.1 + u64::from(p[1]), a.2 + u64::from(p[2]))
    });
    assert!(r.abs_diff(g) > 1000 && g.abs_diff(b) > 1000, "still color");

    let save = lib("dl-save");
    let out = download_chapters(
        &c,
        &save.files,
        &save.id,
        &[request(1.0, 1)],
        PageQuality::Save,
        limits(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    let small = fs::read(save.files.page_path(&save.id, "c1", 0)).unwrap();
    assert_ne!(small, page);
    assert!(small.len() < page.len());
    let (w, h) = converter::decode::image_dimensions(&small).unwrap();
    assert!(h < 2800 && w < 1400, "{w}x{h}");
    assert_eq!(out.chapters[0].quality, Some(PageQuality::Save));
    let _ = fs::remove_dir_all(full.root);
    let _ = fs::remove_dir_all(save.root);
}

/// A chapter saved small, then downloaded again at the original quality.
async fn saved_small(tag: &str, c: &FakeCatalog) -> Lib {
    let l = lib(tag);
    download_chapters(
        c,
        &l.files,
        &l.id,
        &[request(1.0, 1)],
        PageQuality::Save,
        limits(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    l
}

#[tokio::test]
async fn downloading_again_replaces_the_chapter_with_the_original_pages() {
    let c = FakeCatalog::new();
    let pages = vec![jpeg(1400, 2800, 1), jpeg(1400, 2800, 2)];
    add_chapter(&c, 1, pages.clone());
    let l = saved_small("re-ok", &c).await;
    let before = fs::read(l.files.page_path(&l.id, "c1", 0)).unwrap();
    assert_ne!(before, pages[0]);

    let out = download_chapters_as(
        &c,
        &l.files,
        &l.id,
        &[request(1.0, 1)],
        PageQuality::Full,
        true,
        limits(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap();
    assert_eq!(out.chapters[0].quality, Some(PageQuality::Full));
    assert_eq!(out.chapters[0].pages, 2);
    for (i, page) in pages.iter().enumerate() {
        assert_eq!(&fs::read(l.files.page_path(&l.id, "c1", i)).unwrap(), page);
    }
    let left = names_in(&l.files.series_dir(&l.id));
    assert_eq!(left, ["c1"], "no staging or old folders left: {left:?}");
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn a_failed_download_again_leaves_the_existing_chapter_exactly_as_it_was() {
    let c = FakeCatalog::new();
    add_chapter(&c, 1, vec![jpeg(1400, 2800, 1), jpeg(1400, 2800, 2)]);
    let l = saved_small("re-fail", &c).await;
    let snapshot: Vec<Vec<u8>> = (0..2).map(|i| fs::read(l.files.page_path(&l.id, "c1", i)).unwrap()).collect();

    // The second page can no longer be fetched.
    c.set_pages(1, &["/c1/p0", "/c1/missing"]);
    let err = download_chapters_as(
        &c,
        &l.files,
        &l.id,
        &[request(1.0, 1)],
        PageQuality::Full,
        true,
        limits(),
        &quick(),
        None,
        &CancelFlag::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PipelineError::NothingDownloaded), "{err:?}");
    for (i, old) in snapshot.iter().enumerate() {
        assert_eq!(&fs::read(l.files.page_path(&l.id, "c1", i)).unwrap(), old, "page {i}");
    }
    assert_eq!(names_in(&l.files.series_dir(&l.id)), ["c1"], "the half copy is gone");
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn a_cancelled_download_again_leaves_the_existing_chapter_exactly_as_it_was() {
    let c = FakeCatalog::new();
    add_chapter(&c, 1, (0..12).map(|i| jpeg(300, 500, i)).collect());
    let l = saved_small("re-cancel", &c).await;
    let snapshot: Vec<Vec<u8>> = (0..12).map(|i| fs::read(l.files.page_path(&l.id, "c1", i)).unwrap()).collect();
    c.set_path_latency_ms("/c1/", 60);
    let cancel = CancelFlag::new();
    let flag = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        flag.cancel();
    });
    let err = download_chapters_as(
        &c,
        &l.files,
        &l.id,
        &[request(1.0, 1)],
        PageQuality::Full,
        true,
        limits(),
        &quick(),
        None,
        &cancel,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PipelineError::Cancelled), "{err:?}");
    for (i, old) in snapshot.iter().enumerate() {
        assert_eq!(&fs::read(l.files.page_path(&l.id, "c1", i)).unwrap(), old, "page {i}");
    }
    assert_eq!(names_in(&l.files.series_dir(&l.id)), ["c1"], "no staging folder is left");
    let _ = fs::remove_dir_all(l.root);
}

// ---- Exports -----------------------------------------------------------------

/// Chapters written straight into the library: `pages[c]` are the files of chapter `c + 1`.
fn series_with(l: &Lib, pages: &[Vec<Vec<u8>>]) -> DownloadedSeries {
    let mut db = LibraryDb::default();
    for (c, chapter) in pages.iter().enumerate() {
        let key = format!("c{}", c + 1);
        fs::create_dir_all(l.files.chapter_dir(&l.id, &key)).unwrap();
        for (p, bytes) in chapter.iter().enumerate() {
            fs::write(l.files.page_path(&l.id, &key, p), bytes).unwrap();
        }
        db.record_chapter(
            TITLE,
            &ConversionSettings::default(),
            library::DownloadedChapter {
                key,
                number: Some((c + 1) as f64),
                name: format!("Chapter {}", c + 1),
                source_id: "s".into(),
                chapter_id: c as i64,
                pages: chapter.len(),
                bytes: 0,
                downloaded_at: 1,
                quality: Some(PageQuality::Full),
            },
            1,
        );
    }
    let mut series = db.series.remove(0);
    series.info = Some(library::SeriesInfo {
        author: "Some Writer".into(),
        artist: "Some Artist".into(),
        genres: vec!["Action".into(), "Manga".into()],
        description: "A story.".into(),
        ..Default::default()
    });
    series
}

fn job(l: &Lib, series: &DownloadedSeries, format: OriginalFormat, grouping: Grouping) -> ExportJob {
    let mut j = ExportJob::new(
        series.clone(),
        series.chapters.iter().map(|c| c.key.clone()).collect(),
        device_profiles::phone_original(),
        ExportTarget::Folder { path: l.out() },
        l.work(),
    );
    j.grouping = grouping;
    j.original = Some(OriginalExport { format, rtl: true });
    j
}

fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.push((f.name().unwrap().to_string(), data));
    }
    out
}

#[tokio::test]
async fn a_cbz_holds_the_stored_pages_byte_for_byte_in_reading_order_with_comic_info() {
    let l = lib("cbz");
    let a = vec![jpeg(300, 450, 1), png(300, 450, 2), webp(60, 90, 3)];
    let b = vec![jpeg(200, 300, 4), gif(40, 60, 5)];
    let series = series_with(&l, &[a.clone(), b.clone()]);
    let j = job(&l, &series, OriginalFormat::Cbz, Grouping::Single);
    let outcome = export_series(&l.files, &j, None, &CancelFlag::new()).await.unwrap();
    assert_eq!(outcome.files.len(), 1);
    assert_eq!((outcome.pages, outcome.chapters), (5, 2));
    let path = PathBuf::from(&outcome.files[0].location);
    assert!(path.extension().is_some_and(|e| e == "cbz"), "{path:?}");

    let entries = zip_entries(&path);
    let pages: Vec<&(String, Vec<u8>)> = entries.iter().filter(|(n, _)| n != "ComicInfo.xml").collect();
    assert_eq!(pages.len(), 5);
    let mut sorted: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();
    let as_written = sorted.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, as_written, "the names sort in reading order");
    // JPEG, PNG and WebP are copied untouched.
    assert_eq!(pages[0].1, a[0]);
    assert_eq!(pages[1].1, a[1]);
    assert_eq!(pages[2].1, a[2]);
    assert!(pages[2].0.ends_with(".webp") && pages[0].0.ends_with(".jpg") && pages[1].0.ends_with(".png"));
    assert_eq!(pages[3].1, b[0]);
    // The GIF was turned into a PNG, which keeps every pixel.
    assert!(pages[4].0.ends_with(".png"));
    let decoded = image::load_from_memory(&pages[4].1).unwrap().to_rgb8();
    let source = image::load_from_memory(&b[1]).unwrap().to_rgb8();
    assert_eq!(decoded.as_raw(), source.as_raw());

    let info = String::from_utf8(entries.iter().find(|(n, _)| n == "ComicInfo.xml").unwrap().1.clone()).unwrap();
    assert!(info.contains("<Series>Original Series</Series>"), "{info}");
    assert!(info.contains("<Writer>Some Writer</Writer>"));
    assert!(info.contains("<Genre>Action, Manga</Genre>"));
    assert!(info.contains("YesAndRightToLeft"));
    assert!(info.contains("Bookmark=\"Chapter 1\"") && info.contains("Bookmark=\"Chapter 2\""));
    assert!(info.contains("<PageCount>5</PageCount>"));
    assert!(names_in(&l.work()).is_empty(), "no scratch file is left");
    let _ = fs::remove_dir_all(l.root);
}

/// The objects of a PDF, read back the way a viewer would.
struct Pdf {
    objects: Vec<(u32, String, Option<Vec<u8>>)>,
    xref_size: usize,
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Check the structure of a PDF (header, trailer, every cross-reference entry)
/// and return what is in it.
fn read_pdf(bytes: &[u8]) -> Pdf {
    assert!(bytes.starts_with(b"%PDF-1."), "header");
    assert!(bytes.ends_with(b"%%EOF\n"), "end marker");
    let start_at = bytes.windows(9).rposition(|w| w == b"startxref").expect("startxref");
    let tail = std::str::from_utf8(&bytes[start_at..]).unwrap();
    let xref_offset: usize = tail.lines().nth(1).unwrap().trim().parse().unwrap();
    assert!(bytes[xref_offset..].starts_with(b"xref\n"), "startxref points at the table");
    let table = std::str::from_utf8(&bytes[xref_offset..start_at]).unwrap();
    let mut lines = table.lines();
    assert_eq!(lines.next(), Some("xref"));
    let head: Vec<usize> = lines.next().unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
    assert_eq!(head[0], 0);
    let size = head[1];
    assert_eq!(lines.next().unwrap(), "0000000000 65535 f ", "the free entry");
    let trailer_at = table.find("trailer").expect("trailer");
    let entries: Vec<&str> = table[..trailer_at].lines().skip(3).collect();
    assert_eq!(entries.len(), size - 1, "one entry for each object");
    let trailer = &table[trailer_at..];
    assert!(trailer.contains(&format!("/Size {size}")) && trailer.contains("/Root 1 0 R"), "{trailer}");

    let mut objects = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let number = i as u32 + 1;
        assert_eq!(entry.len(), 19, "{entry:?} (plus a newline makes the 20 bytes the format needs)");
        assert!(entry.ends_with(" 00000 n ") || entry.ends_with("n "), "{entry}");
        let offset: usize = entry[..10].parse().unwrap();
        let label = format!("{number} 0 obj\n");
        assert!(bytes[offset..].starts_with(label.as_bytes()), "object {number} is at its offset");
        let body = offset + label.len();
        let end = find(bytes, b"\nendobj\n", body).expect("endobj");
        let stream_at = find(bytes, b"\nstream\n", body).filter(|&s| s < end);
        match stream_at {
            Some(s) => {
                let dict = String::from_utf8_lossy(&bytes[body..s]).into_owned();
                let length: usize = dict
                    .split("/Length ")
                    .nth(1)
                    .and_then(|t| t.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|n| n.parse().ok())
                    .expect("a stream has its length");
                let data_from = s + "\nstream\n".len();
                let data = bytes[data_from..data_from + length].to_vec();
                assert!(bytes[data_from + length..].starts_with(b"\nendstream"), "the length is right");
                objects.push((number, dict, Some(data)));
            }
            None => objects.push((number, String::from_utf8_lossy(&bytes[body..end]).into_owned(), None)),
        }
    }
    Pdf { objects, xref_size: size }
}

impl Pdf {
    fn pages(&self) -> Vec<&(u32, String, Option<Vec<u8>>)> {
        self.objects.iter().filter(|(_, d, _)| d.contains("/Type /Page ")).collect()
    }
    fn dct_streams(&self) -> Vec<&Vec<u8>> {
        self.objects
            .iter()
            .filter(|(_, d, _)| d.contains("/DCTDecode"))
            .filter_map(|(_, _, s)| s.as_ref())
            .collect()
    }
}

#[tokio::test]
async fn a_pdf_is_valid_has_a_page_per_picture_and_embeds_each_jpeg_unchanged() {
    let l = lib("pdf");
    let a = vec![jpeg(300, 450, 1), jpeg(310, 460, 2), png(120, 180, 3)];
    let b = vec![webp(60, 90, 4), jpeg(200, 300, 5)];
    let series = series_with(&l, &[a.clone(), b.clone()]);
    let j = job(&l, &series, OriginalFormat::Pdf, Grouping::Single);
    let outcome = export_series(&l.files, &j, None, &CancelFlag::new()).await.unwrap();
    assert_eq!(outcome.files.len(), 1);
    let path = PathBuf::from(&outcome.files[0].location);
    assert!(path.extension().is_some_and(|e| e == "pdf"));
    let bytes = fs::read(&path).unwrap();
    let pdf = read_pdf(&bytes);

    assert_eq!(pdf.pages().len(), 5);
    let tree = pdf.objects.iter().find(|(n, _, _)| *n == 2).unwrap();
    assert!(tree.1.contains("/Type /Pages") && tree.1.contains("/Count 5"), "{}", tree.1);
    assert_eq!(pdf.xref_size, pdf.objects.len() + 1);
    // A page is as big as its picture.
    assert!(pdf.pages()[0].1.contains("/MediaBox [0 0 300 450]"), "{}", pdf.pages()[0].1);
    assert!(pdf.pages()[2].1.contains("/MediaBox [0 0 120 180]"));
    // The JPEGs are in untouched; the lossless PNG was written without loss
    // (checked below) and the WebP became a JPEG.
    let dct = pdf.dct_streams();
    assert_eq!(dct.len(), 4);
    assert_eq!(dct[0], &a[0]);
    assert_eq!(dct[1], &a[1]);
    assert_eq!(dct[3], &b[1]);
    let flate: Vec<_> = pdf.objects.iter().filter(|(_, d, _)| d.contains("/FlateDecode")).collect();
    assert_eq!(flate.len(), 1);
    assert!(flate[0].1.contains("/DeviceRGB") && flate[0].1.contains("/Predictor 15"));
    // Undo the "Up" predictor and compare the pixels with the PNG.
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&flate[0].2.as_ref().unwrap()[..]).read_to_end(&mut raw).unwrap();
    let (w, h) = (120usize, 180usize);
    let source = image::load_from_memory(&a[2]).unwrap().to_rgb8();
    let mut rows: Vec<Vec<u8>> = Vec::new();
    for y in 0..h {
        let line = &raw[y * (w * 3 + 1)..(y + 1) * (w * 3 + 1)];
        assert_eq!(line[0], 2);
        let row: Vec<u8> = line[1..]
            .iter()
            .enumerate()
            .map(|(i, v)| if y == 0 { *v } else { v.wrapping_add(rows[y - 1][i]) })
            .collect();
        rows.push(row);
    }
    assert_eq!(rows.concat(), *source.as_raw(), "no pixel changed");
    // A chapter list and the metadata.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Type /Outlines") && text.contains("(Chapter 1)") && text.contains("(Chapter 2)"));
    assert!(text.contains("/Title (Original Series)") && text.contains("/Author (Some Writer)"));
    assert!(text.contains("/Direction /R2L"));
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn grouping_gives_one_file_per_series_per_chapter_or_every_n() {
    let l = lib("groups");
    let chapters: Vec<Vec<Vec<u8>>> = (0..5).map(|c| vec![jpeg(80, 120, c as u8), jpeg(80, 120, c as u8 + 50)]).collect();
    let series = series_with(&l, &chapters);
    for (grouping, expected, tag) in [
        (Grouping::Single, 1usize, "one"),
        (Grouping::PerChapter, 5, "each"),
        (Grouping::EveryN(2), 3, "two"),
    ] {
        for format in OriginalFormat::ALL {
            let mut j = job(&l, &series, format, grouping);
            j.target = ExportTarget::Folder { path: l.out().join(format!("{tag}-{}", format.extension())) };
            let outcome = export_series(&l.files, &j, None, &CancelFlag::new()).await.unwrap();
            assert_eq!(outcome.files.len(), expected, "{grouping:?} {format:?}");
            assert_eq!(outcome.pages, 10);
            // Every file is a book of its own: no empty ones, none shared.
            let mut total = 0;
            for f in &outcome.files {
                let bytes = fs::read(&f.location).unwrap();
                total += match format {
                    OriginalFormat::Pdf => read_pdf(&bytes).pages().len(),
                    OriginalFormat::Cbz => zip_entries(Path::new(&f.location)).len() - 1,
                };
            }
            assert_eq!(total, 10);
        }
    }
    // Names say what is inside.
    let mut j = job(&l, &series, OriginalFormat::Cbz, Grouping::EveryN(2));
    j.target = ExportTarget::Folder { path: l.out().join("names") };
    let outcome = export_series(&l.files, &j, None, &CancelFlag::new()).await.unwrap();
    let names = names_in(&l.out().join("names"));
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names[0].contains("Ch 1-2") && names[1].contains("Ch 3-4") && names[2].contains("Ch 5"), "{names:?}");
    drop(outcome);
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn cancelling_stops_the_export_and_leaves_no_partial_file_anywhere() {
    for format in OriginalFormat::ALL {
        let l = lib(&format!("cancel-{}", format.extension()));
        let chapters: Vec<Vec<Vec<u8>>> = (0..6).map(|c| vec![jpeg(60, 90, c as u8); 4]).collect();
        let series = series_with(&l, &chapters);
        let j = job(&l, &series, format, Grouping::PerChapter);
        let cancel = CancelFlag::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let flag = cancel.clone();
        // Cancel as soon as the first chapter is finished.
        let watcher = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if matches!(event, JobEvent::ChapterDone { .. }) {
                    flag.cancel();
                }
            }
        });
        let err = export_series(&l.files, &j, Some(tx), &cancel).await.unwrap_err();
        assert!(matches!(err, PipelineError::Cancelled), "{err:?}");
        watcher.abort();
        assert!(names_in(&l.out()).is_empty(), "nothing was sent: {:?}", names_in(&l.out()));
        assert!(names_in(&l.work()).is_empty(), "no scratch file: {:?}", names_in(&l.work()));
        let _ = fs::remove_dir_all(l.root);
    }
}

#[tokio::test]
async fn a_cancel_already_made_writes_nothing() {
    let l = lib("cancelled-first");
    let series = series_with(&l, &[vec![jpeg(60, 90, 1)]]);
    let j = job(&l, &series, OriginalFormat::Cbz, Grouping::Single);
    let cancel = CancelFlag::new();
    cancel.cancel();
    let err = export_series(&l.files, &j, None, &cancel).await.unwrap_err();
    assert!(matches!(err, PipelineError::Cancelled), "{err:?}");
    assert!(names_in(&l.out()).is_empty() && names_in(&l.work()).is_empty());
    let _ = fs::remove_dir_all(l.root);
}

#[tokio::test]
async fn an_unreadable_page_stops_the_export_and_says_which() {
    let l = lib("bad-page");
    let series = series_with(&l, &[vec![jpeg(60, 90, 1), b"not a picture".to_vec()]]);
    for format in OriginalFormat::ALL {
        let j = job(&l, &series, format, Grouping::Single);
        let err = export_series(&l.files, &j, None, &CancelFlag::new()).await.unwrap_err();
        assert!(err.to_string().contains("Chapter 1"), "{format:?}: {err}");
        assert!(names_in(&l.out()).is_empty());
        assert!(names_in(&l.work()).is_empty(), "scratch is swept: {:?}", names_in(&l.work()));
    }
    let _ = fs::remove_dir_all(l.root);
}
