//! The source test behind the Sources page's "Retest" button.
//!
//! A source passes only if the whole chain works against the live site:
//! popular list -> search -> chapter list -> page list -> a real page image.
//! Nothing is hard-coded about what a source carries, so it keeps working as
//! catalogs change; the optional reference title is an extra sanity check for
//! sources we know carry a particular series.

use catalog::{Browse, Catalog, CatalogError, Source};
use futures_util::stream::{self, StreamExt};
use library::{StepTiming, TestRecord, Verdict};
use std::future::Future;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct CheckOptions {
    /// Longest any single step may take.
    pub step_timeout: Duration,
    /// A title the source should find when searched for.
    pub reference_title: Option<String>,
    /// How many popular series to try when looking for one with chapters.
    pub max_series_tried: usize,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            step_timeout: Duration::from_secs(45),
            reference_title: None,
            max_series_tried: 4,
        }
    }
}

/// Run the full test for one source.
pub async fn check_source<C: Catalog>(
    catalog: &C,
    source: &Source,
    opts: &CheckOptions,
) -> TestRecord {
    let mut steps = Vec::new();
    let outcome = run_steps(catalog, source, opts, &mut steps).await;
    let (verdict, message) = match outcome {
        Ok(()) => (
            Verdict::Pass,
            "Working: found series, chapters and page images.".to_string(),
        ),
        Err(err) => explain(
            &err,
            steps.last().map(|s| s.name.as_str()).unwrap_or("start"),
        ),
    };
    TestRecord {
        at: now_secs(),
        verdict,
        steps,
        message,
    }
}

/// Test several sources, a few at a time, calling `on_done` as each finishes.
pub async fn test_many<C: Catalog>(
    catalog: &C,
    sources: Vec<(Source, CheckOptions)>,
    concurrency: usize,
    mut on_done: impl FnMut(&Source, &TestRecord),
) -> Vec<(Source, TestRecord)> {
    let mut results = Vec::new();
    let mut running = stream::iter(sources)
        .map(|(source, opts)| async move {
            let record = check_source(catalog, &source, &opts).await;
            (source, record)
        })
        .buffer_unordered(concurrency.max(1));
    while let Some((source, record)) = running.next().await {
        on_done(&source, &record);
        results.push((source, record));
    }
    results
}

#[derive(Debug)]
enum StepError {
    Catalog(CatalogError),
    Timeout,
    Check(String),
}

async fn run_steps<C: Catalog>(
    c: &C,
    source: &Source,
    opts: &CheckOptions,
    steps: &mut Vec<StepTiming>,
) -> Result<(), StepError> {
    // 1. popular
    let popular = timed(steps, "popular list", opts, async {
        let page = c.browse(&source.id, &Browse::Popular, 1).await?;
        if page.mangas.is_empty() {
            Err(StepError::Check("the popular list is empty".into()))
        } else {
            Ok(page.mangas)
        }
    })
    .await?;

    // 2. search for something we just saw
    let first = popular[0].title.clone();
    timed(steps, "search", opts, async {
        let found = c
            .browse(&source.id, &Browse::Search(first.clone()), 1)
            .await?;
        if found.mangas.iter().any(|m| same_title(&m.title, &first)) {
            Ok(())
        } else {
            Err(StepError::Check(format!(
                "searching for '{first}' did not find it"
            )))
        }
    })
    .await?;

    // 2b. optional: a series we know this source carries
    if let Some(reference) = &opts.reference_title {
        timed(steps, "reference search", opts, async {
            let found = c
                .browse(&source.id, &Browse::Search(reference.clone()), 1)
                .await?;
            let want = reference.to_lowercase();
            if found
                .mangas
                .iter()
                .any(|m| m.title.to_lowercase().contains(&want))
            {
                Ok(())
            } else {
                Err(StepError::Check(format!("'{reference}' was not found")))
            }
        })
        .await?;
    }

    // 3. chapters of the first popular series that has any
    let chapter = timed(steps, "chapter list", opts, async {
        for manga in popular.iter().take(opts.max_series_tried.max(1)) {
            let chapters = c.fetch_chapters(manga.id).await?;
            if let Some(last) = chapters.last() {
                return Ok(last.clone());
            }
        }
        Err(StepError::Check(
            "none of the popular series has chapters".into(),
        ))
    })
    .await?;

    // 4. pages of that chapter
    let pages = timed(steps, "page list", opts, async {
        let pages = c.chapter_pages(chapter.id).await?;
        if pages.is_empty() {
            Err(StepError::Check("the chapter has no pages".into()))
        } else {
            Ok(pages)
        }
    })
    .await?;

    // 5. a real image
    timed(steps, "page image", opts, async {
        let bytes = c.get_bytes(&pages[0]).await?;
        match image_kind(&bytes) {
            Some(_) if bytes.len() >= 512 => Ok(()),
            Some(_) => Err(StepError::Check(format!(
                "the first page is only {} bytes",
                bytes.len()
            ))),
            None => Err(StepError::Check("the first page is not an image".into())),
        }
    })
    .await?;
    Ok(())
}

