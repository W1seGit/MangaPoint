//! Getting a finished file onto the reader.

use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::Disks;

/// Where a finished file goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExportTarget {
    /// A folder on this computer.
    Folder { path: PathBuf },
    /// A mounted reader or SD card; files go into `<mount>/<folder>/`.
    Device { mount: PathBuf, folder: String },
    /// The reader's WebDAV share (CrossPoint's Wi-Fi file transfer).
    WebDav { base_url: String, folder: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedFile {
    /// A path or URL the user can be shown.
    pub location: String,
    pub bytes: u64,
}

impl ExportTarget {
    pub async fn write(&self, filename: &str, data: &[u8]) -> Result<ExportedFile> {
        match self {
            ExportTarget::Folder { path } => write_file(path, filename, data, false),
            ExportTarget::Device { mount, folder } => {
                check_space(mount, data.len() as u64)?;
                write_file(&mount.join(sanitize_component(folder)), filename, data, true)
            }
            ExportTarget::WebDav { base_url, folder } => {
                upload_webdav(base_url, folder, filename, data.to_vec(), &CancelFlag::new()).await
            }
        }
    }
}

impl ExportTarget {
    /// Send a finished file that is already on disk. Large books are copied,
    /// never loaded into memory (Wi-Fi uploads are the exception, since they
    /// need the whole body).
    pub async fn write_file(&self, filename: &str, src: &Path) -> Result<ExportedFile> {
        self.write_file_with(filename, src, |_| {}, || {}, &CancelFlag::new()).await
    }

    /// As [`write_file`](Self::write_file), calling `progress` with how many
    /// bytes of the file have been stored so far, and `flushing` when a drive
    /// has the whole file and is being made to really store it.
    ///
    /// To a drive, what is reported is what the drive has accepted: it is made
    /// to store the file as it goes, so the numbers follow its real speed
    /// instead of racing ahead into the system's cache and then stalling.
    ///
    /// When `cancel` is set the copy stops at once, the half-written file is
    /// removed and [`PipelineError::Cancelled`] is returned.
    pub async fn write_file_with(
        &self,
        filename: &str,
        src: &Path,
        mut progress: impl FnMut(u64) + Send + 'static,
        flushing: impl FnOnce() + Send + 'static,
        cancel: &CancelFlag,
    ) -> Result<ExportedFile> {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let cancel = cancel.clone();
        let size = fs::metadata(src)
            .map_err(|e| PipelineError::Io(src.to_path_buf(), e))?
            .len();
        // Copying gigabytes takes minutes; it must not hold up the thread the
        // app's window runs on (Windows would call the app "not responding").
        let (filename_owned, src_owned) = (filename.to_string(), src.to_path_buf());
        match self {
            ExportTarget::Folder { path } => {
                let path = path.clone();
                blocking(move || copy_file(&path, &filename_owned, &src_owned, None, &mut progress, &cancel)).await
            }
            ExportTarget::Device { mount, folder } => {
                let mount = mount.clone();
                let dir = mount.join(sanitize_component(folder));
                blocking(move || {
                    // Listing the disks asks the system, which can be slow.
                    check_space(&mount, size)?;
                    copy_file(&dir, &filename_owned, &src_owned, Some(Box::new(flushing)), &mut progress, &cancel)
                })
                .await
            }
            ExportTarget::WebDav { base_url, folder } => {
                // (Not streamed: that needs reqwest's `stream` feature and so
                // another dependency; the file is read whole, as before.)
                let data = tokio::select! {
                    data = tokio::fs::read(src) => {
                        data.map_err(|e| PipelineError::Io(src.to_path_buf(), e))?
                    }
                    () = cancel.cancelled() => return Err(PipelineError::Cancelled),
                };
                let sent = upload_webdav(base_url, folder, filename, data, &cancel).await?;
                // The whole body goes in one request, so this is all there is to say.
                progress(sent.bytes);
                Ok(sent)
            }
        }
    }
}

/// Run slow file work on a thread of its own and wait for it.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| PipelineError::Upload(format!("the copy was interrupted: {e}")))?
}

