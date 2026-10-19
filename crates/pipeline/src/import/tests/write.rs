//! Writing into the library safely: what a cancel, an error or an unreadable
//! book leaves behind (nothing), and how pages are placed in a chapter.

use super::*;
use library::{LibraryDb, LibraryFiles};

/// A book with one chapter whose pages are these pictures in a folder.
fn one_chapter(src: &Path, names: &[&str], into: Placement, number: f64) -> ImportBook {
    let mut book = ImportBook::new("x", Format::Images, BookSource::Dir(src.to_path_buf()));
    let pages = names.iter().map(|n| PageRef { source: 0, key: n.to_string() }).collect();
    let mut chapter = ImportChapter::new(Some(number), format!("Chapter {number}"), pages);
    chapter.into = into;
    book.chapters.push(chapter);
    book
}

fn pictures(dir: &Path, shades: &[(&str, u8)]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    for (name, shade) in shades {
        std::fs::write(dir.join(name), png(*shade)).unwrap();
    }
    dir.to_path_buf()
}

/// Every file under a folder with its bytes, to compare before and after.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.push((
                    path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// The library as one existing chapter "c1" of series "s" (two pages).
fn library_with_one_chapter(tag: &str) -> (Lib, Vec<DownloadedChapter>) {
    let src = pictures(&temp(&format!("{tag}-old")), &[("a.png", 1), ("b.png", 2)]);
    let lib = lib(&format!("{tag}-lib"));
    let first = commit_into(&one_chapter(&src, &["a.png", "b.png"], Placement::New, 1.0), &lib, "s", &[]);
    (lib, first.chapters)
}

fn vanished_source(book: &mut ImportBook, near: &Path) {
    book.sources.push(BookSource::Zip(near.join("vanished.cbz")));
    book.chapters[0].pages.push(PageRef { source: 1, key: "1.png".into() });
}

