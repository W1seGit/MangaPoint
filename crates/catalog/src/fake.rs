//! An in-memory [`Catalog`] for tests (enable the `fake` feature).

use crate::api::Catalog;
use crate::client::{Browse, BrowsePage, Chapter, Extension, MangaDetails, MangaSummary, Source};
use crate::error::{CatalogError, Result};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// How a fake source should misbehave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    NeedsBrowser,
    NeedsFlareSolverr,
    Http(u16),
    Other,
}

impl Fault {
    fn error(self) -> CatalogError {
        match self {
            Fault::NeedsBrowser => CatalogError::NeedsBrowser,
            Fault::NeedsFlareSolverr => CatalogError::NeedsFlareSolverr,
            Fault::Http(c) => CatalogError::SourceHttp(c),
            Fault::Other => CatalogError::Graphql("boom".into()),
        }
    }
}

#[derive(Default)]
struct State {
    extensions: Vec<Extension>,
    installed: HashSet<String>,
    stores: Vec<String>,
    /// source id -> (display name, lang)
    sources: Vec<Source>,
    /// extension pkg -> ids of the sources it provides
    ext_sources: HashMap<String, Vec<String>>,
    manga: HashMap<String, Vec<MangaSummary>>,
    chapters: HashMap<i64, Vec<Chapter>>,
    pages: HashMap<i64, Vec<String>>,
    /// chapter id -> details of its series
    details: HashMap<i64, MangaDetails>,
    /// manga id -> its details
    manga_details: HashMap<i64, MangaDetails>,
    bytes: HashMap<String, Vec<u8>>,
    faults: HashMap<String, Fault>,
    /// Fail the first N calls to each path in `get_bytes`.
    flaky_paths: HashMap<String, u32>,
    calls: Vec<String>,
    /// How long each `get_bytes` takes.
    latency_ms: u64,
    /// Extra time for paths starting with a prefix.
    path_latency: Vec<(String, u64)>,
    in_flight: usize,
    max_in_flight: usize,
}

#[derive(Default)]
pub struct FakeCatalog {
    state: Mutex<State>,
}

impl FakeCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer an extension that provides one source. Not installed yet.
    pub fn add_extension(&self, pkg: &str, name: &str, lang: &str, source_id: &str) {
        let mut s = self.state.lock().unwrap();
        s.extensions.push(Extension {
            pkg_name: pkg.into(),
            name: name.into(),
            lang: lang.into(),
            is_installed: false,
            is_nsfw: false,
            has_update: false,
        });
        s.ext_sources
            .entry(pkg.into())
            .or_default()
            .push(source_id.into());
        s.sources.push(Source {
            id: source_id.into(),
            name: name.into(),
            lang: lang.into(),
            is_nsfw: false,
            extension_pkg: Some(pkg.into()),
        });
    }

    pub fn add_manga(&self, source_id: &str, id: i64, title: &str) {
        self.state
            .lock()
            .unwrap()
            .manga
            .entry(source_id.into())
            .or_default()
            .push(MangaSummary {
                id,
                title: title.into(),
                thumbnail_url: Some(format!("/thumb/{id}")),
            });
    }

    pub fn set_chapters(&self, manga_id: i64, chapters: Vec<Chapter>) {
        self.state
            .lock()
            .unwrap()
            .chapters
            .insert(manga_id, chapters);
    }

    pub fn set_pages(&self, chapter_id: i64, paths: &[&str]) {
        self.state
            .lock()
            .unwrap()
            .pages
            .insert(chapter_id, paths.iter().map(|p| p.to_string()).collect());
    }

    pub fn set_details(&self, chapter_id: i64, details: MangaDetails) {
        self.state
            .lock()
            .unwrap()
            .details
            .insert(chapter_id, details);
    }

    pub fn set_manga_details(&self, manga_id: i64, details: MangaDetails) {
        self.state
            .lock()
            .unwrap()
            .manga_details
            .insert(manga_id, details);
    }

    pub fn set_bytes(&self, path: &str, data: Vec<u8>) {
        self.state.lock().unwrap().bytes.insert(path.into(), data);
    }

    pub fn fail_source(&self, source_id: &str, fault: Fault) {
        self.state
            .lock()
            .unwrap()
            .faults
            .insert(source_id.into(), fault);
    }

    pub fn heal_source(&self, source_id: &str) {
        self.state.lock().unwrap().faults.remove(source_id);
    }

    /// Make the next `times` fetches of `path` fail.
    /// Make every `get_bytes` take this long, so tests can overlap requests.
    pub fn set_latency_ms(&self, ms: u64) {
        self.state.lock().unwrap().latency_ms = ms;
    }

    /// Make `get_bytes` of paths starting with `prefix` take this much longer.
    pub fn set_path_latency_ms(&self, prefix: &str, ms: u64) {
        self.state
            .lock()
            .unwrap()
            .path_latency
            .push((prefix.to_string(), ms));
    }

    /// The most `get_bytes` calls that were ever running at the same moment.
    pub fn max_in_flight(&self) -> usize {
        self.state.lock().unwrap().max_in_flight
    }

    pub fn flaky_path(&self, path: &str, times: u32) {
        self.state
            .lock()
            .unwrap()
            .flaky_paths
            .insert(path.into(), times);
    }

    pub fn is_installed(&self, pkg: &str) -> bool {
        self.state.lock().unwrap().installed.contains(pkg)
    }

    pub fn stores(&self) -> Vec<String> {
        self.state.lock().unwrap().stores.clone()
    }

    /// Names of the calls made so far, in order (for asserting behaviour).
    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    fn record(&self, call: String) {
        self.state.lock().unwrap().calls.push(call);
    }

    fn source_fault(&self, source_id: &str) -> Option<Fault> {
        self.state.lock().unwrap().faults.get(source_id).copied()
    }
}