/// Copy `src` into `dir` under a name that does not exist yet. Written to a
/// temporary name first, so a full disk never leaves a half-file that looks
/// complete.
///
/// With `flush`, the file is forced out to the drive before it gets its real
/// name, so that when this returns it is safe to unplug (a card or a reader
/// can otherwise still be holding the end of the file in memory).
fn copy_file(
    dir: &Path,
    filename: &str,
    src: &Path,
    flush: Option<Box<dyn FnOnce() + Send>>,
    progress: &mut dyn FnMut(u64),
    cancel: &CancelFlag,
) -> Result<ExportedFile> {
    fs::create_dir_all(dir).map_err(|e| PipelineError::Io(dir.to_path_buf(), e))?;
    let path = unique_path(dir, filename);
    let tmp = path.with_extension("part");
    let bytes = copy_in_chunks(src, &tmp, flush.is_some(), progress, cancel).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        if e.kind() == std::io::ErrorKind::Interrupted && cancel.is_cancelled() {
            PipelineError::Cancelled
        } else {
            PipelineError::Io(tmp.clone(), e)
        }
    })?;
    // From here on a failure must not leave the temporary file on the drive.
    let finish = || -> Result<()> {
        if let Some(flushing) = flush {
            flushing();
            flush_to_disk(&tmp)?;
        }
        fs::rename(&tmp, &path).map_err(|e| PipelineError::Io(path.clone(), e))
    };
    if let Err(e) = finish() {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(ExportedFile {
        location: path.display().to_string(),
        bytes,
    })
}

/// How much is read and written at a time.
const COPY_CHUNK: usize = 1 << 20;
/// How much a drive is given before it is made to store it (see `copy_in_chunks`).
const DRIVE_SYNC_EVERY: u64 = 8 << 20;

/// Copy `src` to `dst`, reporting how many bytes are stored after each chunk.
///
/// With `to_drive`, the drive is made to really store what it has every few
/// megabytes. Without that the system takes the whole file into its cache in
/// seconds and then makes the last step wait for the drive, which is slow, so
/// there would be nothing to report until the very end.
fn copy_in_chunks(
    src: &Path,
    dst: &Path,
    to_drive: bool,
    progress: &mut dyn FnMut(u64),
    cancel: &CancelFlag,
) -> std::io::Result<u64> {
    use std::io::{Read, Write};
    let mut from = fs::File::open(src)?;
    let mut to = fs::File::create(dst)?;
    let mut buffer = vec![0u8; COPY_CHUNK];
    let (mut written, mut synced) = (0u64, 0u64);
    loop {
        // Checked before every chunk, so a cancel stops the copy within one
        // chunk (and one drive sync), not at the end of the file.
        if cancel.is_cancelled() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = from.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        to.write_all(&buffer[..n])?;
        written += n as u64;
        if to_drive && written - synced >= DRIVE_SYNC_EVERY {
            to.sync_data()?;
            synced = written;
        }
        progress(written);
    }
    to.flush()?;
    progress(written);
    Ok(written)
}

/// Force a written file out to its drive.
fn flush_to_disk(path: &Path) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| PipelineError::Io(path.to_path_buf(), e))
}

fn write_file(dir: &Path, filename: &str, data: &[u8], flush: bool) -> Result<ExportedFile> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |e| PipelineError::Io(p, e)
    };
    fs::create_dir_all(dir).map_err(io(dir))?;
    let path = unique_path(dir, filename);
    let tmp = path.with_extension("part");
    fs::write(&tmp, data).map_err(io(&tmp))?;
    if flush {
        flush_to_disk(&tmp)?;
    }
    fs::rename(&tmp, &path).map_err(io(&path))?;
    Ok(ExportedFile {
        location: path.display().to_string(),
        bytes: data.len() as u64,
    })
}

/// `dir/name`, or `dir/name (2)` and so on if that file already exists, so
/// nothing is ever overwritten.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (filename, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

