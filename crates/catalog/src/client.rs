//! Typed access to Suwayomi's GraphQL API.
//!
//! Suwayomi blocks schema introspection, so every query here is written by
//! hand against names confirmed on a live server (v2.4.2366). Values are
//! inlined using JSON string escaping (valid GraphQL string syntax) rather
//! than variables, because the input type names cannot be discovered.

use crate::error::{classify_graphql_error, CatalogError, Result};
use reqwest::Client;
use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct SuwayomiClient {
    http: Client,
    base: String,
    credentials: Option<Credentials>,
    limits: Limits,
}

/// How long each kind of call may take, and how much a body may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Time to open a connection (the server is on this computer).
    pub connect: Duration,
    /// Quick GraphQL queries: about, sources, installed, status.
    pub query: Duration,
    /// Calls where the server scrapes a website: chapter lists, pages, searches.
    pub scrape: Duration,
    /// Fetching a page image or cover.
    pub bytes: Duration,
    /// The most a page image or cover may be.
    pub max_body: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            query: Duration::from_secs(30),
            scrape: Duration::from_secs(90),
            bytes: Duration::from_secs(60),
            max_body: 64 * 1024 * 1024,
        }
    }
}

// ---- Response types ---------------------------------------------------------

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub build_type: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub pkg_name: String,
    pub name: String,
    pub lang: String,
    pub is_installed: bool,
    pub is_nsfw: bool,
    pub has_update: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub is_nsfw: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MangaSummary {
    pub id: i64,
    pub title: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowsePage {
    pub mangas: Vec<MangaSummary>,
    pub has_next_page: bool,
}

const MANGA_FIELDS: &str = "id title description author artist genre status thumbnailUrl";

/// What a source says about a series itself.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MangaDetails {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    #[serde(default)]
    pub genre: Vec<String>,
    /// For example "ONGOING" or "COMPLETED".
    pub status: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: i64,
    pub name: String,
    pub chapter_number: f64,
    pub scanlator: Option<String>,
    /// Milliseconds since the epoch; 0 when the source gives no date.
    #[serde(deserialize_with = "int_or_string")]
    pub upload_date: i64,
    pub is_downloaded: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterStatus {
    pub is_downloaded: bool,
    pub page_count: i32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct QueueItem {
    pub chapter: QueueChapter,
    pub progress: f64,
    pub tries: i32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct QueueChapter {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DownloadStatus {
    pub state: String,
    pub queue: Vec<QueueItem>,
}

/// What to list from a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Browse {
    Popular,
    Latest,
    Search(String),
}

/// Suwayomi sends some 64-bit values as JSON strings.
fn int_or_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<i64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("number out of range")),
        Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!(
            "expected integer, got {other}"
        ))),
    }
}

/// A GraphQL string literal for `s`. JSON escaping is a subset-compatible
/// superset-safe encoding for GraphQL strings, which also stops injection.
pub fn literal(s: &str) -> String {
    serde_json::to_string(s).expect("strings always serialize")
}

// ---- Client -----------------------------------------------------------------

impl SuwayomiClient {
    pub fn new(base_url: impl Into<String>, credentials: Option<Credentials>) -> Result<Self> {
        Self::with_limits(base_url, credentials, Limits::default())
    }

