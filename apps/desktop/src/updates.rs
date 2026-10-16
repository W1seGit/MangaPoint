//! Knowing whether a newer MangaPoint exists, and getting it.
//!
//! The release list is GitHub's "latest release" of this project. Looking is
//! read-only and quiet: a few seconds' wait, and any failure (offline, a busy
//! GitHub) just means nothing is offered.
//!
//! Getting a new version never touches the library, the settings or the
//! catalog engine, which live in their own folder. What is done with the
//! download depends on how this copy was installed:
//!
//! * **Windows, installed with the setup program:** the new setup is run
//!   silently, which closes this copy, replaces it and opens the new one.
//! * **Everything else:** the file is downloaded and its checksum checked, and
//!   then handed over (the disk image is opened, the folder is shown, or the
//!   install command is given), because those cannot be replaced safely from
//!   inside the running program.
//!
//! The one file downloaded lives in the app's own `update` folder, and that
//! folder is emptied the next time the app starts.

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// The version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_URL: &str = "https://api.github.com/repos/W1seGit/MangaPoint/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/W1seGit/MangaPoint/releases/latest";

/// Where the check for a newer version has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    /// Not looked yet.
    Idle,
    /// Looking.
    Checking,
    /// This is the newest.
    UpToDate,
    /// A newer one is there.
    Available { version: String },
    /// It did not work; this says why.
    Failed(String),
}

/// How a download is used once it is here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    /// Run the installer silently: it replaces this copy and opens the new one.
    Replace,
    /// Open the file (a disk image on macOS).
    Open,
    /// Show where the file is (a portable program, a package on Linux).
    Reveal,
    /// Nothing for this system is in the release: send the person to its page.
    Page,
}

/// One file of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// A newer version, as shown to the person.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    /// A few short lines taken from the release notes.
    pub summary: Vec<String>,
    /// The release's page.
    pub page: String,
    /// The file for this system, and the list of checksums it is checked against.
    pub asset: Option<Asset>,
    pub sums: Option<Asset>,
    pub apply: Apply,
}

// ---- versions ------------------------------------------------------------

/// `1.2.3` or `v1.2.3` as numbers. Anything with a suffix (`1.2.3-beta`) is a
/// pre-release and is never offered.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let v = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(v)
}

/// Whether `candidate` is a newer version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

// ---- release notes ---------------------------------------------------------

const SUMMARY_LINES: usize = 5;
const SUMMARY_WIDTH: usize = 96;

/// A short summary of a release's notes: its first few bullet points, plain.
/// Headings, links and code marks are dropped, and long lines cut at a word.
pub fn summarize(notes: &str) -> Vec<String> {
    let clean = |line: &str| -> String {
        let mut s = line.trim().trim_start_matches(['-', '*', '+']).trim().to_string();
        // [text](url) -> text
        while let (Some(a), Some(b)) = (s.find('['), s.find("](")) {
            if b < a {
                break;
            }
            let Some(end) = s[b..].find(')') else { break };
            let text = s[a + 1..b].to_string();
            s.replace_range(a..b + end + 1, &text);
        }
        s = s.replace(['`', '*', '_'], "");
        let s = s.trim();
        if s.chars().count() <= SUMMARY_WIDTH {
            return s.to_string();
        }
        let mut cut: String = s.chars().take(SUMMARY_WIDTH).collect();
        if let Some(i) = cut.rfind(' ') {
            cut.truncate(i);
        }
        format!("{}\u{2026}", cut.trim_end_matches(['.', ',', ';', ':', ' ']))
    };
    let bullets: Vec<String> = notes
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") || t.starts_with("* ")
        })
        .map(clean)
        .filter(|l| !l.is_empty())
        .take(SUMMARY_LINES)
        .collect();
    if !bullets.is_empty() {
        return bullets;
    }
    notes
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(clean)
        .filter(|l| !l.is_empty())
        .take(3)
        .collect()
}

// ---- which file ------------------------------------------------------------

/// Whether this copy was put in place by the Windows setup program (which
/// leaves its uninstaller beside the program) and not just copied.
pub fn installed_with_setup() -> bool {
    cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("unins000.exe").is_file()))
            .unwrap_or(false)
}

