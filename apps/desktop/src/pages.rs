//! Downloaded pages, handed to the web view by address.
//!
//! The reader shows pages as ordinary pictures whose address is
//! `page://localhost/<series>/<chapter>/<file>` (`http://page.localhost/...` on
//! Windows). A page is never read into the app's memory and pushed to the web
//! view as text: the view asks for the file and gets it straight from disk.
//!
//! Anything that can ask is treated as hostile. A request is answered only if
//! the series and the chapter are folders the library really has, the file is
//! one of the chapter's saved pages, and, once symbolic links are resolved, it
//! is inside the library. Everything else is a 404.

use dioxus::desktop::wry::http::{Method, Request, Response};
use dioxus::desktop::RequestAsyncResponder;
use library::LibraryFiles;
use std::borrow::Cow;
use std::path::PathBuf;

/// The name the web view knows this by.
pub const SCHEME: &str = "page";

/// The longest request path we look at, and the longest name in it.
const MAX_PATH: usize = 600;
const MAX_NAME: usize = 128;

/// Where the web view finds pages.
fn base() -> &'static str {
    if cfg!(target_os = "windows") {
        "http://page.localhost"
    } else {
        "page://localhost"
    }
}

/// The address of one saved page. `version` changes when the chapter's pages
/// are replaced (a better download), so the web view asks for them again.
pub fn url(series_id: &str, chapter_key: &str, file: &str, version: &str) -> String {
    format!("{}/{series_id}/{chapter_key}/{file}?v={version}", base())
}

/// Percent-decode a path segment once. `None` for a bad escape or anything
/// that is not text.
fn decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Names Windows keeps for devices, which must not be used as folder or file
/// names (with or without an extension).
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

/// A series id or chapter key: what the library makes them from, and nothing
/// else. No separators, no dots, no drive letters, no escapes.
fn is_safe_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !is_reserved(name)
}

/// A saved page's file name: `00001.img`.
fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !name.starts_with('.')
        && !is_reserved(name)
}

/// The file a request path stands for, if it is a saved page of a chapter the
/// library has. `path` is the path of the address, such as
/// `/solo-leveling-3fa9c2e1/c12/00003.img`.
pub fn resolve(files: &LibraryFiles, path: &str) -> Option<PathBuf> {
    if path.len() > MAX_PATH || !path.starts_with('/') {
        return None;
    }
    let mut parts = path[1..].split('/');
    let (series, chapter, file) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    // Decoded once: an escaped separator or dot decodes to something the
    // checks below refuse, and a doubly escaped one still holds a `%`.
    let (series, chapter, file) = (decode(series)?, decode(chapter)?, decode(file)?);
    if !is_safe_folder_name(&series) || !is_safe_folder_name(&chapter) || !is_safe_file_name(&file) {
        return None;
    }
    // The series and the chapter must be real folders of the library (not links).
    let series_dir = files.series_dir(&series);
    let chapter_dir = files.chapter_dir(&series, &chapter);
    let is_real_dir = |p: &std::path::Path| {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    };
    if !is_real_dir(&series_dir) || !is_real_dir(&chapter_dir) {
        return None;
    }
    // And the file one of its saved pages.
    let wanted = files
        .page_files(&series, &chapter)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == file.as_str()))?;
    // Links are resolved last: whatever the path says, the file must really be
    // inside a library folder.
    let real = std::fs::canonicalize(&wanted).ok()?;
    let inside = files
        .roots()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| real.starts_with(&root));
    inside.then_some(real)
}

/// What kind of picture this is, from its first bytes. `None` for anything else.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

fn reply(status: u16, mime: &str, cache: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", cache)
        .header("X-Content-Type-Options", "nosniff")
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}

fn refuse(status: u16) -> Response<Cow<'static, [u8]>> {
    reply(status, "text/plain", "no-store", Vec::new())
}