// ---- Drives ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub mount: PathBuf,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Is this mounted disk one a person plugs in? The system's "removable" flag
/// is right for flash drives and SD cards on Windows, but Linux marks many
/// card slots and USB disks as fixed, so the places where desktops mount
/// external media count as well.
fn looks_external(mount: &Path, removable: bool) -> bool {
    if removable {
        return true;
    }
    let m = mount.to_string_lossy();
    m.starts_with("/Volumes/") || m.starts_with("/media/") || m.starts_with("/run/media/")
}

/// Drives that look plugged in right now: SD cards, USB sticks, and a reader
/// in USB drive mode. It is a list of suggestions; the user can always pick
/// any folder instead.
pub fn removable_drives() -> Vec<Drive> {
    Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| looks_external(d.mount_point(), d.is_removable()))
        .map(|d| Drive {
            mount: d.mount_point().to_path_buf(),
            name: d.name().to_string_lossy().into_owned(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
        })
        .collect()
}

/// Fail early, with a clear message, if the card is too full. `mount` may be
/// the drive itself or any folder on it; a place we cannot match to a drive is
/// let through rather than blocked.
fn check_space(mount: &Path, needed: u64) -> Result<()> {
    let disks = Disks::new_with_refreshed_list();
    // The drive that holds the place: the deepest mount point it is under.
    let Some(disk) = disks
        .iter()
        .filter(|d| mount.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().components().count())
    else {
        return Ok(());
    };
    // Leave a little room for the filesystem.
    let needed_with_margin = needed + needed / 20;
    if disk.available_space() < needed_with_margin {
        return Err(PipelineError::NoSpace {
            mount: mount.to_path_buf(),
            needed: needed_with_margin,
            available: disk.available_space(),
        });
    }
    Ok(())
}

// ---- Names -----------------------------------------------------------------

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make `name` safe as one path component on Windows, macOS and Linux:
/// no separators or reserved characters, no trailing dots or spaces, not a
/// reserved device name, at most 100 characters.
pub fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(100).collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        return "manga".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

fn number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `Series - Ch 1-12.xtch`, `Series - Ch 5.xtch`, or with a part number
/// `Series - Part 2 - Ch 13-25.xtch`.
pub fn output_file_name(
    series: &str,
    first: Option<f64>,
    last: Option<f64>,
    part: Option<usize>,
    extension: &str,
) -> String {
    let mut name = sanitize_component(series);
    if let Some(p) = part {
        name.push_str(&format!(" - Part {p}"));
    }
    match (first, last) {
        (Some(a), Some(b)) if (a - b).abs() < 0.0005 => {
            name.push_str(&format!(" - Ch {}", number(a)))
        }
        (Some(a), Some(b)) => name.push_str(&format!(" - Ch {}-{}", number(a), number(b))),
        _ => {}
    }
    format!("{name}.{extension}")
}

// ---- WebDAV ----------------------------------------------------------------

/// Percent-encode one URL path segment.
fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Ask the reader's WebDAV share whether it is there. `Ok` means something
/// answered like a file share; `Err` is a sentence about what went wrong.
pub async fn check_webdav(base_url: &str) -> std::result::Result<(), String> {
    let base = base_url.trim();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("The address should start with http://".to_string());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let propfind = reqwest::Method::from_bytes(b"PROPFIND").expect("valid method");
    let response = client
        .request(propfind, format!("{}/", base.trim_end_matches('/')))
        .header("Depth", "0")
        .send()
        .await
        .map_err(|_| format!("Nothing answered at {base}. Is the reader on, on the same Wi-Fi, and in file transfer mode?"))?;
    let status = response.status();
    if status.is_success() || status.as_u16() == 207 {
        Ok(())
    } else {
        Err(format!("Something answered at {base}, but it said {status}."))
    }
}

/// The most a single file sent over Wi-Fi may be. The reader's web server has
/// to take the whole file in one request, and it is a small chip: one file of
/// 2.8 GB (more than a 32-bit size can say) took an X4 Pro down. Bigger books
/// are sent in parts of at most this size.
pub const WIFI_MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// How long a file of `bytes` may take to send: at least ten minutes, and
/// long enough for a slow link (about 100 KB a second).
fn upload_timeout(bytes: usize) -> Duration {
    Duration::from_secs(600.max(bytes as u64 / 100_000))
}

