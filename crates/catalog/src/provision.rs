//! Getting a verified copy of Suwayomi onto disk.
//!
//! The archive is downloaded from the official GitHub release, hashed while
//! it streams, and compared with the digest pinned in [`crate::release`].
//! Nothing is extracted unless the hash matches.

use crate::error::{CatalogError, Result};
use crate::release::{ArchiveKind, Asset, VERSION_TAG};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// A usable Suwayomi install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// Directory holding `bin/` and `jre/`.
    pub dir: PathBuf,
    pub java: PathBuf,
    pub jar: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    Downloading { done: u64, total: Option<u64> },
    Verifying,
    Extracting,
    Done,
}

/// Install Suwayomi under `root` if it is not already there, and return it.
///
/// Layout: `root/suwayomi-<tag>/` for the install, `root/downloads/` for
/// the temporary archive (deleted after a successful extraction).
pub async fn provision(
    root: &Path,
    asset: &Asset,
    mut progress: impl FnMut(Progress),
) -> Result<Install> {
    let target = root.join(format!("suwayomi-{VERSION_TAG}"));
    if let Ok(install) = locate_install(&target) {
        progress(Progress::Done);
        return Ok(install);
    }

    let downloads = root.join("downloads");
    fs::create_dir_all(&downloads)?;
    let archive = downloads.join(&asset.name);

    // Reuse a previous download only if it still matches the pinned hash.
    let reusable = archive.exists() && sha256_file(&archive).ok().as_deref() == Some(asset.sha256);
    if !reusable {
        download_verified(asset, &archive, &mut progress).await?;
    }

    progress(Progress::Extracting);
    let staging = root.join(format!(".extracting-{VERSION_TAG}"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    {
        let (archive, staging, kind) = (archive.clone(), staging.clone(), asset.kind);
        tokio::task::spawn_blocking(move || extract(&archive, &staging, kind))
            .await
            .map_err(|e| CatalogError::Archive(e.to_string()))??;
    }

    // The archive contains one top-level folder; find the real install root.
    let found = locate_install(&staging)?;
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&found.dir, &target)?;
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&archive);

    let install = locate_install(&target)?;
    progress(Progress::Done);
    Ok(install)
}

async fn download_verified(
    asset: &Asset,
    dest: &Path,
    progress: &mut impl FnMut(Progress),
) -> Result<()> {
    let part = dest.with_extension("part");
    let response = reqwest::get(asset.url()).await?.error_for_status()?;
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(Progress::Downloading { done, total });
    }
    file.flush().await?;
    drop(file);

    progress(Progress::Verifying);
    let actual = hex(&hasher.finalize());
    if actual != asset.sha256 {
        let _ = fs::remove_file(&part);
        return Err(CatalogError::ChecksumMismatch {
            asset: asset.name.clone(),
            expected: asset.sha256.to_string(),
            actual,
        });
    }
    fs::rename(&part, dest)?;
    Ok(())
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn extract(archive: &Path, dest: &Path, kind: ArchiveKind) -> Result<()> {
    match kind {
        ArchiveKind::Zip => extract_zip(fs::File::open(archive)?, dest),
        ArchiveKind::TarGz => {
            let file = fs::File::open(archive)?;
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
            // `unpack` refuses entries that would escape `dest`.
            tar.unpack(dest)
                .map_err(|e| CatalogError::Archive(e.to_string()))
        }
    }
}

fn extract_zip<R: io::Read + io::Seek>(reader: R, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| CatalogError::Archive(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| CatalogError::Archive(e.to_string()))?;
        // `enclosed_name` is None for absolute paths and `..` traversal.
        let relative = entry.enclosed_name().ok_or_else(|| {
            CatalogError::Archive(format!("unsafe path in archive: {}", String::from_utf8_lossy(entry.name_raw())))
        })?;
        debug_assert!(relative
            .components()
            .all(|c| matches!(c, Component::Normal(_))));
        let out_path = dest.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)?;
        io::copy(&mut entry, &mut out)?;
        out.flush()?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// Find `bin/Suwayomi-Server.jar` and the bundled Java within `dir`
/// (looking at most two folders deep, since archives add a top-level folder).
pub fn locate_install(dir: &Path) -> Result<Install> {
    let bad =
        |what: &str| CatalogError::BadInstall(format!("{what} not found under {}", dir.display()));
    let root = candidate_roots(dir)
        .into_iter()
        .find(|r| r.join("bin").join("Suwayomi-Server.jar").is_file())
        .ok_or_else(|| bad("bin/Suwayomi-Server.jar"))?;
    let jar = root.join("bin").join("Suwayomi-Server.jar");
    let java = find_java(&root.join("jre")).ok_or_else(|| bad("bundled Java (jre/bin/java)"))?;
    Ok(Install {
        dir: root,
        java,
        jar,
    })
}

fn candidate_roots(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    if let Ok(entries) = fs::read_dir(dir) {
        out.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    out
}

fn find_java(jre: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "java.exe" } else { "java" };
    let direct = jre.join("bin").join(name);
    if direct.is_file() {
        return Some(direct);
    }
    // macOS bundles keep the runtime under Contents/Home.
    let mac = jre.join("Contents").join("Home").join("bin").join(name);
    mac.is_file().then_some(mac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, data) in entries {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extracts_normal_archives() {
        let dest = temp_dir("zip-ok");
        let data = zip_with(&[("top/bin/a.txt", b"hello"), ("top/jre/bin/b.txt", b"x")]);
        extract_zip(Cursor::new(data), &dest).unwrap();
        assert_eq!(fs::read(dest.join("top/bin/a.txt")).unwrap(), b"hello");
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn refuses_path_traversal() {
        let dest = temp_dir("zip-evil");
        let outside = dest.parent().unwrap().join("mangapoint-escaped.txt");
        let _ = fs::remove_file(&outside);
        let data = zip_with(&[("../mangapoint-escaped.txt", b"pwned")]);
        let err = extract_zip(Cursor::new(data), &dest).unwrap_err();
        assert!(matches!(err, CatalogError::Archive(_)), "{err:?}");
        assert!(!outside.exists());
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn sha256_matches_known_vector() {
        let dir = temp_dir("sha");
        let f = dir.join("abc.txt");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_file(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn locate_install_finds_nested_layout() {
        let dir = temp_dir("locate");
        let root = dir.join("Suwayomi-Server-x");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("jre/bin")).unwrap();
        fs::write(root.join("bin/Suwayomi-Server.jar"), b"").unwrap();
        let java = if cfg!(windows) { "java.exe" } else { "java" };
        fs::write(root.join("jre/bin").join(java), b"").unwrap();
        let found = locate_install(&dir).unwrap();
        assert_eq!(found.dir, root);
        assert!(matches!(
            locate_install(&dir.join("jre")),
            Err(CatalogError::BadInstall(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }
}