#[test]
fn a_replacement_that_is_cancelled_failed_or_unreadable_leaves_the_old_chapter_byte_for_byte() {
    let (lib, have) = library_with_one_chapter("replace-safe");
    let series_dir = lib.files.series_dir("s");
    let before = snapshot(&series_dir);
    assert_eq!(before.len(), 2);
    let new_src = pictures(&temp("replace-safe-new"), &[("x.png", 10), ("y.png", 20), ("z.png", 30)]);
    let replace = |names: &[&str]| one_chapter(&new_src, names, Placement::Replace("c1".into()), 1.0);
    let target = CommitTarget { series_id: "s", existing: &have, first_id: -9, now: 5 };

    // Cancelled part-way (after the first page is written).
    let cancel = CancelFlag::new();
    let c = cancel.clone();
    let r = commit(
        &replace(&["x.png", "y.png", "z.png"]),
        &lib.files,
        &target,
        &mut |done, _| {
            if done >= 1 {
                c.cancel();
            }
        },
        &cancel,
    );
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    assert_eq!(snapshot(&series_dir), before, "cancelled: the old chapter and nothing else");

    // Cancelled before it starts.
    let r = commit(&replace(&["x.png"]), &lib.files, &target, &mut |_, _| {}, &cancel);
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    assert_eq!(snapshot(&series_dir), before);

    // A second source that is gone: the read fails after the first page was written.
    let mut book = replace(&["x.png"]);
    vanished_source(&mut book, &new_src);
    let r = commit(&book, &lib.files, &target, &mut |_, _| {}, &CancelFlag::new());
    assert!(r.is_err() && !matches!(r, Err(PipelineError::Cancelled)));
    assert_eq!(snapshot(&series_dir), before, "failed: the old chapter and nothing else");

    // Every new page unreadable: nothing replaces the chapter.
    std::fs::write(new_src.join("bad.png"), b"not a picture").unwrap();
    let outcome = commit(&replace(&["bad.png"]), &lib.files, &target, &mut |_, _| {}, &CancelFlag::new()).unwrap();
    assert!(outcome.chapters.is_empty());
    assert_eq!(outcome.skipped_pages, 1);
    assert_eq!(snapshot(&series_dir), before, "unreadable: the old chapter and nothing else");

    // And a replacement that works replaces it, leaving no staging folder behind.
    let outcome = commit(
        &replace(&["x.png", "y.png", "z.png"]),
        &lib.files,
        &target,
        &mut |_, _| {},
        &CancelFlag::new(),
    )
    .unwrap();
    assert_eq!(outcome.chapters[0].pages, 3);
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [10, 20, 30]);
    let names: Vec<_> = std::fs::read_dir(&series_dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn a_cancelled_or_failed_import_leaves_no_files_new_or_appended() {
    let (lib, have) = library_with_one_chapter("clean-exit");
    let series_dir = lib.files.series_dir("s");
    let before = snapshot(&series_dir);
    let src = pictures(&temp("clean-exit-new"), &[("p.png", 5), ("q.png", 6), ("r.png", 7)]);
    // Appended pages and a new chapter in one book, cancelled after the first page.
    let mut book = one_chapter(&src, &["p.png", "q.png"], Placement::Append("c1".into()), 1.0);
    let mut extra = one_chapter(&src, &["r.png"], Placement::New, 2.0);
    book.chapters.append(&mut extra.chapters);
    let cancel = CancelFlag::new();
    let c = cancel.clone();
    let target = CommitTarget { series_id: "s", existing: &have, first_id: -9, now: 5 };
    let r = commit(
        &book,
        &lib.files,
        &target,
        &mut |done, _| {
            if done >= 1 {
                c.cancel();
            }
        },
        &cancel,
    );
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    assert_eq!(snapshot(&series_dir), before);

    // A series that did not exist before is not left as an empty folder.
    let target = CommitTarget { series_id: "fresh", existing: &[], first_id: -1, now: 5 };
    let cancel = CancelFlag::new();
    cancel.cancel();
    let book = one_chapter(&src, &["p.png"], Placement::New, 1.0);
    assert!(commit(&book, &lib.files, &target, &mut |_, _| {}, &cancel).is_err());
    assert!(!lib.files.series_dir("fresh").exists());
    let mut book = one_chapter(&src, &["p.png"], Placement::New, 1.0);
    vanished_source(&mut book, &src);
    assert!(commit(&book, &lib.files, &target, &mut |_, _| {}, &CancelFlag::new()).is_err());
    assert!(!lib.files.series_dir("fresh").exists(), "nothing of the failed import is left");
}

#[test]
fn appended_pages_go_after_the_highest_page_even_when_the_chapter_has_gaps() {
    let (lib, have) = library_with_one_chapter("append-gap");
    // The chapter loses its first page: 00002.img is all that is left.
    let dir = lib.files.chapter_dir("s", "c1");
    std::fs::remove_file(dir.join("00001.img")).unwrap();
    let src = pictures(&temp("append-gap-new"), &[("n.png", 99)]);
    let outcome = commit_into(&one_chapter(&src, &["n.png"], Placement::Append("c1".into()), 1.0), &lib, "s", &have);
    assert_eq!(outcome.chapters[0].pages, 2);
    assert!(dir.join("00002.img").exists() && dir.join("00003.img").exists());
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [2, 99], "the old page was not overwritten");
}

#[test]
fn pages_for_a_chapter_deleted_meanwhile_make_a_chapter_with_their_own_number() {
    let (lib, _have) = library_with_one_chapter("append-gone");
    let src = pictures(&temp("append-gone-new"), &[("n.png", 99)]);
    // The caller's list no longer has "c7".
    let outcome = commit_into(&one_chapter(&src, &["n.png"], Placement::Append("c7".into()), 7.0), &lib, "s", &[]);
    let chapter = &outcome.chapters[0];
    assert_eq!((chapter.number, chapter.name.as_str(), chapter.key.as_str()), (Some(7.0), "Chapter 7", "c7"));
    assert_eq!(shades_on_disk(&lib, "s", "c7"), [99]);
    let outcome = commit_into(&one_chapter(&src, &["n.png"], Placement::Replace("c8".into()), 8.0), &lib, "s", &[]);
    assert_eq!(outcome.chapters[0].number, Some(8.0));
}

