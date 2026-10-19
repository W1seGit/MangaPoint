//! The importer against every kind of file it reads. The fixtures are made
//! here: pictures are real PNGs and JPEGs, archives are written by the same
//! libraries or by the app's own CBZ and EPUB writers, so an export can be
//! imported back.

use super::*;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use library::{LibraryFiles, IMPORT_SOURCE};

mod write;

/// Read these, with nothing to cancel it.
fn scan(paths: &[PathBuf]) -> ScanResult {
    super::scan(paths, &CancelFlag::new())
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mp-import-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(shade: u8) -> Vec<u8> {
    let img = image::GrayImage::from_pixel(12, 18, image::Luma([shade]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(shade: u8) -> Vec<u8> {
    let img = image::GrayImage::from_pixel(16, 16, image::Luma([shade]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

fn write_zip(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored))
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// The file kind of a written page, from its first bytes.
fn kind(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes).unwrap() {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        _ => "other",
    }
}

struct Lib {
    files: LibraryFiles,
}

fn lib(tag: &str) -> Lib {
    let dir = temp(tag);
    Lib { files: LibraryFiles::new(dir.join("library")) }
}

/// Write a book into a fresh series and read the result back from disk.
fn commit_all(book: &ImportBook, lib: &Lib) -> CommitOutcome {
    commit_into(book, lib, "series", &[])
}

fn commit_into(book: &ImportBook, lib: &Lib, series: &str, existing: &[DownloadedChapter]) -> CommitOutcome {
    let target = CommitTarget { series_id: series, existing, first_id: -1 - existing.len() as i64, now: 100 };
    commit(book, &lib.files, &target, &mut |_, _| {}, &CancelFlag::new()).unwrap()
}

fn shades_on_disk(lib: &Lib, series: &str, key: &str) -> Vec<u8> {
    lib.files
        .page_files(series, key)
        .iter()
        .map(|p| {
            let bytes = std::fs::read(p).unwrap();
            image::load_from_memory(&bytes).unwrap().to_luma8().get_pixel(0, 0).0[0]
        })
        .collect()
}

// ---- Folders and loose pictures ----------------------------------------------

#[test]
fn a_folder_of_folders_is_chapters_and_loose_pictures_are_one_chapter() {
    let dir = temp("folders");
    let root = dir.join("My Manga");
    for (chapter, shades) in [("Chapter 10", vec![30u8, 40]), ("Chapter 2", vec![10, 20, 25]), ("Chapter 1", vec![5])] {
        std::fs::create_dir_all(root.join(chapter)).unwrap();
        for (i, shade) in shades.iter().enumerate() {
            std::fs::write(root.join(chapter).join(format!("{:03}.png", i + 1)), png(*shade)).unwrap();
        }
    }
    std::fs::write(root.join("cover.png"), png(222)).unwrap();
    let result = scan(std::slice::from_ref(&root));
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::Folder);
    assert_eq!(book.meta.title, "My Manga");
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.pages.len())).collect();
    assert_eq!(chapters, [(Some(1.0), 1), (Some(2.0), 3), (Some(10.0), 2)]);
    assert_eq!(book.page_count(), 6, "the cover beside the chapters is not a page");
    assert!(book.cover.is_some());

    let lib = lib("folders-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(outcome.chapters.len(), 3);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [10, 20, 25]);
    assert_eq!(outcome.chapters[1].pages, 3);
    assert!(outcome.chapters[1].bytes > 0);
    assert_eq!(outcome.chapters[0].source_id, IMPORT_SOURCE);

    // Loose pictures from one folder: one book of one chapter, in order.
    let loose = dir.join("Loose");
    std::fs::create_dir_all(&loose).unwrap();
    let names = ["p10.png", "p2.png", "p1.jpg"];
    for (name, shade) in names.iter().zip([30u8, 20, 10]) {
        let bytes = if name.ends_with("jpg") { jpeg(shade) } else { png(shade) };
        std::fs::write(loose.join(name), bytes).unwrap();
    }
    let result = scan(&names.iter().map(|n| loose.join(n)).collect::<Vec<_>>());
    assert_eq!(result.books.len(), 1);
    assert_eq!(result.books[0].format, Format::Images);
    assert_eq!(result.books[0].meta.title, "Loose");
    let keys: Vec<_> = result.books[0].chapters[0].pages.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["p1.jpg", "p2.png", "p10.png"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_page_that_is_not_a_picture_is_skipped_and_the_rest_close_the_gap() {
    let dir = temp("gap");
    let root = dir.join("Odd");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("1.png"), png(10)).unwrap();
    std::fs::write(root.join("2.png"), b"this is not a picture").unwrap();
    std::fs::write(root.join("3.png"), png(30)).unwrap();
    let book = scan(&[root]).books.remove(0);
    assert_eq!(book.page_count(), 3);
    let lib = lib("gap-lib");
    let outcome = commit_all(&book, &lib);
    assert_eq!(outcome.skipped_pages, 1);
    assert_eq!(outcome.chapters[0].pages, 2);
    assert_eq!(shades_on_disk(&lib, "series", &outcome.chapters[0].key), [10, 30]);
    let names: Vec<_> = lib
        .files
        .page_files("series", &outcome.chapters[0].key)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["00001.img", "00002.img"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_of_archives_is_a_series_with_a_chapter_in_each() {
    let dir = temp("archives");
    let root = dir.join("One Piece");
    std::fs::create_dir_all(&root).unwrap();
    write_zip(&root.join("One Piece - c10.cbz"), &[("1.png", png(10)), ("2.png", png(11))]);
    write_zip(&root.join("One Piece - c2.cbz"), &[("1.png", png(20))]);
    let book = scan(&[root]).books.remove(0);
    assert_eq!(book.meta.title, "One Piece");
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.pages.len())).collect();
    assert_eq!(chapters, [(Some(2.0), 1), (Some(10.0), 2)]);
    assert_eq!(book.sources.len(), 2);
    let lib = lib("archives-lib");
    let outcome = commit_all(&book, &lib);
    assert_eq!(outcome.chapters.len(), 2);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [20]);
    assert_eq!(shades_on_disk(&lib, "series", "c10"), [10, 11]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- CBZ and ZIP ---------------------------------------------------------------

#[test]
fn a_cbz_from_our_own_export_imports_back_with_its_details_chapters_and_cover() {
    let dir = temp("roundtrip");
    let path = dir.join("book.cbz");
    let meta = crate::cbz::CbzMeta {
        series: "Round Trip".into(),
        language: "en".into(),
        rtl: true,
        summary: "A book & its pages.".into(),
        author: "A. Writer".into(),
        artist: "B. Artist".into(),
        genres: vec!["Action".into(), "Drama".into()],
        ..Default::default()
    };
    let mut builder = crate::cbz::CbzBuilder::create(&path, meta).unwrap();
    for shade in [10u8, 20, 30, 40, 50] {
        builder.add_page(&png(shade)).unwrap();
    }
    builder.push_chapter("Chapter 1", 0, Some(1.0));
    builder.push_chapter("Chapter 2: The Fall", 2, Some(2.0));
    builder.finish().unwrap();

    let result = scan(&[path]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::Cbz);
    assert_eq!(book.meta.title, "Round Trip");
    assert_eq!((book.meta.author.as_str(), book.meta.artist.as_str()), ("A. Writer", "B. Artist"));
    assert_eq!(book.meta.description, "A book & its pages.");
    assert_eq!(book.meta.genres, ["Action", "Drama"]);
    assert_eq!(book.meta.right_to_left, Some(true));
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.name.as_str(), c.pages.len())).collect();
    assert_eq!(chapters, [(Some(1.0), "Chapter 1", 2), (Some(2.0), "Chapter 2: The Fall", 3)]);
    let cover = image::load_from_memory(book.cover.as_ref().unwrap()).unwrap().to_luma8();
    assert_eq!(cover.get_pixel(0, 0).0[0], 10, "the cover is the front page");

    let lib = lib("roundtrip-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [30, 40, 50]);
    assert_eq!(outcome.skipped_pages, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_zip_named_for_a_chapter_with_no_details_is_one_chapter_and_names_are_never_paths() {
    let dir = temp("zip");
    let path = dir.join("Berserk - Chapter 41.zip");
    write_zip(
        &path,
        &[("../../evil.png", png(10)), ("001.png", png(20)), ("sub/../002.png", png(30)), ("notes.txt", b"hi".to_vec())],
    );
    let book = scan(&[path]).books.remove(0);
    assert_eq!(book.format, Format::Zip);
    assert_eq!(book.meta.title, "Berserk");
    assert_eq!(book.chapters.len(), 1);
    assert_eq!(book.chapters[0].number, Some(41.0));
    let lib = lib("zip-lib");
    let outcome = commit_all(&book, &lib);
    assert_eq!(outcome.chapters[0].pages, 3);
    // Everything was written inside the library, under our own names.
    assert!(!dir.join("evil.png").exists() && !dir.parent().unwrap().join("evil.png").exists());
    let written: Vec<_> = lib
        .files
        .page_files("series", &outcome.chapters[0].key)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written, ["00001.img", "00002.img", "00003.img"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_password_protected_zip_and_a_zip_with_no_pictures_are_refused_with_reasons() {
    let dir = temp("refuse");
    let locked = dir.join("secret.cbz");
    write_zip(&locked, &[("1.png", png(9))]);
    // Mark the entry as encrypted (bit 0 of the general purpose flags) in the
    // central directory, which is how a reader learns of a password.
    let mut bytes = std::fs::read(&locked).unwrap();
    let at = bytes.windows(4).position(|w| w == b"PK").unwrap();
    bytes[at + 8] |= 1;
    std::fs::write(&locked, bytes).unwrap();
    let text = dir.join("notes.zip");
    write_zip(&text, &[("a.txt", b"x".to_vec())]);
    let mobi = dir.join("book.mobi");
    std::fs::write(&mobi, b"BOOKMOBI").unwrap();
    let odd = dir.join("thing.xyz");
    std::fs::write(&odd, b"?").unwrap();
    let result = scan(&[locked, text, mobi, odd, dir.join("missing.cbz")]);
    assert!(result.books.is_empty());
    let messages: Vec<String> = result.failures.iter().map(|f| f.message.clone()).collect();
    assert!(messages[0].contains("password"), "{}", messages[0]);
    assert!(messages[1].contains("no page pictures"), "{}", messages[1]);
    assert!(messages[2].contains("Calibre"), "{}", messages[2]);
    assert!(messages[3].contains("not a file type"), "{}", messages[3]);
    assert_eq!(result.failures[4].file, "missing.cbz");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_with_the_wrong_extension_is_read_for_what_it_is() {
    let dir = temp("sniff");
    let path = dir.join("actually-a-zip.pdf");
    write_zip(&path, &[("1.png", png(10))]);
    let book = scan(&[path]).books.remove(0);
    assert_eq!(book.format, Format::Zip);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- EPUB ----------------------------------------------------------------------

#[test]
fn an_epub_from_our_own_export_imports_back_with_its_chapters_and_direction() {
    let dir = temp("epub");
    let path = dir.join("manga.epub");
    let meta = crate::epub::EpubMeta {
        title: "Epub Series".into(),
        language: "ja".into(),
        rtl: true,
        width: 12,
        height: 18,
        created: 0,
    };
    let mut builder = crate::epub::EpubBuilder::create(&path, meta).unwrap();
    for shade in [10u8, 20, 30, 40] {
        builder.add_page(&png(shade)).unwrap();
    }
    builder.push_chapter("Chapter 1", 0);
    builder.push_chapter("Chapter 2", 2);
    builder.finish().unwrap();
    let result = scan(&[path]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::Epub);
    assert_eq!(book.meta.title, "Epub Series");
    assert_eq!(book.meta.right_to_left, Some(true));
    assert_eq!(book.meta.language, "ja");
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.name.as_str(), c.pages.len())).collect();
    assert_eq!(chapters, [(Some(1.0), "Chapter 1", 2), (Some(2.0), "Chapter 2", 2)]);
    assert!(book.cover.is_some());
    let lib = lib("epub-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [30, 40]);
    assert_eq!(outcome.skipped_pages, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_epub_of_text_is_refused_and_a_hand_made_epub_reads_its_details_and_cover() {
    let dir = temp("epub2");
    let text = dir.join("novel.epub");
    let container = br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = |extra_item: &str, spine: &str| {
        format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
            <dc:title>Hand Made</dc:title><dc:creator opf:role="aut">The Writer</dc:creator><dc:creator opf:role="ill">The Artist</dc:creator>
            <dc:description>&lt;p&gt;About &amp;amp; this&lt;/p&gt;</dc:description><dc:subject>Fantasy</dc:subject><dc:language>en</dc:language>
            <meta name="cover" content="cov"/></metadata>
            <manifest><item id="t" href="t.xhtml" media-type="application/xhtml+xml"/>{extra_item}</manifest><spine>{spine}</spine></package>"#
        )
    };
    write_zip(
        &text,
        &[
            ("META-INF/container.xml", container.to_vec()),
            ("OEBPS/content.opf", opf("", r#"<itemref idref="t"/>"#).into_bytes()),
            ("OEBPS/t.xhtml", b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Just words.</p></body></html>".to_vec()),
        ],
    );
    let made = dir.join("made.epub");
    let items = r#"<item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/>
        <item id="i1" href="img/a%201.png" media-type="image/png"/><item id="i2" href="img/b.jpg" media-type="image/jpeg"/><item id="cov" href="img/b.jpg" media-type="image/jpeg" properties="cover-image"/>"#;
    write_zip(
        &made,
        &[
            ("META-INF/container.xml", container.to_vec()),
            ("OEBPS/content.opf", opf(items, r#"<itemref idref="p2"/><itemref idref="p1"/>"#).into_bytes()),
            ("OEBPS/p1.xhtml", b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><img src=\"img/a%201.png\"/></body></html>".to_vec()),
            ("OEBPS/p2.xhtml", b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><img src=\"img/b.jpg\"/></body></html>".to_vec()),
            ("OEBPS/img/a 1.png", png(11)),
            ("OEBPS/img/b.jpg", jpeg(99)),
        ],
    );
    let result = scan(&[text, made]);
    assert_eq!(result.failures.len(), 1);
    assert!(result.failures[0].message.contains("book of text"), "{}", result.failures[0].message);
    let book = &result.books[0];
    assert_eq!(book.meta.title, "Hand Made");
    assert_eq!((book.meta.author.as_str(), book.meta.artist.as_str()), ("The Writer", "The Artist"));
    assert_eq!(book.meta.description, "About & this");
    assert_eq!(book.meta.genres, ["Fantasy"]);
    let keys: Vec<_> = book.chapters[0].pages.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["OEBPS/img/b.jpg", "OEBPS/img/a 1.png"], "pages follow the reading order, not the file names");
    assert_eq!(image::guess_format(book.cover.as_ref().unwrap()).unwrap(), image::ImageFormat::Jpeg);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- PDF, 7Z, RAR, TIFF --------------------------------------------------------

#[test]
fn a_pdf_of_pictures_imports_page_by_page() {
    let dir = temp("pdf");
    let path = dir.join("Scans - ch3.pdf");
    pdf_in::tests::make_pdf(&path, &[(16, 16, Some("DCTDecode"), jpeg(70)), (12, 18, None, vec![140; 12 * 18])], "Scans");
    let result = scan(&[path]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::Pdf);
    assert_eq!(book.chapters[0].number, Some(3.0));
    let lib = lib("pdf-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(outcome.chapters[0].pages, 2);
    let first = std::fs::read(&lib.files.page_files("series", "c3")[0]).unwrap();
    assert_eq!(kind(&first), "jpg");
    assert_eq!(shades_on_disk(&lib, "series", "c3")[1], 140);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_7z_archive_imports_with_its_folders_as_chapters() {
    let dir = temp("sevenz");
    let path = dir.join("Series.cb7");
    sevenz_in::tests::make_7z(
        &path,
        &[
            ("Chapter 2/1.png", png(30)),
            ("Chapter 1/2.png", png(20)),
            ("Chapter 1/1.png", png(10)),
            ("cover.jpg", jpeg(200)),
        ],
    );
    let result = scan(&[path]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::SevenZ);
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.pages.len())).collect();
    assert_eq!(chapters, [(Some(1.0), 2), (Some(2.0), 1)]);
    assert_eq!(image::guess_format(book.cover.as_ref().unwrap()).unwrap(), image::ImageFormat::Jpeg);
    let lib = lib("sevenz-lib");
    commit_all(book, &lib);
    assert_eq!(shades_on_disk(&lib, "series", "c1"), [10, 20]);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [30]);
    let _ = std::fs::remove_dir_all(&dir);
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn vint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// One RAR 5 block: CRC, header size, then type, flags and the rest.
fn rar_block(kind: u64, flags: u64, rest: &[u8]) -> Vec<u8> {
    let mut header = vint(kind);
    header.extend(vint(flags));
    header.extend_from_slice(rest);
    let mut sized = vint(header.len() as u64);
    sized.extend(header);
    let mut out = crc32(&sized).to_le_bytes().to_vec();
    out.extend(sized);
    out
}

/// A RAR 5 archive of stored (uncompressed) files, written by hand: a RAR cannot
/// be made without WinRAR, and the unpacker checks every CRC, so this proves the
/// reading code on a real archive.
fn make_rar5(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    out.extend(rar_block(1, 0, &vint(0)));
    for (name, data) in files {
        // Data size, file flags (CRC present), size, attributes, CRC, compression
        // (stored), host OS, name.
        let mut rest = vint(data.len() as u64);
        rest.extend(vint(0x04));
        rest.extend(vint(data.len() as u64));
        rest.extend(vint(0));
        rest.extend(crc32(data).to_le_bytes());
        rest.extend(vint(0));
        rest.extend(vint(1));
        rest.extend(vint(name.len() as u64));
        rest.extend_from_slice(name.as_bytes());
        out.extend(rar_block(2, 0x02, &rest));
        out.extend_from_slice(data);
    }
    out.extend(rar_block(5, 0, &vint(0)));
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_rar_archive_imports_with_its_folders_as_chapters() {
    let dir = temp("rar");
    let path = dir.join("Series.cbr");
    make_rar5(
        &path,
        &[("Ch 2/1.png", png(30)), ("Ch 1/2.png", png(20)), ("Ch 1/1.png", png(10)), ("notes.txt", b"hello".to_vec())],
    );
    let result = scan(&[path]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    let book = &result.books[0];
    assert_eq!(book.format, Format::Rar);
    let chapters: Vec<_> = book.chapters.iter().map(|c| (c.number, c.pages.len())).collect();
    assert_eq!(chapters, [(Some(1.0), 2), (Some(2.0), 1)]);
    let lib = lib("rar-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(outcome.skipped_pages, 0);
    assert_eq!(shades_on_disk(&lib, "series", "c1"), [10, 20]);
    assert_eq!(shades_on_disk(&lib, "series", "c2"), [30]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_rar_is_refused_without_a_crash() {
    let dir = temp("rarbad");
    let path = dir.join("broken.cbr");
    std::fs::write(&path, b"Rar!\x1a\x07\x01\x00this is not really an archive").unwrap();
    let result = scan(&[path]);
    assert!(result.books.is_empty());
    assert!(result.failures[0].message.contains("RAR"), "{}", result.failures[0].message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_multi_page_tiff_and_loose_tiffs_become_pngs() {
    let dir = temp("tiff");
    let multi = dir.join("scan.tif");
    tiff_in::tests::write_tiff(&multi, &[(12, 18, 40), (12, 18, 90)]);
    let single = dir.join("b.tiff");
    tiff_in::tests::write_tiff(&single, &[(12, 18, 160)]);
    let result = scan(&[multi, single]);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.books.len(), 1, "pictures from one folder are one book");
    let book = &result.books[0];
    assert_eq!(book.page_count(), 3);
    let lib = lib("tiff-lib");
    let outcome = commit_all(book, &lib);
    assert_eq!(outcome.chapters[0].pages, 3);
    assert_eq!(shades_on_disk(&lib, "series", &outcome.chapters[0].key), [160, 40, 90]);
    let first = std::fs::read(&lib.files.page_files("series", &outcome.chapters[0].key)[0]).unwrap();
    assert_eq!(kind(&first), "png");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Putting books together and into a series ------------------------------------

fn pages_book(label: &str, chapters: &[(Option<f64>, &str, usize)]) -> ImportBook {
    let mut book = ImportBook::new(label, Format::Cbz, BookSource::Zip(PathBuf::from(label)));
    for (number, name, pages) in chapters {
        let refs = (0..*pages).map(|i| PageRef { source: 0, key: format!("{name}/{i}.png") }).collect();
        book.chapters.push(ImportChapter::new(*number, *name, refs));
    }
    book
}

#[test]
fn books_merge_with_their_pages_pointing_at_the_right_source() {
    let a = pages_book("a.cbz", &[(Some(1.0), "one", 1)]);
    let mut b = pages_book("b.cbz", &[(Some(2.0), "two", 2)]);
    b.meta.author = "B".into();
    b.cover = Some(vec![1]);
    let merged = ImportBook::merge(vec![a, b]).unwrap();
    assert_eq!(merged.sources.len(), 2);
    assert_eq!(merged.chapters[1].pages[0].source, 1);
    assert_eq!(merged.meta.author, "B");
    assert_eq!(merged.cover, Some(vec![1]));
    assert_eq!(merged.page_count(), 3);
    assert!(ImportBook::merge(Vec::new()).is_none());
}

#[test]
fn books_with_the_same_chapter_numbers_are_numbered_on_when_put_together() {
    let a = pages_book("a.cbz", &[(Some(1.0), "Chapter 1", 1), (Some(2.0), "Chapter 2", 1)]);
    let b = pages_book("b.cbz", &[(Some(1.0), "Chapter 1", 1), (Some(2.0), "A named one", 1)]);
    let merged = ImportBook::merge(vec![a, b]).unwrap();
    let chapters: Vec<_> = merged.chapters.iter().map(|c| (c.number, c.name.as_str())).collect();
    assert_eq!(
        chapters,
        [(Some(1.0), "Chapter 1"), (Some(2.0), "Chapter 2"), (Some(3.0), "Chapter 3"), (Some(4.0), "A named one")]
    );
}

#[test]
fn chapters_without_numbers_are_numbered_after_the_ones_the_series_has() {
    let mut book = pages_book("loose", &[(None, "Prologue", 1), (Some(5.0), "Five", 1), (None, "Finale", 1)]);
    book.number_chapters(&[1.0, 2.0, 9.5]);
    let numbers: Vec<_> = book.chapters.iter().map(|c| c.number).collect();
    assert_eq!(numbers, [Some(10.0), Some(5.0), Some(11.0)]);
    let mut alone = pages_book("one", &[(None, "", 1)]);
    alone.number_chapters(&[]);
    assert_eq!(alone.chapters[0].number, Some(1.0));
    assert_eq!(alone.chapters[0].name, "Chapter 1");
}

fn existing(number: f64, key: &str, id: i64) -> DownloadedChapter {
    DownloadedChapter {
        key: key.into(),
        number: Some(number),
        name: format!("Chapter {number}"),
        source_id: IMPORT_SOURCE.into(),
        chapter_id: id,
        pages: 2,
        bytes: 10,
        downloaded_at: 1,
    }
}

#[test]
fn a_chapter_the_series_already_has_is_left_out_until_the_person_decides_otherwise() {
    let mut book = pages_book("more", &[(Some(1.0), "one", 1), (Some(3.0), "three", 1)]);
    book.meet(&[existing(1.0, "c1", -1), existing(2.0, "c2", -2)]);
    assert_eq!(book.chapters[0].meets.as_deref(), Some("c1"));
    assert!(book.chapters[0].skip, "left out by default");
    assert!(!book.chapters[1].skip && book.chapters[1].meets.is_none());
    assert_eq!(book.included_pages(), 1);
}

#[test]
fn pages_can_be_added_to_a_chapter_or_replace_it_and_a_cancel_leaves_nothing_behind() {
    let dir = temp("place");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for (name, shade) in [("a.png", 1u8), ("b.png", 2), ("c.png", 3)] {
        std::fs::write(src.join(name), png(shade)).unwrap();
    }
    let lib = lib("place-lib");
    let one = |names: &[&str], into: Placement, number: f64| {
        let mut book = ImportBook::new("x", Format::Images, BookSource::Dir(src.clone()));
        let pages = names.iter().map(|n| PageRef { source: 0, key: n.to_string() }).collect();
        let mut chapter = ImportChapter::new(Some(number), "x", pages);
        chapter.into = into;
        book.chapters.push(chapter);
        book
    };
    // A new chapter, then pages added after its own, then replaced.
    let first = commit_into(&one(&["a.png", "b.png"], Placement::New, 1.0), &lib, "s", &[]);
    assert_eq!(first.chapters[0].key, "c1");
    let have = first.chapters.clone();
    let more = commit_into(&one(&["c.png"], Placement::Append("c1".into()), 1.0), &lib, "s", &have);
    assert_eq!(more.chapters[0].pages, 3);
    assert_eq!(more.chapters[0].chapter_id, have[0].chapter_id, "the chapter keeps its id");
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [1, 2, 3]);
    let replaced = commit_into(&one(&["c.png"], Placement::Replace("c1".into()), 1.0), &lib, "s", &more.chapters);
    assert_eq!(replaced.chapters[0].pages, 1);
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [3]);
    // A new chapter whose number is taken does not take its folder.
    let clash = commit_into(&one(&["a.png"], Placement::New, 1.0), &lib, "s", &replaced.chapters);
    assert_ne!(clash.chapters[0].key, "c1");
    assert_eq!(clash.chapters[0].number, Some(1.0));
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [3], "the existing chapter is untouched");
    // Cancelled before anything is written: nothing is left of the new chapter.
    let cancel = CancelFlag::new();
    cancel.cancel();
    let book = one(&["a.png", "b.png"], Placement::New, 7.0);
    let target = CommitTarget { series_id: "s", existing: &[], first_id: -50, now: 1 };
    let result = commit(&book, &lib.files, &target, &mut |_, _| {}, &cancel);
    assert!(matches!(result, Err(PipelineError::Cancelled)));
    assert!(lib.files.page_files("s", "c7").is_empty());
    assert!(!lib.files.chapter_dir("s", "c7").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn progress_counts_the_pages_as_they_are_written() {
    let dir = temp("progress");
    let root = dir.join("P");
    std::fs::create_dir_all(&root).unwrap();
    for i in 0..5 {
        std::fs::write(root.join(format!("{i}.png")), png(i as u8)).unwrap();
    }
    let book = scan(&[root]).books.remove(0);
    let lib = lib("progress-lib");
    let mut seen = Vec::new();
    let target = CommitTarget { series_id: "s", existing: &[], first_id: -1, now: 1 };
    commit(&book, &lib.files, &target, &mut |d, t| seen.push((d, t)), &CancelFlag::new()).unwrap();
    assert_eq!(seen.first(), Some(&(0, 5)));
    assert_eq!(seen.last(), Some(&(5, 5)));
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    let _ = std::fs::remove_dir_all(&dir);
}