/// The answer to a request for `path`, read from the library. Blocking.
pub fn answer(files: &LibraryFiles, method: &Method, path: &str) -> Response<Cow<'static, [u8]>> {
    if method != Method::GET {
        return refuse(405);
    }
    let Some(file) = resolve(files, path) else {
        return refuse(404);
    };
    let Ok(bytes) = std::fs::read(&file) else {
        return refuse(404);
    };
    match sniff(&bytes) {
        // A page's address changes when its chapter is downloaded again (see
        // `url`), so a page can be kept as long as the browser likes.
        Some(mime) => reply(200, mime, "private, max-age=31536000, immutable", bytes),
        None => refuse(415),
    }
}

/// Answer the web view's request for a page.
pub fn serve(request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        responder.respond(refuse(503));
        return;
    };
    runtime.spawn(async move {
        // Reading a file waits on the disk, which must not hold up the window.
        let response = tokio::task::spawn_blocking(move || {
            answer(&crate::state::library_files(), &method, &path)
        })
        .await
        .unwrap_or_else(|_| refuse(500));
        responder.respond(response);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SERIES: &str = "solo-3fa9c2e1";

    /// A library with one series and one chapter of two pages, and a secret
    /// file next to it that must never be served.
    fn fixture(tag: &str) -> (PathBuf, LibraryFiles) {
        let root = std::env::temp_dir().join(format!("mangapoint-pages-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let library = root.join("library");
        let chapter = library.join(SERIES).join("c1");
        fs::create_dir_all(&chapter).unwrap();
        fs::write(chapter.join("00001.img"), [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]).unwrap();
        fs::write(chapter.join("00002.img"), b"\x89PNG\r\n\x1a\nxxxx").unwrap();
        fs::write(chapter.join("source.txt"), "s:1").unwrap();
        fs::write(root.join("secret.txt"), "top secret").unwrap();
        fs::write(library.join("secret.img"), "top secret").unwrap();
        (root.clone(), LibraryFiles::new(library))
    }

    #[test]
    fn a_saved_page_is_found_and_only_a_saved_page() {
        let (root, files) = fixture("ok");
        let found = resolve(&files, &format!("/{SERIES}/c1/00001.img")).unwrap();
        assert!(found.ends_with("00001.img"));
        assert!(found.starts_with(fs::canonicalize(root.join("library")).unwrap()));
        // Other files in the chapter folder are not pages.
        assert!(resolve(&files, &format!("/{SERIES}/c1/source.txt")).is_none());
        assert!(resolve(&files, &format!("/{SERIES}/c1/00001%2eimg")).is_some(), "an escaped dot is decoded once, and is then the real name");
        assert!(resolve(&files, &format!("/{SERIES}/c1/00009.img")).is_none());
        assert!(resolve(&files, &format!("/{SERIES}/c2/00001.img")).is_none());
        assert!(resolve(&files, "/other-1/c1/00001.img").is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hostile_paths_are_refused() {
        let (root, files) = fixture("hostile");
        let long = "a".repeat(5000);
        let huge_key = format!("/{SERIES}/{}/00001.img", "c".repeat(200));
        let hostile = [
            // Climbing out.
            "/../secret.img".to_string(),
            format!("/{SERIES}/../secret.img"),
            format!("/{SERIES}/c1/../../secret.img"),
            format!("/{SERIES}/c1/..%2f..%2fsecret.img"),
            format!("/{SERIES}/..%2fsecret.img/00001.img"),
            "/%2e%2e/c1/00001.img".to_string(),
            format!("/{SERIES}/%2e%2e/00001.img"),
            format!("/{SERIES}/c1/%2e%2e%2f%2e%2e%2fsecret.txt"),
            format!("/{SERIES}/c1/%2E%2E"),
            // Separators, escaped and not, and double escapes.
            format!("/{SERIES}%2Fc1/00001.img"),
            format!("/{SERIES}/c1%5C00001.img"),
            format!("/{SERIES}/c1\\00001.img"),
            format!("/{SERIES}/c1/%252e%252e%252fsecret.img"),
            // Absolute paths, drives, UNC paths and URLs.
            "//etc/passwd".to_string(),
            "/C:/Windows/win.ini".to_string(),
            "/C%3A/Windows/win.ini".to_string(),
            "/\\\\server\\share\\x/c1/00001.img".to_string(),
            "/%5C%5Cserver%5Cshare/c1/00001.img".to_string(),
            "/file:///etc/passwd/c1/x.img".to_string(),
            format!("/{SERIES}/c1/C:00001.img"),
            format!("/{SERIES}/c1/00001.img:stream"),
            // Device names.
            format!("/{SERIES}/c1/NUL"),
            format!("/{SERIES}/c1/nul.img"),
            format!("/{SERIES}/c1/COM1.img"),
            "/CON/c1/00001.img".to_string(),
            format!("/{SERIES}/LPT9/00001.img"),
            // Shape.
            String::new(),
            "/".to_string(),
            format!("/{SERIES}"),
            format!("/{SERIES}/c1"),
            format!("/{SERIES}/c1/"),
            format!("/{SERIES}/c1/00001.img/extra"),
            format!("{SERIES}/c1/00001.img"),
            format!("/{SERIES}//00001.img"),
            format!("/{SERIES}/c1/.hidden"),
            format!("/{SERIES}/c1/00001.img\0"),
            format!("/{SERIES}/c1/%00"),
            format!("/{SERIES}/c1/%zz"),
            format!("/{SERIES}/c1/%c0%af"),
            format!("/{SERIES}/c1/%"),
            // Size.
            huge_key,
            format!("/{long}/c1/00001.img"),
            format!("/{SERIES}/c1/{long}"),
        ];
        for path in &hostile {
            assert!(resolve(&files, path).is_none(), "served {path:?}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_get_is_answered_and_the_type_comes_from_the_bytes() {
        let (root, files) = fixture("answer");
        let page = format!("/{SERIES}/c1/00001.img");
        let ok = answer(&files, &Method::GET, &page);
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.headers()["Content-Type"], "image/jpeg");
        assert!(ok.headers()["Cache-Control"].to_str().unwrap().contains("max-age"));
        assert_eq!(ok.body().as_ref(), &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]);
        let png = answer(&files, &Method::GET, &format!("/{SERIES}/c1/00002.img"));
        assert_eq!(png.headers()["Content-Type"], "image/png");
        for method in [Method::POST, Method::PUT, Method::DELETE, Method::HEAD, Method::OPTIONS] {
            assert_eq!(answer(&files, &method, &page).status(), 405, "{method}");
        }
        assert_eq!(answer(&files, &Method::GET, "/../secret.txt").status(), 404);
        // A page that is not a picture is not handed over as one.
        fs::write(files.page_path(SERIES, "c1", 2), b"<script>alert(1)</script>").unwrap();
        assert_eq!(answer(&files, &Method::GET, &format!("/{SERIES}/c1/00003.img")).status(), 415);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn types_are_sniffed() {
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xdb]), Some("image/jpeg"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"GIF89a...."), Some("image/gif"));
        assert_eq!(sniff(b"<html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn addresses_are_built_the_way_they_are_read() {
        let address = url(SERIES, "c1", "00001.img", "7");
        assert!(address.ends_with(&format!("/{SERIES}/c1/00001.img?v=7")), "{address}");
        let path = address.split_once("localhost").map(|(_, p)| p).unwrap_or("");
        let path = path.split('?').next().unwrap();
        let (root, files) = fixture("roundtrip");
        assert!(resolve(&files, path).is_some(), "{path}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_library_is_not_followed() {
        let (root, files) = fixture("link");
        let chapter = root.join("library").join(SERIES).join("c1");
        std::os::unix::fs::symlink(root.join("secret.txt"), chapter.join("00007.img")).unwrap();
        assert!(resolve(&files, &format!("/{SERIES}/c1/00007.img")).is_none());
        std::os::unix::fs::symlink(&chapter, root.join("library").join(SERIES).join("c9")).unwrap();
        assert!(resolve(&files, &format!("/{SERIES}/c9/00001.img")).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