#[test]
fn a_page_is_written_once_for_each_place_it_is_used_and_counted_once_each() {
    let src = pictures(&temp("twice"), &[("a.png", 4)]);
    let lib = lib("twice-lib");
    let mut book = one_chapter(&src, &["a.png", "a.png"], Placement::New, 1.0);
    book.chapters.push(ImportChapter::new(Some(2.0), "two", vec![PageRef { source: 0, key: "a.png".into() }]));
    let mut seen = Vec::new();
    let target = CommitTarget { series_id: "s", existing: &[], first_id: -1, now: 1 };
    commit(&book, &lib.files, &target, &mut |d, t| seen.push((d, t)), &CancelFlag::new()).unwrap();
    assert_eq!(seen.last(), Some(&(3, 3)));
    assert_eq!(shades_on_disk(&lib, "s", "c1"), [4, 4]);
    assert_eq!(shades_on_disk(&lib, "s", "c2"), [4]);
}

/// A PNG that says it is `w` x `h` and has nothing else of a picture.
fn png_header_only(w: u32, h: u32) -> Vec<u8> {
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend(crc32(&body).to_be_bytes());
        out
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &[0x78, 0x9c]));
    out
}

#[test]
fn a_page_the_converter_would_refuse_is_refused_at_import_with_the_same_limit() {
    assert_eq!(MAX_PAGE_PIXELS, 120_000_000);
    assert!(commit::accept(&png(1)));
    assert!(commit::accept(&png_header_only(10_000, 12_000)), "exactly 120 megapixels");
    assert!(!commit::accept(&png_header_only(10_000, 12_001)));
    assert!(!commit::accept(&png_header_only(20_000, 20_000)), "400 MP was accepted before");
    assert!(!commit::accept(b"GIF nonsense"));
    let src = temp("toobig");
    std::fs::write(src.join("big.png"), png_header_only(20_000, 20_000)).unwrap();
    std::fs::write(src.join("ok.png"), png(3)).unwrap();
    let lib = lib("toobig-lib");
    let outcome = commit_into(&one_chapter(&src, &["ok.png", "big.png"], Placement::New, 1.0), &lib, "s", &[]);
    assert_eq!(outcome.skipped_pages, 1);
    assert_eq!(outcome.chapters[0].pages, 1);
}

#[test]
fn leftover_staging_folders_are_swept_only_when_they_are_ours_and_their_process_is_gone() {
    let lib = lib("sweep");
    let series = lib.files.series_dir("s");
    let ours = series.join("_import-4000000-1");
    let alive = series.join("_import-4000001-2");
    let unmarked = series.join("_import-4000002-3");
    let chapter = series.join("c1");
    let other = series.join("notes");
    for dir in [&ours, &alive, &unmarked, &chapter, &other] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("00001.img"), b"x").unwrap();
    }
    std::fs::write(ours.join("import.partial"), "4000000").unwrap();
    std::fs::write(alive.join("import.partial"), "4000001").unwrap();
    // A chapter that was being replaced when the app stopped waits in the staging folder.
    std::fs::create_dir_all(ours.join("old-c9")).unwrap();
    std::fs::write(ours.join("old-c9").join("00001.img"), b"precious").unwrap();
    let running = |pid: u32| pid == 4_000_001;
    let removed = commit::sweep_with(&lib.files, &LibraryDb::default(), &running);
    assert_eq!(removed, 1);
    assert!(!ours.exists());
    assert!(alive.exists() && unmarked.exists() && chapter.exists() && other.exists());
    assert_eq!(std::fs::read(series.join("c9").join("00001.img")).unwrap(), b"precious", "put back");

    // A library entry with that very name protects it.
    std::fs::remove_dir_all(&alive).unwrap();
    std::fs::write(unmarked.join("import.partial"), "4000002").unwrap();
    let mut db = LibraryDb::default();
    db.record_chapter(
        "T",
        &Default::default(),
        DownloadedChapter { key: "_import-4000002-3".into(), ..Default::default() },
        1,
    );
    assert_eq!(commit::sweep_with(&lib.files, &db, &|_| false), 0);
    assert!(unmarked.exists());
    assert_eq!(commit::sweep_with(&lib.files, &LibraryDb::default(), &|_| false), 1);
    assert!(!unmarked.exists());
}