/// Run one step, record its timing, and turn a timeout into an error.
async fn timed<T>(
    steps: &mut Vec<StepTiming>,
    name: &str,
    opts: &CheckOptions,
    fut: impl Future<Output = Result<T, StepError>>,
) -> Result<T, StepError> {
    let start = Instant::now();
    let result = match tokio::time::timeout(opts.step_timeout, fut).await {
        Ok(r) => r,
        Err(_) => Err(StepError::Timeout),
    };
    steps.push(StepTiming {
        name: name.to_string(),
        ms: start.elapsed().as_millis() as u64,
        ok: result.is_ok(),
        detail: result.as_ref().err().map(|e| format!("{e:?}")),
    });
    result
}

impl From<CatalogError> for StepError {
    fn from(e: CatalogError) -> Self {
        StepError::Catalog(e)
    }
}

/// Turn a failure into a verdict and a sentence a person can act on.
fn explain(err: &StepError, step: &str) -> (Verdict, String) {
    match err {
        StepError::Catalog(CatalogError::NeedsBrowser) => (
            Verdict::NeedsBrowser,
            "This source needs browser support. Turn it on in Settings, then retest.".into(),
        ),
        StepError::Catalog(CatalogError::NeedsFlareSolverr) => (
            Verdict::NeedsFlareSolverr,
            "This source is protected by Cloudflare and cannot be used right now.".into(),
        ),
        StepError::Catalog(CatalogError::SourceHttp(code)) => (
            Verdict::Failed,
            format!("The site answered with an error ({code}) during the {step} step."),
        ),
        StepError::Timeout => (
            Verdict::Failed,
            format!("The site took too long to answer during the {step} step."),
        ),
        StepError::Check(what) => (
            Verdict::Failed,
            format!("Failed at the {step} step: {what}."),
        ),
        StepError::Catalog(other) => (
            Verdict::Failed,
            format!("Failed at the {step} step: {other}."),
        ),
    }
}