    pub fn with_limits(
        base_url: impl Into<String>,
        credentials: Option<Credentials>,
        limits: Limits,
    ) -> Result<Self> {
        let http = Client::builder().connect_timeout(limits.connect).build()?;
        Ok(Self {
            http,
            base: base_url.into().trim_end_matches('/').to_string(),
            credentials,
            limits,
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn request(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        let req = self.http.request(method, url);
        match &self.credentials {
            Some(c) => req.basic_auth(&c.username, Some(&c.password)),
            None => req,
        }
    }

    /// Send a GraphQL document and decode `data`. Any GraphQL error becomes a
    /// specific [`CatalogError`] where we recognise the message.
    /// Uses the long (scraping) time allowance; see [`SuwayomiClient::query`]
    /// and [`SuwayomiClient::graphql_within`] for shorter ones.
    pub async fn graphql<T: DeserializeOwned>(&self, query: &str) -> Result<T> {
        self.graphql_within(query, self.limits.scrape).await
    }

    /// A GraphQL call that should be quick (status, lists); gives up after
    /// [`Limits::query`].
    pub async fn query<T: DeserializeOwned>(&self, query: &str) -> Result<T> {
        self.graphql_within(query, self.limits.query).await
    }

    /// Like [`SuwayomiClient::graphql`] with the caller's own time limit
    /// for the whole call.
    pub async fn graphql_within<T: DeserializeOwned>(
        &self,
        query: &str,
        timeout: Duration,
    ) -> Result<T> {
        let response = self
            .request(reqwest::Method::POST, format!("{}/api/graphql", self.base))
            .timeout(timeout)
            .json(&json!({ "query": query }))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        if let Some(errors) = body.get("errors").and_then(Value::as_array) {
            let message = errors
                .first()
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(classify_graphql_error(message));
        }
        let data = body
            .get("data")
            .cloned()
            .ok_or_else(|| CatalogError::BadResponse("no data field".into()))?;
        serde_json::from_value(data).map_err(|e| CatalogError::BadResponse(e.to_string()))
    }

    /// GET a path or URL on the server, such as a page image returned by
    /// [`SuwayomiClient::chapter_pages`].
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}/{}", self.base, path.trim_start_matches('/'))
        };
        let limit = self.limits.max_body;
        let mut response = self
            .request(reqwest::Method::GET, url)
            .timeout(self.limits.bytes)
            .send()
            .await?
            .error_for_status()?;
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(CatalogError::TooLarge { limit });
        }
        let mut body = Vec::with_capacity(
            response
                .content_length()
                .map_or(0, |n| n.min(limit) as usize),
        );
        while let Some(chunk) = response.chunk().await? {
            if body.len() as u64 + chunk.len() as u64 > limit {
                return Err(CatalogError::TooLarge { limit });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    pub async fn about(&self) -> Result<ServerInfo> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            about_server: ServerInfo,
        }
        let r: R = self
            .query("{ aboutServer { name version buildType } }")
            .await?;
        Ok(r.about_server)
    }

    /// Whether the embedded browser is enabled in the running server.
    pub async fn kcef_enabled(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct S {
            #[serde(rename = "kcefEnabled")]
            kcef_enabled: bool,
        }
        #[derive(Deserialize)]
        struct R {
            settings: S,
        }
        let r: R = self.query("{ settings { kcefEnabled } }").await?;
        Ok(r.settings.kcef_enabled)
    }

    /// Add an extension store, for example
    /// `https://github.com/keiyoushi/extensions/raw/repo/index.pb`.
    ///
    /// Suwayomi's older `extensionRepos` setting is deprecated, and it
    /// rewrites GitHub URLs to a stub index, so it must not be used. The new
    /// store list is not exposed through the GraphQL settings type, so the
    /// way to check a store loaded is [`SuwayomiClient::fetch_extensions`].
    pub async fn add_extension_store(&self, url: &str) -> Result<()> {
        let q = format!(
            "mutation {{ addExtensionStore(input:{{indexUrl:{}}}) {{ __typename }} }}",
            literal(url)
        );
        let _: Value = self.graphql(&q).await?;
        Ok(())
    }

    pub async fn remove_extension_store(&self, url: &str) -> Result<()> {
        let q = format!(
            "mutation {{ removeExtensionStore(input:{{indexUrl:{}}}) {{ __typename }} }}",
            literal(url)
        );
        let _: Value = self.graphql(&q).await?;
        Ok(())
    }