#[async_trait]
impl Catalog for FakeCatalog {
    async fn fetch_extensions(&self) -> Result<Vec<Extension>> {
        let s = self.state.lock().unwrap();
        Ok(s.extensions
            .iter()
            .map(|e| Extension {
                is_installed: s.installed.contains(&e.pkg_name),
                ..e.clone()
            })
            .collect())
    }

    async fn install_extension(&self, pkg_name: &str) -> Result<()> {
        self.record(format!("install {pkg_name}"));
        let mut s = self.state.lock().unwrap();
        if !s.extensions.iter().any(|e| e.pkg_name == pkg_name) {
            return Err(CatalogError::Graphql(format!(
                "unknown extension {pkg_name}"
            )));
        }
        s.installed.insert(pkg_name.into());
        Ok(())
    }

    async fn uninstall_extension(&self, pkg_name: &str) -> Result<()> {
        self.record(format!("uninstall {pkg_name}"));
        self.state.lock().unwrap().installed.remove(pkg_name);
        Ok(())
    }

    async fn add_extension_store(&self, index_url: &str) -> Result<()> {
        self.record(format!("add_store {index_url}"));
        self.state.lock().unwrap().stores.push(index_url.into());
        Ok(())
    }

    async fn list_sources(&self) -> Result<Vec<Source>> {
        let s = self.state.lock().unwrap();
        let live: HashSet<&String> = s
            .installed
            .iter()
            .flat_map(|pkg| s.ext_sources.get(pkg).into_iter().flatten())
            .collect();
        Ok(s.sources
            .iter()
            .filter(|x| live.contains(&x.id))
            .cloned()
            .collect())
    }

    async fn browse(&self, source_id: &str, kind: &Browse, _page: u32) -> Result<BrowsePage> {
        self.record(format!("browse {source_id} {kind:?}"));
        if let Some(f) = self.source_fault(source_id) {
            return Err(f.error());
        }
        let s = self.state.lock().unwrap();
        let all = s.manga.get(source_id).cloned().unwrap_or_default();
        let mangas = match kind {
            Browse::Search(q) => {
                let q = q.to_lowercase();
                all.into_iter()
                    .filter(|m| m.title.to_lowercase().contains(&q))
                    .collect()
            }
            _ => all,
        };
        Ok(BrowsePage {
            mangas,
            has_next_page: false,
        })
    }

    async fn fetch_chapters(&self, manga_id: i64) -> Result<Vec<Chapter>> {
        self.record(format!("chapters {manga_id}"));
        // Faults are keyed by source; find which source owns this manga.
        let owner = {
            let s = self.state.lock().unwrap();
            s.manga
                .iter()
                .find(|(_, v)| v.iter().any(|m| m.id == manga_id))
                .map(|(k, _)| k.clone())
        };
        if let Some(f) = owner.and_then(|o| self.source_fault(&o)) {
            return Err(f.error());
        }
        Ok(self
            .state
            .lock()
            .unwrap()
            .chapters
            .get(&manga_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn manga_of_chapter(&self, chapter_id: i64) -> Result<MangaDetails> {
        self.record(format!("details {chapter_id}"));
        self.state
            .lock()
            .unwrap()
            .details
            .get(&chapter_id)
            .cloned()
            .ok_or_else(|| CatalogError::BadResponse("no such chapter".into()))
    }

    async fn manga_details(&self, manga_id: i64) -> Result<MangaDetails> {
        self.record(format!("manga {manga_id}"));
        self.state
            .lock()
            .unwrap()
            .manga_details
            .get(&manga_id)
            .cloned()
            .ok_or_else(|| CatalogError::BadResponse("no such series".into()))
    }

    async fn chapter_pages(&self, chapter_id: i64) -> Result<Vec<String>> {
        self.record(format!("pages {chapter_id}"));
        Ok(self
            .state
            .lock()
            .unwrap()
            .pages
            .get(&chapter_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        self.record(format!("get {path}"));
        let latency = {
            let mut s = self.state.lock().unwrap();
            s.in_flight += 1;
            s.max_in_flight = s.max_in_flight.max(s.in_flight);
            s.latency_ms
                + s.path_latency
                    .iter()
                    .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
                    .map(|(_, ms)| *ms)
                    .sum::<u64>()
        };
        if latency > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(latency)).await;
        }
        let mut s = self.state.lock().unwrap();
        s.in_flight -= 1;
        if let Some(n) = s.flaky_paths.get_mut(path) {
            if *n > 0 {
                *n -= 1;
                return Err(CatalogError::SourceHttp(503));
            }
        }
        s.bytes
            .get(path)
            .cloned()
            .ok_or(CatalogError::SourceHttp(404))
    }
}

/// A chapter with sensible defaults, for building test data.
pub fn chapter(id: i64, number: f64, upload_date: i64) -> Chapter {
    Chapter {
        id,
        name: format!("Chapter {number}"),
        chapter_number: number,
        scanlator: None,
        upload_date,
        is_downloaded: false,
    }
}