/// The file of a release for this system, and what to do with it.
pub fn choose_asset(names: &[String], version: &str) -> Option<(String, Apply)> {
    let want = |suffix: &str| {
        let name = format!("MangaPoint-{version}-{suffix}");
        names.iter().find(|n| **n == name).cloned()
    };
    let arch = std::env::consts::ARCH;
    if cfg!(windows) {
        if installed_with_setup() {
            return want("windows-x86_64-setup.exe").map(|n| (n, Apply::Replace));
        }
        return want("windows-x86_64-portable.exe").map(|n| (n, Apply::Reveal));
    }
    if cfg!(target_os = "macos") {
        let suffix = if arch == "aarch64" { "macos-arm64.dmg" } else { "macos-x86_64.dmg" };
        return want(suffix).map(|n| (n, Apply::Open));
    }
    // Linux: a package where this program came from one, else the bare program.
    let packaged = std::env::current_exe().is_ok_and(|p| p.starts_with("/usr"));
    if packaged {
        let deb = std::path::Path::new("/var/lib/dpkg").is_dir();
        return want(if deb { "linux-x86_64.deb" } else { "linux-x86_64.rpm" }).map(|n| (n, Apply::Reveal));
    }
    want("linux-x86_64").map(|n| (n, Apply::Reveal))
}

/// What to type to install a downloaded package, where that is the way.
pub fn install_command(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let dir = path.parent()?.display();
    if name.ends_with(".deb") {
        Some(format!("sudo apt install \"{dir}/{name}\""))
    } else if name.ends_with(".rpm") {
        Some(format!("sudo dnf install \"{dir}/{name}\""))
    } else {
        None
    }
}

// ---- looking ---------------------------------------------------------------

/// `MANGAPOINT_UPDATE_URL` points the check at another place (a test server).
fn source_url() -> (String, bool) {
    match std::env::var("MANGAPOINT_UPDATE_URL") {
        Ok(url) if !url.trim().is_empty() => (url, true),
        _ => (DEFAULT_URL.to_string(), false),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("MangaPoint/{VERSION}"))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(serde::Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Turn a release into an [`Update`], if it is newer than `current` and a
/// proper release.
fn update_from(release: Release, current: &str) -> Option<Update> {
    if release.draft || release.prerelease {
        return None;
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    if !is_newer(&version, current) {
        return None;
    }
    let names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
    let find = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| Asset { name: a.name.clone(), url: a.browser_download_url.clone() })
    };
    let (asset, apply) = match choose_asset(&names, &version) {
        Some((name, apply)) => (find(&name), apply),
        None => (None, Apply::Page),
    };
    let apply = if asset.is_some() { apply } else { Apply::Page };
    Some(Update {
        summary: summarize(&release.body),
        page: if release.html_url.is_empty() { RELEASES_PAGE.to_string() } else { release.html_url },
        sums: find("SHA256SUMS.txt"),
        asset,
        apply,
        version,
    })
}

/// Look for a newer version: `Some(update)` if there is one.
pub async fn check() -> Result<Option<Update>, String> {
    let (url, _) = source_url();
    let resp = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "Could not reach GitHub to look for a newer version.".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {} when looking for a newer version.", resp.status().as_u16()));
    }
    let release: Release = resp
        .json()
        .await
        .map_err(|_| "GitHub's answer about the newest version could not be read.".to_string())?;
    Ok(update_from(release, VERSION))
}

// ---- getting it ------------------------------------------------------------

/// The folder the one download lives in.
pub fn download_dir() -> PathBuf {
    crate::state::paths().root().join("update")
}

/// Remove whatever an earlier update left. Called when the app starts.
pub fn clear_leftovers() {
    let dir = download_dir();
    if dir.starts_with(crate::state::paths().root()) && dir.is_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The checksum `SHA256SUMS.txt` gives for `name`.
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let (hash, file) = (parts.next()?, parts.next()?);
        (file.trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

/// Download the file of `update`, checking it against the release's checksums.
/// `progress` gets the fraction done (0.0 to 1.0). On any problem the partly
/// downloaded file is removed.
pub async fn download(update: &Update, mut progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let asset = update.asset.as_ref().ok_or("This release has no file for your system.")?;
    let sums = update.sums.as_ref().ok_or("This release has no list of checksums, so the file cannot be checked.")?;
    let client = client()?;
    let text = client
        .get(&sums.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "Could not get the list of checksums.".to_string())?
        .text()
        .await
        .map_err(|_| "Could not read the list of checksums.".to_string())?;
    let want = checksum_for(&text, &asset.name).ok_or("The release's checksums do not list this file.")?;

    let dir = download_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make room for the download: {e}"))?;
    let part = dir.join(format!("{}.part", asset.name));
    let done = dir.join(&asset.name);
    let result = async {
        let resp = client
            .get(&asset.url)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|_| "The download could not be started.".to_string())?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = std::fs::File::create(&part).map_err(|e| format!("Could not save the download: {e}"))?;
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "The download was interrupted.".to_string())?;
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|e| format!("Could not save the download: {e}"))?;
            got += chunk.len() as u64;
            if total > 0 {
                progress((got as f32 / total as f32).min(1.0));
            }
        }
        file.flush().map_err(|e| format!("Could not save the download: {e}"))?;
        drop(file);
        let have = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if have != want {
            return Err("The download does not match its checksum, so it was thrown away.".to_string());
        }
        std::fs::rename(&part, &done).map_err(|e| format!("Could not finish the download: {e}"))?;
        Ok(done.clone())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// What was done with a downloaded file.
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    /// The installer is running; this program should close now. Only Windows
    /// replaces itself.
    #[cfg_attr(not(windows), allow(dead_code))]
    Replacing,
    /// The file was opened or shown. Say what to do next.
    Handed { path: PathBuf, command: Option<String> },
}