    /// Refresh the extension list from the configured repositories.
    pub async fn fetch_extensions(&self) -> Result<Vec<Extension>> {
        #[derive(Deserialize)]
        struct Inner {
            extensions: Vec<Extension>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            fetch_extensions: Inner,
        }
        let r: R = self
            .graphql("mutation { fetchExtensions(input:{}) { extensions { pkgName name lang isInstalled isNsfw hasUpdate } } }")
            .await?;
        Ok(r.fetch_extensions.extensions)
    }

    pub async fn install_extension(&self, pkg_name: &str) -> Result<()> {
        self.patch_extension(pkg_name, "install:true").await
    }

    pub async fn uninstall_extension(&self, pkg_name: &str) -> Result<()> {
        self.patch_extension(pkg_name, "uninstall:true").await
    }

    async fn patch_extension(&self, pkg_name: &str, patch: &str) -> Result<()> {
        let q = format!(
            "mutation {{ updateExtension(input:{{id:{}, patch:{{{patch}}}}}) {{ extension {{ pkgName isInstalled }} }} }}",
            literal(pkg_name)
        );
        let _: Value = self.graphql(&q).await?;
        Ok(())
    }

    pub async fn list_sources(&self) -> Result<Vec<Source>> {
        #[derive(Deserialize)]
        struct Nodes {
            nodes: Vec<Source>,
        }
        #[derive(Deserialize)]
        struct R {
            sources: Nodes,
        }
        let r: R = self
            .query("{ sources(first:1000) { nodes { id name lang isNsfw } } }")
            .await?;
        Ok(r.sources.nodes)
    }

    pub async fn browse(&self, source_id: &str, kind: &Browse, page: u32) -> Result<BrowsePage> {
        let (ty, query) = match kind {
            Browse::Popular => ("POPULAR", String::new()),
            Browse::Latest => ("LATEST", String::new()),
            Browse::Search(q) => ("SEARCH", format!(", query:{}", literal(q))),
        };
        let q = format!(
            "mutation {{ fetchSourceManga(input:{{source:{}, type:{ty}, page:{page}{query}}}) {{ hasNextPage mangas {{ id title thumbnailUrl }} }} }}",
            literal(source_id)
        );
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Inner {
            has_next_page: bool,
            mangas: Vec<MangaSummary>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            fetch_source_manga: Inner,
        }
        let r: R = self.graphql(&q).await?;
        Ok(BrowsePage {
            mangas: r.fetch_source_manga.mangas,
            has_next_page: r.fetch_source_manga.has_next_page,
        })
    }

    /// Fetch the chapter list from the source, in the source's own order.
    pub async fn fetch_chapters(&self, manga_id: i64) -> Result<Vec<Chapter>> {
        #[derive(Deserialize)]
        struct Inner {
            chapters: Vec<Chapter>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            fetch_chapters: Inner,
        }
        let q = format!(
            "mutation {{ fetchChapters(input:{{mangaId:{manga_id}}}) {{ chapters {{ id name chapterNumber scanlator uploadDate isDownloaded }} }} }}"
        );
        let r: R = self.graphql(&q).await?;
        Ok(r.fetch_chapters.chapters)
    }

    /// The series a chapter belongs to, with its description, author, genres,
    /// status and cover. Works for any chapter the server has seen, so it can
    /// describe a series downloaded long ago.
    pub async fn manga_of_chapter(&self, chapter_id: i64) -> Result<MangaDetails> {
        #[derive(Deserialize)]
        struct Inner {
            manga: MangaDetails,
        }
        #[derive(Deserialize)]
        struct R {
            chapter: Inner,
        }
        let q = format!("{{ chapter(id:{chapter_id}) {{ manga {{ {MANGA_FIELDS} }} }} }}");
        let known: MangaDetails = self.query::<R>(&q).await?.chapter.manga;
        self.fill_in_details(known).await
    }

    /// A series by its id on the server, with its details; for a series that
    /// was saved without downloading anything.
    pub async fn manga_details(&self, manga_id: i64) -> Result<MangaDetails> {
        #[derive(Deserialize)]
        struct R {
            manga: MangaDetails,
        }
        let q = format!("{{ manga(id:{manga_id}) {{ {MANGA_FIELDS} }} }}");
        let known: MangaDetails = self.query::<R>(&q).await?.manga;
        self.fill_in_details(known).await
    }