#[test]
fn the_same_book_is_recognised_by_its_file_and_a_changed_file_is_not() {
    let dir = temp("prints");
    let path = dir.join("Book - c1.cbz");
    write_zip(&path, &[("1.png", png(1))]);
    let book = scan(std::slice::from_ref(&path)).books.remove(0);
    let first = book_fingerprints(&book);
    assert_eq!(first.len(), 1);
    assert_eq!(book_fingerprints(&scan(std::slice::from_ref(&path)).books[0]), first);
    write_zip(&path, &[("1.png", png(1)), ("2.png", png(2))]);
    assert_ne!(book_fingerprints(&scan(std::slice::from_ref(&path)).books[0]), first);
    // Loose pictures cannot be told apart this way.
    let pics = pictures(&dir.join("pics"), &[("a.png", 1)]);
    let loose = scan(&[pics.join("a.png")]).books.remove(0);
    assert!(book_fingerprints(&loose).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Cancelling a read -----------------------------------------------------------

#[test]
fn a_read_that_is_cancelled_stops_and_reports_nothing_more() {
    let dir = temp("scan-cancel");
    let a = dir.join("A - c1.cbz");
    write_zip(&a, &[("1.png", png(1))]);
    let folder_path = pictures(&dir.join("F").join("Chapter 1"), &[("1.png", 1)]);
    let cancel = CancelFlag::new();
    cancel.cancel();
    let result = super::super::scan(&[a.clone(), folder_path, dir.join("missing.cbz")], &cancel);
    assert!(result.books.is_empty() && result.failures.is_empty());
    // Each reader notices it by itself.
    assert!(matches!(zip_in::scan(&a, &cancel), Err(PipelineError::Cancelled)));
    assert!(matches!(folder::scan_dir(&dir.join("F"), &cancel), Err(PipelineError::Cancelled)));
    // An archive inside a folder is not read once it is cancelled.
    let outer = dir.join("Outer");
    std::fs::create_dir_all(&outer).unwrap();
    write_zip(&outer.join("Ch 1.cbz"), &[("1.png", png(1))]);
    assert!(matches!(folder::scan_dir(&outer, &cancel), Err(PipelineError::Cancelled)));
    // Not cancelled, all of it reads.
    assert_eq!(scan(&[a]).books.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_from_a_7z_a_rar_or_a_folder_stops_at_the_next_entry_when_cancelled() {
    let dir = temp("visit-cancel");
    let cancel = CancelFlag::new();
    cancel.cancel();
    let keys = vec!["1.png".to_string(), "2.png".to_string()];
    let sevenz = dir.join("s.7z");
    sevenz_in::tests::make_7z(&sevenz, &[("1.png", png(1)), ("2.png", png(2))]);
    let r = sevenz_in::visit(&sevenz, &keys, &cancel, &mut |_, _| panic!("read after cancel"));
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    let rar = dir.join("r.rar");
    make_rar5(&rar, &[("1.png", png(1)), ("2.png", png(2))]);
    let r = rar_in::visit(&rar, &keys, &cancel, &mut |_, _| panic!("read after cancel"));
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    let zip = dir.join("z.zip");
    write_zip(&zip, &[("1.png", png(1)), ("2.png", png(2))]);
    let r = zip_in::visit(&zip, &keys, &cancel, &mut |_, _| panic!("read after cancel"));
    assert!(matches!(r, Err(PipelineError::Cancelled)));
    let pics = pictures(&dir.join("p"), &[("1.png", 1), ("2.png", 2)]);
    let r = folder::visit(&pics, &keys, &cancel, &mut |_, _| panic!("read after cancel"));
    assert!(matches!(r, Err(PipelineError::Cancelled)));

    // Cancelled while reading: no more entries are given after the flag is set.
    for which in ["rar", "7z"] {
        let cancel = CancelFlag::new();
        let mut given = 0;
        let mut each = |_: &str, _: Vec<u8>| {
            given += 1;
            cancel.cancel();
            Ok(())
        };
        let r = if which == "rar" {
            rar_in::visit(&rar, &keys, &cancel, &mut each)
        } else {
            sevenz_in::visit(&sevenz, &keys, &cancel, &mut each)
        };
        assert!(matches!(r, Err(PipelineError::Cancelled)) && given == 1, "{which}: {r:?} {given}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_an_archive_holds_twice_is_the_first_one_and_does_not_end_the_pass_early() {
    let dir = temp("dup");
    let rar = dir.join("d.cbr");
    make_rar5(&rar, &[("1.png", png(1)), ("1.png", png(2)), ("2.png", png(3))]);
    // The pass over the archive reaches 2.png although 1.png came twice.
    let mut got = Vec::new();
    rar_in::visit(&rar, &["1.png".into(), "2.png".into()], &CancelFlag::new(), &mut |k, bytes| {
        let shade = image::load_from_memory(&bytes).map(|i| i.to_luma8().get_pixel(0, 0).0[0]).unwrap_or(255);
        got.push((k.to_string(), shade));
        Ok(())
    })
    .unwrap();
    assert_eq!(got, [("1.png".to_string(), 1), ("2.png".to_string(), 3)]);
    // The book lists the page once, and says so.
    let book = scan(&[rar]).books.remove(0);
    assert_eq!(book.page_count(), 2);
    assert!(book.warnings.iter().any(|w| w.contains("same name")), "{:?}", book.warnings);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn many_pictures_of_one_tiff_are_read_through_one_opening() {
    let dir = temp("tiff-many");
    let multi = dir.join("many.tif");
    let shades: Vec<(u32, u32, u8)> = (0..40).map(|i| (8, 8, i as u8 * 5)).collect();
    tiff_in::tests::write_tiff(&multi, &shades);
    let book = scan(std::slice::from_ref(&multi)).books.remove(0);
    assert_eq!(book.page_count(), 40);
    let keys: Vec<String> = book.chapters[0].pages.iter().map(|p| p.key.clone()).collect();
    let mut got = Vec::new();
    folder::visit(&dir, &keys, &CancelFlag::new(), &mut |_, bytes| {
        got.push(image::load_from_memory(&bytes).unwrap().to_luma8().get_pixel(0, 0).0[0]);
        Ok(())
    })
    .unwrap();
    assert_eq!(got, (0..40).map(|i| i as u8 * 5).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Odd text -----------------------------------------------------------------------

#[test]
fn names_with_letters_that_change_length_when_lowercased_do_not_panic() {
    // 'İ' (2 bytes) lowercases to "i̇" (3 bytes), which moved every later offset.
    for name in ["İİİ Chapter 5", "İstanbul - c12", "ÀİÀ Vol 3", "İİİ 7", "İ"] {
        let _ = series_title(name);
        let _ = chapter_number(name);
    }
    assert_eq!(series_title("İİİ - Chapter 5"), "İİİ");
}

#[test]
fn a_chapter_number_that_is_not_a_finite_non_negative_number_is_ignored() {
    for bad in ["NaN", "inf", "-inf", "-3", "-0.5", "infinity"] {
        let xml = format!("<ComicInfo><Series>S</Series><Number>{bad}</Number></ComicInfo>");
        assert_eq!(comicinfo::parse(xml.as_bytes()).unwrap().number, None, "{bad}");
    }
    let xml = "<ComicInfo><Number>12.5</Number></ComicInfo>";
    assert_eq!(comicinfo::parse(xml.as_bytes()).unwrap().number, Some(12.5));
}

#[test]
fn a_library_files_value_is_all_that_a_sweep_needs() {
    // The sweep reads the roots of a LibraryFiles, the one new series go to first.
    let a = temp("roots-a");
    let b = temp("roots-b");
    let files = LibraryFiles::new(&a).with_other_roots([b.clone()]);
    assert_eq!(files.roots(), [a.as_path(), b.as_path()]);
}

#[test]
fn only_chapters_not_yet_decided_are_checked_against_the_series_again() {
    let have = [existing(1.0, "c1", -1), existing(2.0, "c2", -2)];
    let mut book = pages_book("more", &[(Some(1.0), "one", 1), (Some(2.0), "two", 1), (Some(3.0), "three", 1)]);
    // The person chose to keep both copies of chapter 2.
    book.chapters[1].meets = Some("c2".into());
    book.chapters[1].skip = false;
    book.meet_undecided(&have);
    assert!(book.chapters[0].skip && book.chapters[0].meets.as_deref() == Some("c1"));
    assert!(!book.chapters[1].skip, "a decision stays");
    assert!(!book.chapters[2].skip && book.chapters[2].meets.is_none());
}

#[test]
fn a_cover_is_saved_whole_and_its_folder_is_made() {
    let dir = temp("cover-save");
    let dest = dir.join("a").join("b").join("cover.img");
    save_cover(&dest, b"first").unwrap();
    save_cover(&dest, b"second").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1, "no temporary file left");
    // A folder where the file should go is an error, not a panic.
    let blocked = dir.join("blocked");
    std::fs::create_dir_all(blocked.join("cover.img")).unwrap();
    assert!(save_cover(&blocked.join("cover.img"), b"x").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