/// Do what the update calls for with the downloaded `path`.
pub fn apply(update: &Update, path: &std::path::Path) -> Result<Applied, String> {
    match update.apply {
        Apply::Replace => {
            #[cfg(windows)]
            {
                // The installer closes this copy, replaces it, and opens the new one.
                std::process::Command::new(path)
                    .args(["/SILENT", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH=1"])
                    .spawn()
                    .map_err(|e| format!("Could not start the installer: {e}"))?;
                Ok(Applied::Replacing)
            }
            #[cfg(not(windows))]
            {
                let _ = path;
                Err("Updating in place is only for the Windows installer.".into())
            }
        }
        Apply::Open => {
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(path).spawn();
            Ok(Applied::Handed { path: path.to_path_buf(), command: None })
        }
        Apply::Reveal => {
            let dir = path.parent().unwrap_or(path);
            #[cfg(windows)]
            let _ = std::process::Command::new("explorer").arg("/select,").arg(path).spawn();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(dir).spawn();
            #[cfg(all(unix, not(target_os = "macos")))]
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
            let _ = dir;
            Ok(Applied::Handed { path: path.to_path_buf(), command: install_command(path) })
        }
        Apply::Page => Err("This release has no file for your system.".into()),
    }
}

/// Whether a check is due: the person has not turned it off, and it has been a
/// day since the last one.
pub fn check_due(enabled: bool, last_check: u64, now: u64) -> bool {
    enabled && now.saturating_sub(last_check) >= 24 * 60 * 60
}

/// Whether `version` should be offered on its own: it is not the one skipped.
pub fn offer_unasked(version: &str, skipped: &str) -> bool {
    skipped.trim_start_matches('v') != version.trim_start_matches('v')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn versions_are_compared_as_numbers() {
        assert!(is_newer("1.0.2", "1.0.1"));
        assert!(is_newer("v1.10.0", "1.9.9"), "ten is more than nine");
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.0.1", "1.0.1"));
        assert!(!is_newer("1.0.0", "1.0.1"), "never a downgrade");
        assert!(!is_newer("1.1.0-beta", "1.0.1"), "a pre-release is not offered");
        assert!(!is_newer("nonsense", "1.0.1"));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn a_skipped_version_is_not_offered_but_the_next_one_is() {
        assert!(!offer_unasked("1.1.0", "1.1.0"));
        assert!(!offer_unasked("1.1.0", "v1.1.0"));
        assert!(offer_unasked("1.2.0", "1.1.0"), "a newer version comes back");
        assert!(offer_unasked("1.1.0", ""));
    }

    #[test]
    fn a_check_is_due_once_a_day_and_only_when_on() {
        let day = 24 * 60 * 60;
        assert!(check_due(true, 0, 1_000_000));
        assert!(!check_due(true, 1_000_000 - day + 1, 1_000_000));
        assert!(check_due(true, 1_000_000 - day, 1_000_000));
        assert!(!check_due(false, 0, 1_000_000), "off means off");
        assert!(!check_due(true, 2_000_000, 1_000_000), "a clock that went back does not check at every start");
    }

    #[test]
    fn release_notes_become_a_few_plain_lines() {
        let notes = "### Features\n\n- Export as **CBZ**, a comic archive with the series and the [chapters](https://x.y/z).\n- `Ultra save` pages are about a tenth of the size.\n\n### Fixes\n\n- The Export button stays on screen in a short window, which it did not before when the window was made as small as the app allows it to be.\n- One\n- Two\n- Three\n- Four\n";
        let s = summarize(notes);
        assert_eq!(s.len(), SUMMARY_LINES);
        assert_eq!(s[0], "Export as CBZ, a comic archive with the series and the chapters.");
        assert_eq!(s[1], "Ultra save pages are about a tenth of the size.");
        assert!(s[2].ends_with('\u{2026}') && s[2].chars().count() <= SUMMARY_WIDTH + 1, "{}", s[2]);
        assert!(!s.iter().any(|l| l.contains('#') || l.contains('`') || l.contains("](")));
        // Notes with no bullets give their first lines.
        assert_eq!(summarize("## 1.1\n\nA calmer release.\nMore later.\n"), ["A calmer release.", "More later."]);
        assert!(summarize("").is_empty());
    }

    #[test]
    fn the_checksum_of_a_file_is_found_by_name() {
        let h = "a".repeat(64);
        let g = "B".repeat(64);
        let sums = format!("{h}  MangaPoint-1.1.0-macos-arm64.dmg\r\n{g} *MangaPoint-1.1.0-linux-x86_64.deb\n");
        assert_eq!(checksum_for(&sums, "MangaPoint-1.1.0-macos-arm64.dmg"), Some(h));
        assert_eq!(checksum_for(&sums, "MangaPoint-1.1.0-linux-x86_64.deb"), Some("b".repeat(64)));
        assert_eq!(checksum_for(&sums, "other"), None);
        assert_eq!(checksum_for("short  file", "file"), None, "a line that is not a checksum is not one");
    }

    #[test]
    fn only_a_proper_newer_release_is_an_update() {
        let release = |tag: &str, draft: bool, pre: bool| Release {
            tag_name: tag.into(),
            body: "- Something new.".into(),
            html_url: "https://example.test/r".into(),
            draft,
            prerelease: pre,
            assets: vec![
                ReleaseAsset { name: "SHA256SUMS.txt".into(), browser_download_url: "https://example.test/sums".into() },
                ReleaseAsset { name: format!("MangaPoint-{}-linux-x86_64", tag.trim_start_matches('v')), browser_download_url: "https://example.test/f".into() },
            ],
        };
        assert!(update_from(release("v9.9.9", true, false), "1.0.1").is_none(), "a draft");
        assert!(update_from(release("v9.9.9", false, true), "1.0.1").is_none(), "a pre-release");
        assert!(update_from(release("v1.0.1", false, false), "1.0.1").is_none(), "the same version");
        let u = update_from(release("v9.9.9", false, false), "1.0.1").unwrap();
        assert_eq!((u.version.as_str(), u.summary.as_slice()), ("9.9.9", &["Something new.".to_string()][..]));
        assert_eq!(u.page, "https://example.test/r");
        assert!(u.sums.is_some());
    }

    #[test]
    fn the_file_for_each_system_is_chosen_by_name() {
        let all = names(&[
            "MangaPoint-1.1.0-windows-x86_64-setup.exe",
            "MangaPoint-1.1.0-windows-x86_64-portable.exe",
            "MangaPoint-1.1.0-macos-arm64.dmg",
            "MangaPoint-1.1.0-macos-x86_64.dmg",
            "MangaPoint-1.1.0-linux-x86_64.deb",
            "MangaPoint-1.1.0-linux-x86_64.rpm",
            "MangaPoint-1.1.0-linux-x86_64",
        ]);
        let (name, _) = choose_asset(&all, "1.1.0").expect("a file for the system the tests run on");
        assert!(name.starts_with("MangaPoint-1.1.0-"));
        assert!(choose_asset(&names(&["MangaPoint-1.1.0-plan9.zip"]), "1.1.0").is_none());
        assert!(choose_asset(&all, "9.9.9").is_none(), "another version's files are not this one's");
    }

    #[test]
    fn a_package_comes_with_the_command_to_install_it() {
        let deb = PathBuf::from("/tmp/u/MangaPoint-1.1.0-linux-x86_64.deb");
        assert_eq!(install_command(&deb).unwrap(), "sudo apt install \"/tmp/u/MangaPoint-1.1.0-linux-x86_64.deb\"");
        assert!(install_command(std::path::Path::new("/tmp/u/MangaPoint-1.1.0-macos-arm64.dmg")).is_none());
    }
}