    /// A server only knows a series' title and cover from a search; the rest
    /// is read from the source on request, so if it is missing this asks.
    async fn fill_in_details(&self, known: MangaDetails) -> Result<MangaDetails> {
        let missing = known.description.as_deref().is_none_or(str::is_empty)
            && known.author.is_none()
            && known.genre.is_empty();
        if !missing {
            return Ok(known);
        }
        #[derive(Deserialize)]
        struct Fetched {
            manga: MangaDetails,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct F {
            fetch_manga: Fetched,
        }
        let q = format!(
            "mutation {{ fetchManga(input:{{id:{}}}) {{ manga {{ {MANGA_FIELDS} }} }} }}",
            known.id
        );
        // If the source cannot be reached, what was already known is still useful.
        Ok(match self.graphql::<F>(&q).await {
            Ok(f) => f.fetch_manga.manga,
            Err(_) => known,
        })
    }

    /// Page image paths for a chapter. Each is fetchable with
    /// [`SuwayomiClient::get_bytes`], whether or not the chapter has been
    /// downloaded.
    pub async fn chapter_pages(&self, chapter_id: i64) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Inner {
            pages: Vec<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            fetch_chapter_pages: Inner,
        }
        let q = format!(
            "mutation {{ fetchChapterPages(input:{{chapterId:{chapter_id}}}) {{ pages }} }}"
        );
        let r: R = self.graphql(&q).await?;
        Ok(r.fetch_chapter_pages.pages)
    }

    pub async fn enqueue_download(&self, chapter_id: i64) -> Result<()> {
        let q = format!(
            "mutation {{ enqueueChapterDownload(input:{{id:{chapter_id}}}) {{ downloadStatus {{ state }} }} }}"
        );
        let _: Value = self.graphql(&q).await?;
        Ok(())
    }

    pub async fn start_downloader(&self) -> Result<()> {
        let _: Value = self
            .graphql("mutation { startDownloader(input:{}) { downloadStatus { state } } }")
            .await?;
        Ok(())
    }

    pub async fn clear_downloader(&self) -> Result<()> {
        let _: Value = self
            .graphql("mutation { clearDownloader(input:{}) { downloadStatus { state } } }")
            .await?;
        Ok(())
    }