fn same_title(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Recognise common image formats by their first bytes.
pub fn image_kind(b: &[u8]) -> Option<&'static str> {
    match b {
        [0xFF, 0xD8, 0xFF, ..] => Some("JPEG"),
        [0x89, b'P', b'N', b'G', ..] => Some("PNG"),
        [b'G', b'I', b'F', ..] => Some("GIF"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("WebP"),
        _ => None,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::{chapter, FakeCatalog, Fault};

    fn png_bytes(len: usize) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G'];
        v.resize(len, 0);
        v
    }

    fn source(id: &str) -> Source {
        Source {
            id: id.into(),
            name: id.into(),
            lang: "en".into(),
            is_nsfw: false,
            extension_pkg: None,
        }
    }

    /// A source with one series, two chapters, three pages and real-looking images.
    fn healthy() -> FakeCatalog {
        let c = FakeCatalog::new();
        c.add_manga("s", 1, "Berserk");
        c.add_manga("s", 2, "One Piece");
        c.set_chapters(1, vec![chapter(10, 1.0, 0), chapter(11, 2.0, 0)]);
        c.set_pages(11, &["/p/1", "/p/2", "/p/3"]);
        c.set_bytes("/p/1", png_bytes(2048));
        c
    }

    fn step_names(r: &TestRecord) -> Vec<&str> {
        r.steps.iter().map(|s| s.name.as_str()).collect()
    }

    #[tokio::test]
    async fn a_healthy_source_passes_every_step() {
        let c = healthy();
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::Pass, "{}", r.message);
        assert_eq!(
            step_names(&r),
            [
                "popular list",
                "search",
                "chapter list",
                "page list",
                "page image"
            ]
        );
        assert!(r.steps.iter().all(|s| s.ok));
        assert!(r.at > 1_700_000_000);
    }

    #[tokio::test]
    async fn the_reference_title_adds_a_step_and_must_be_found() {
        let c = healthy();
        let mut o = CheckOptions {
            reference_title: Some("one piece".into()),
            ..Default::default()
        };
        let r = check_source(&c, &source("s"), &o).await;
        assert_eq!(r.verdict, Verdict::Pass);
        assert!(step_names(&r).contains(&"reference search"));

        o.reference_title = Some("Nonexistent Series".into());
        let r = check_source(&c, &source("s"), &o).await;
        assert_eq!(r.verdict, Verdict::Failed);
        assert!(r.message.contains("reference search"), "{}", r.message);
        assert_eq!(r.steps.last().unwrap().name, "reference search");
    }

    #[tokio::test]
    async fn needs_browser_and_flaresolverr_are_reported_as_such() {
        let c = healthy();
        c.fail_source("s", Fault::NeedsBrowser);
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::NeedsBrowser);
        assert!(r.message.contains("Settings"));

        c.fail_source("s", Fault::NeedsFlareSolverr);
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::NeedsFlareSolverr);
    }

    #[tokio::test]
    async fn http_errors_and_other_failures_say_where_they_happened() {
        let c = healthy();
        c.fail_source("s", Fault::Http(503));
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::Failed);
        assert!(
            r.message.contains("503") && r.message.contains("popular list"),
            "{}",
            r.message
        );

        c.fail_source("s", Fault::Other);
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::Failed);
    }

    #[tokio::test]
    async fn each_broken_link_in_the_chain_is_caught() {
        // No popular series.
        let c = FakeCatalog::new();
        let r = check_source(&c, &source("empty"), &CheckOptions::default()).await;
        assert!(r.message.contains("popular list is empty"), "{}", r.message);

        // Series without chapters.
        let c = FakeCatalog::new();
        c.add_manga("s", 1, "A");
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert!(r.message.contains("chapter list"), "{}", r.message);

        // Chapter without pages.
        let c = FakeCatalog::new();
        c.add_manga("s", 1, "A");
        c.set_chapters(1, vec![chapter(5, 1.0, 0)]);
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert!(r.message.contains("no pages"), "{}", r.message);

        // Page that is not an image / too small / missing.
        for (bytes, needle) in [
            (Some(b"<html>blocked</html>".to_vec()), "not an image"),
            (Some(png_bytes(100)), "only 100 bytes"),
            (None, "404"),
        ] {
            let c = FakeCatalog::new();
            c.add_manga("s", 1, "A");
            c.set_chapters(1, vec![chapter(5, 1.0, 0)]);
            c.set_pages(5, &["/p"]);
            if let Some(b) = bytes {
                c.set_bytes("/p", b);
            }
            let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
            assert_eq!(r.verdict, Verdict::Failed);
            assert!(r.message.contains(needle), "{needle}: {}", r.message);
        }
    }

    #[tokio::test]
    async fn the_first_series_without_chapters_is_skipped_over() {
        let c = FakeCatalog::new();
        c.add_manga("s", 1, "Empty One");
        c.add_manga("s", 2, "Has Chapters");
        c.set_chapters(2, vec![chapter(7, 1.0, 0)]);
        c.set_pages(7, &["/p"]);
        c.set_bytes("/p", png_bytes(1024));
        let r = check_source(&c, &source("s"), &CheckOptions::default()).await;
        assert_eq!(r.verdict, Verdict::Pass, "{}", r.message);
    }

    #[tokio::test]
    async fn test_many_reports_each_result_and_returns_all() {
        let good = healthy();
        let sources = vec![
            (source("s"), CheckOptions::default()),
            (source("missing"), CheckOptions::default()),
        ];
        let mut seen = Vec::new();
        let results = test_many(&good, sources, 2, |s, r| {
            seen.push((s.id.clone(), r.verdict))
        })
        .await;
        assert_eq!(results.len(), 2);
        assert_eq!(seen.len(), 2);
        assert!(seen.contains(&("s".to_string(), Verdict::Pass)));
        assert!(seen.contains(&("missing".to_string(), Verdict::Failed)));
    }

    #[test]
    fn image_kinds_are_recognised() {
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("JPEG"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("WebP"));
        assert_eq!(image_kind(b"GIF89a"), Some("GIF"));
        assert_eq!(image_kind(b"<html>"), None);
        assert_eq!(image_kind(&[]), None);
    }
}