async fn upload_webdav(
    base_url: &str,
    folder: &str,
    filename: &str,
    data: Vec<u8>,
    cancel: &CancelFlag,
) -> Result<ExportedFile> {
    let fail = |m: String| PipelineError::Upload(m);
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(fail(format!("'{base_url}' is not an http address")));
    }
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(upload_timeout(data.len()))
        .build()
        .map_err(|e| fail(e.to_string()))?;
    let dir_url = format!(
        "{}/{}/",
        base_url.trim_end_matches('/'),
        encode_segment(&sanitize_component(folder))
    );
    let file_url = format!("{dir_url}{}", encode_segment(filename));
    let bytes = data.len() as u64;
    // Set once the file's request may have reached the reader.
    let put_started = std::sync::atomic::AtomicBool::new(false);
    let work = async {
        // Create the folder; a failure usually just means it already exists.
        let mkcol = reqwest::Method::from_bytes(b"MKCOL").expect("valid method");
        let _ = client.request(mkcol, &dir_url).send().await;
        put_started.store(true, std::sync::atomic::Ordering::SeqCst);
        client
            .put(&file_url)
            .body(data)
            .send()
            .await
            .map_err(|e| fail(e.to_string()))
    };
    // Cancel drops the request, which closes the connection.
    let response = tokio::select! {
        response = work => response?,
        () = cancel.cancelled() => {
            if put_started.load(std::sync::atomic::Ordering::SeqCst) {
                // The reader may be holding the front of the file. Ask it to
                // throw that away, in the background so Cancel is not delayed.
                let url = file_url.clone();
                tokio::spawn(async move {
                    let quick = reqwest::Client::builder()
                        .timeout(Duration::from_secs(5))
                        .build();
                    if let Ok(quick) = quick {
                        let _ = quick.delete(&url).send().await;
                    }
                });
            }
            return Err(PipelineError::Cancelled);
        }
    };
    let status = response.status();
    if status.is_success() {
        Ok(ExportedFile {
            location: file_url,
            bytes,
        })
    } else {
        Err(fail(format!("the reader answered {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::xtc::Format;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn temp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("mangapoint-export-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_slow_link_gets_more_time_for_a_big_file() {
        assert_eq!(upload_timeout(1_000), Duration::from_secs(600));
        assert_eq!(upload_timeout(512 * 1024 * 1024), Duration::from_secs(5368));
    }

    #[test]
    fn names_are_safe_on_every_platform() {
        assert_eq!(
            sanitize_component("Re:Zero / Season 1?"),
            "Re_Zero _ Season 1_"
        );
        assert_eq!(sanitize_component("  spaced   out  "), "spaced out");
        assert_eq!(sanitize_component("trailing..."), "trailing");
        assert_eq!(sanitize_component("CON"), "CON_");
        assert_eq!(sanitize_component("nul.txt"), "nul.txt_");
        assert_eq!(sanitize_component(""), "manga");
        assert_eq!(sanitize_component("..."), "manga");
        assert_eq!(sanitize_component("進撃の巨人"), "進撃の巨人");
        assert_eq!(sanitize_component(&"x".repeat(300)).chars().count(), 100);
        assert!(!sanitize_component("../../etc/passwd").contains('/'));
    }

    #[test]
    fn file_names_describe_the_chapters() {
        let n = |a, b, p| output_file_name("Berserk", a, b, p, Format::Xtch.extension());
        assert_eq!(n(Some(1.0), Some(12.0), None), "Berserk - Ch 1-12.xtch");
        assert_eq!(n(Some(5.0), Some(5.0), None), "Berserk - Ch 5.xtch");
        assert_eq!(
            n(Some(5.5), Some(6.0), Some(2)),
            "Berserk - Part 2 - Ch 5.5-6.xtch"
        );
        assert_eq!(n(None, None, None), "Berserk.xtch");
        assert_eq!(
            output_file_name("A/B", Some(1.0), Some(2.0), None, Format::Xtc.extension()),
            "A_B - Ch 1-2.xtc"
        );
    }

    #[tokio::test]
    async fn folder_export_writes_and_never_overwrites() {
        let dir = temp("folder");
        let target = ExportTarget::Folder {
            path: dir.join("out"),
        };
        let a = target.write("Book.xtch", b"one").await.unwrap();
        let b = target.write("Book.xtch", b"two").await.unwrap();
        let c = target.write("Book.xtch", b"three").await.unwrap();
        assert!(a.location.ends_with("Book.xtch"));
        assert!(b.location.ends_with("Book (2).xtch"), "{}", b.location);
        assert!(c.location.ends_with("Book (3).xtch"));
        assert_eq!(fs::read(&a.location).unwrap(), b"one");
        assert_eq!(fs::read(&b.location).unwrap(), b"two");
        assert!(fs::read_dir(dir.join("out"))
            .unwrap()
            .all(|e| { e.unwrap().path().extension().is_some_and(|x| x != "part") }));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn device_export_goes_into_a_subfolder_with_a_safe_name() {
        let dir = temp("device");
        fs::create_dir_all(&dir).unwrap();
        let target = ExportTarget::Device {
            mount: dir.clone(),
            folder: "Manga/../Evil".into(),
        };
        let f = target.write("Book.xtc", b"data").await.unwrap();
        let path = PathBuf::from(&f.location);
        assert!(path.starts_with(&dir), "stays inside the card: {path:?}");
        assert_eq!(path.parent().unwrap().parent().unwrap(), dir.as_path());
        assert_eq!(fs::read(&path).unwrap(), b"data");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unique_path_handles_names_without_extensions() {
        let dir = temp("unique");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plain"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "plain"), dir.join("plain (2)"));
        assert_eq!(unique_path(&dir, "fresh.xtc"), dir.join("fresh.xtc"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn listing_drives_does_not_fail() {
        // Whatever is mounted on this machine, it must not panic.
        for d in removable_drives() {
            assert!(d.available_bytes <= d.total_bytes);
        }
    }

    #[test]
    fn a_mount_we_cannot_find_is_not_blocked() {
        // A relative path is under no mount, whatever the system's disks are
        // (an absolute one would sit under `/` on macOS and Linux).
        assert!(check_space(Path::new("definitely-not-mounted"), u64::MAX / 4).is_ok());
    }

    #[test]
    fn url_segments_are_encoded() {
        assert_eq!(
            encode_segment("Berserk - Ch 1-12.xtch"),
            "Berserk%20-%20Ch%201-12.xtch"
        );
        assert_eq!(encode_segment("a/b"), "a%2Fb");
        assert_eq!(encode_segment("進"), "%E9%80%B2");
    }

    /// Each request the fake server saw: its first line and its body.
    type Seen = Vec<(String, Vec<u8>)>;

    /// A one-shot HTTP server that records each request and replies `status`.
    fn fake_server(
        status: &'static str,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, content_length) = loop {
                    let n = stream.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (pos + 4, len);
                    }
                    if n == 0 {
                        break (buf.len(), 0);
                    }
                };
                while buf.len() < head_end + content_length {
                    let n = stream.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let request_line = String::from_utf8_lossy(&buf)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                seen.push((request_line, buf[head_end..].to_vec()));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
            seen
        });
        (url, handle)
    }

    #[tokio::test]
    async fn webdav_creates_the_folder_then_puts_the_file() {
        let (url, server) = fake_server("201 Created", 2);
        let target = ExportTarget::WebDav {
            base_url: url.clone(),
            folder: "Manga".into(),
        };
        let f = target.write("My Book.xtch", b"payload").await.unwrap();
        let seen = server.join().unwrap();
        assert!(seen[0].0.starts_with("MKCOL /Manga/ "), "{:?}", seen[0]);
        assert!(
            seen[1].0.starts_with("PUT /Manga/My%20Book.xtch "),
            "{:?}",
            seen[1]
        );
        assert_eq!(seen[1].1, b"payload");
        assert_eq!(f.location, format!("{url}/Manga/My%20Book.xtch"));
    }

    #[tokio::test]
    async fn webdav_errors_are_reported() {
        let (url, server) = fake_server("507 Insufficient Storage", 2);
        let target = ExportTarget::WebDav {
            base_url: url,
            folder: "Manga".into(),
        };
        let err = target.write("b.xtc", b"x").await.unwrap_err();
        server.join().unwrap();
        assert!(matches!(err, PipelineError::Upload(m) if m.contains("507")));

        let err = ExportTarget::WebDav {
            base_url: "ftp://reader".into(),
            folder: "m".into(),
        }
        .write("b.xtc", b"x")
        .await
        .unwrap_err();
        assert!(matches!(err, PipelineError::Upload(_)));
    }

    #[test]
    fn places_where_desktops_mount_external_media_count_as_drives() {
        assert!(looks_external(Path::new("E:\\"), true));
        assert!(looks_external(Path::new("/Volumes/XTEINK"), false));
        assert!(looks_external(Path::new("/media/me/XTEINK"), false));
        assert!(looks_external(Path::new("/run/media/me/XTEINK"), false));
        assert!(!looks_external(Path::new("/"), false));
        assert!(!looks_external(Path::new("/home"), false));
        assert!(!looks_external(Path::new("C:\\"), false));
    }

    #[tokio::test]
    async fn a_file_sent_to_a_drive_is_flushed_and_lands_in_its_folder() {
        let mount = temp("device-flush");
        fs::create_dir_all(&mount).unwrap();
        let src = mount.join("book.xtc");
        fs::write(&src, vec![7u8; 4096]).unwrap();
        let target = ExportTarget::Device {
            mount: mount.clone(),
            folder: "Manga".into(),
        };
        let sent = target.write_file("My Book.xtc", &src).await.unwrap();
        assert!(sent.location.ends_with("My Book.xtc"));
        assert_eq!(fs::read(mount.join("Manga").join("My Book.xtc")).unwrap().len(), 4096);
        assert!(!mount.join("Manga").join("My Book.part").exists(), "no half-written file is left");
        let _ = fs::remove_dir_all(mount);
    }

    #[tokio::test]
    async fn the_bytes_stored_are_reported_as_a_file_is_sent() {
        let mount = temp("device-progress");
        fs::create_dir_all(&mount).unwrap();
        let src = mount.join("big.xtc");
        let size = 20 * 1024 * 1024 + 123;
        fs::write(&src, vec![1u8; size]).unwrap();
        let target = ExportTarget::Device {
            mount: mount.clone(),
            folder: "Manga".into(),
        };
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let sent = target
            .write_file_with("Big.xtc", &src, move |n| log.lock().unwrap().push(n), || {}, &CancelFlag::new())
            .await
            .unwrap();
        assert_eq!(sent.bytes, size as u64);
        let seen = seen.lock().unwrap();
        assert!(seen.len() > 5, "it is reported as it goes, not once at the end");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "never goes backwards");
        assert_eq!(*seen.last().unwrap(), size as u64);
        let _ = fs::remove_dir_all(mount);
    }

    #[tokio::test]
    async fn cancelling_stops_a_copy_to_a_drive_and_leaves_no_file() {
        let mount = temp("device-cancel");
        fs::create_dir_all(&mount).unwrap();
        let src = mount.join("big.xtc");
        let size = 64 * 1024 * 1024;
        fs::write(&src, vec![1u8; size]).unwrap();
        let target = ExportTarget::Device {
            mount: mount.clone(),
            folder: "Manga".into(),
        };
        let cancel = CancelFlag::new();
        let (flag, stored) = (cancel.clone(), std::sync::Arc::new(std::sync::Mutex::new(0u64)));
        let seen = stored.clone();
        let result = target
            .write_file_with(
                "Big.xtc",
                &src,
                move |n| {
                    *seen.lock().unwrap() = n;
                    // Cancel when about a quarter of it is stored.
                    if n >= size as u64 / 4 {
                        flag.cancel();
                    }
                },
                || {},
                &cancel,
            )
            .await;
        assert!(matches!(result, Err(PipelineError::Cancelled)), "{result:?}");
        let stored = *stored.lock().unwrap();
        assert!(stored < size as u64 / 2, "it kept writing after the cancel: {stored} of {size} bytes");
        let leftovers: Vec<_> = fs::read_dir(mount.join("Manga")).map(|d| d.flatten().collect()).unwrap_or_default();
        assert!(leftovers.is_empty(), "files left behind: {leftovers:?}");
        let _ = fs::remove_dir_all(mount);
    }

    #[tokio::test]
    async fn a_copy_that_was_already_cancelled_does_not_start() {
        let mount = temp("device-cancelled-first");
        fs::create_dir_all(&mount).unwrap();
        let src = mount.join("a.xtc");
        fs::write(&src, b"data").unwrap();
        let target = ExportTarget::Folder { path: mount.join("out") };
        let cancel = CancelFlag::new();
        cancel.cancel();
        let result = target.write_file_with("a.xtc", &src, |_| {}, || {}, &cancel).await;
        assert!(matches!(result, Err(PipelineError::Cancelled)));
        assert!(!mount.join("out").exists(), "nothing is created for a cancelled copy");
        let _ = fs::remove_dir_all(mount);
    }

    /// A one-answer server for a test: replies with `status_line` to the first
    /// request it gets.
    fn answer_once(status_line: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = write!(stream, "HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn the_reader_check_tells_an_answering_share_from_everything_else() {
        assert!(check_webdav(&answer_once("207 Multi-Status")).await.is_ok());
        let refused = check_webdav(&answer_once("401 Unauthorized")).await.unwrap_err();
        assert!(refused.contains("401"), "{refused}");
        assert!(check_webdav("192.168.1.5").await.unwrap_err().contains("http://"));
        // Nothing listening at all.
        let dead = check_webdav("http://127.0.0.1:9").await.unwrap_err();
        assert!(dead.contains("Nothing answered"), "{dead}");
    }

    /// A reader that answers MKCOL and DELETE at once but sits on a PUT for
    /// `stall`. Returns its address and the request lines it saw.
    fn stalling_reader(
        stall: Duration,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let line = String::from_utf8_lossy(&buf[..n])
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let put = line.starts_with("PUT");
                    log.lock().unwrap().push(line);
                    if put {
                        std::thread::sleep(stall);
                    }
                    let _ = write!(
                        stream,
                        "HTTP/1.1 201 Created
Content-Length: 0
Connection: close

"
                    );
                });
            }
        });
        (url, seen)
    }

    #[tokio::test]
    async fn cancelling_a_wifi_upload_returns_at_once_and_cleans_up_the_reader() {
        let dir = temp("webdav-cancel");
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("b.xtc");
        fs::write(&src, vec![3u8; 1000]).unwrap();
        let (url, seen) = stalling_reader(Duration::from_secs(4));
        let target = ExportTarget::WebDav { base_url: url, folder: "Manga".into() };
        let cancel = CancelFlag::new();
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            flag.cancel();
        });
        let started = std::time::Instant::now();
        let result = target.write_file_with("b.xtc", &src, |_| {}, || {}, &cancel).await;
        assert!(matches!(result, Err(PipelineError::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        // The half-sent file is deleted from the reader, in the background.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let seen = seen.lock().unwrap();
        assert!(seen.iter().any(|l| l.starts_with("DELETE /Manga/b.xtc ")), "{seen:?}");
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_final_step_removes_the_temporary_file_from_the_drive() {
        let mount = temp("device-rename-fails");
        fs::create_dir_all(&mount).unwrap();
        let src = mount.join("a.xtc");
        fs::write(&src, b"data").unwrap();
        let target = ExportTarget::Device { mount: mount.clone(), folder: "Manga".into() };
        // Between the copy and the rename, put a folder where the file should go.
        let blocker = mount.join("Manga").join("a.xtc");
        let result = target
            .write_file_with("a.xtc", &src, |_| {}, move || fs::create_dir_all(&blocker).unwrap(), &CancelFlag::new())
            .await;
        assert!(result.is_err());
        let parts: Vec<_> = fs::read_dir(mount.join("Manga"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .collect();
        assert!(parts.is_empty(), "left behind: {parts:?}");
        let _ = fs::remove_dir_all(mount);
    }
}