    pub async fn download_status(&self) -> Result<DownloadStatus> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct R {
            download_status: DownloadStatus,
        }
        let r: R = self
            .query("{ downloadStatus { state queue { progress tries chapter { id } } } }")
            .await?;
        Ok(r.download_status)
    }

    pub async fn chapter_status(&self, chapter_id: i64) -> Result<ChapterStatus> {
        #[derive(Deserialize)]
        struct R {
            chapter: ChapterStatus,
        }
        let q = format!("{{ chapter(id:{chapter_id}) {{ isDownloaded pageCount }} }}");
        let r: R = self.query(&q).await?;
        Ok(r.chapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_escape_quotes_and_control_characters() {
        assert_eq!(literal("one piece"), "\"one piece\"");
        assert_eq!(literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(literal("x\\y\n"), "\"x\\\\y\\n\"");
        // An attempt to break out of the string stays inside it.
        let evil = literal("\") { id } mutation { x");
        assert!(evil.starts_with("\"\\\")"));
        assert_eq!(evil.matches('"').count() - evil.matches("\\\"").count(), 2);
    }

    #[test]
    fn chapter_upload_date_accepts_numbers_and_strings() {
        let a: Chapter = serde_json::from_value(json!({
            "id": 1, "name": "c", "chapterNumber": 1.5, "scanlator": null,
            "uploadDate": "1700000000000", "isDownloaded": false
        }))
        .unwrap();
        assert_eq!(a.upload_date, 1_700_000_000_000);
        let b: Chapter = serde_json::from_value(json!({
            "id": 2, "name": "c", "chapterNumber": 2.0, "scanlator": "grp",
            "uploadDate": 0, "isDownloaded": true
        }))
        .unwrap();
        assert_eq!(b.upload_date, 0);
        assert_eq!(b.scanlator.as_deref(), Some("grp"));
    }

    /// A one-shot local HTTP server: reads the request, then runs `serve`.
    async fn serve_once<F, Fut>(serve: F) -> String
    where
        F: Fn(tokio::net::TcpStream) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(serve(s));
            }
        });
        url
    }

    fn limits(ms: u64, max_body: u64) -> Limits {
        let d = Duration::from_millis(ms);
        Limits {
            connect: d,
            query: d,
            scrape: d * 4,
            bytes: d,
            max_body,
        }
    }

    #[tokio::test]
    async fn a_stopped_server_is_reported_as_unreachable() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let c = SuwayomiClient::new(format!("http://127.0.0.1:{port}"), None).unwrap();
        let err = c.about().await.unwrap_err();
        assert!(matches!(err, CatalogError::Unreachable(_)), "{err:?}");
        let err = c.get_bytes("/x").await.unwrap_err();
        assert!(matches!(err, CatalogError::Unreachable(_)), "{err:?}");
    }

    #[tokio::test]
    async fn quick_queries_time_out_sooner_than_scrapes() {
        use tokio::io::AsyncReadExt;
        let url = serve_once(|mut s| async move {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
        .await;
        let c = SuwayomiClient::with_limits(url, None, limits(300, 1000)).unwrap();
        let t = std::time::Instant::now();
        let err = c.about().await.unwrap_err();
        assert!(matches!(&err, CatalogError::Http(e) if e.is_timeout()), "{err:?}");
        assert!(t.elapsed() < Duration::from_millis(1100), "{:?}", t.elapsed());
        // A scrape gets the longer allowance (4x here).
        let t = std::time::Instant::now();
        let _ = c.graphql::<Value>("{ x }").await.unwrap_err();
        assert!(t.elapsed() >= Duration::from_millis(1000), "{:?}", t.elapsed());
        // The caller can ask for less.
        let t = std::time::Instant::now();
        let _ = c
            .graphql_within::<Value>("{ x }", Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(t.elapsed() < Duration::from_millis(600));
    }

    const CRLF: &str = "\r\n";

    async fn body_server(len: usize, declare: bool) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        serve_once(move |mut s| async move {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let head = if declare {
                format!("HTTP/1.1 200 OK{CRLF}Content-Length: {len}{CRLF}Connection: close{CRLF}{CRLF}")
            } else {
                format!("HTTP/1.1 200 OK{CRLF}Connection: close{CRLF}{CRLF}")
            };
            let _ = s.write_all(head.as_bytes()).await;
            for _ in 0..len / 100 {
                if s.write_all(&[7u8; 100]).await.is_err() {
                    return;
                }
            }
            let _ = s.shutdown().await;
        })
        .await
    }

    #[tokio::test]
    async fn bodies_over_the_cap_are_refused_but_smaller_ones_pass() {
        let c_big = SuwayomiClient::with_limits(body_server(5000, true).await, None, limits(2000, 1000))
            .unwrap();
        let err = c_big.get_bytes("/p").await.unwrap_err();
        assert!(matches!(err, CatalogError::TooLarge { limit: 1000 }), "{err:?}");
        // No Content-Length: caught while streaming.
        let c_chunked =
            SuwayomiClient::with_limits(body_server(5000, false).await, None, limits(2000, 1000))
                .unwrap();
        let err = c_chunked.get_bytes("/p").await.unwrap_err();
        assert!(matches!(err, CatalogError::TooLarge { limit: 1000 }), "{err:?}");
        let ok = SuwayomiClient::with_limits(body_server(800, true).await, None, limits(2000, 1000))
            .unwrap();
        assert_eq!(ok.get_bytes("/p").await.unwrap().len(), 800);
    }
}
