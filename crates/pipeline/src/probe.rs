//! A short speed test of a source: fetch a handful of pages at once and see
//! how long it takes. Cheap enough to run before a download, and it tells
//! sources apart far better than guessing.

use crate::fetch::{fetch_page, with_retry, RetryPolicy};
use crate::types::{CancelFlag, ChapterRef};
use catalog::Catalog;
use converter::decode::image_dimensions;
use futures_util::future::join_all;
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::time::{Duration, Instant};

/// Pages fetched at once in a probe.
pub const PROBE_PAGES: usize = 6;

/// Pages per second the source gave for the first few pages of this chapter,
/// including the time to list them. `None` if it gave no usable page.
pub async fn probe_source<C: Catalog>(
    catalog: &C,
    option: &ChapterRef,
    policy: &RetryPolicy,
) -> Option<f32> {
    let started = Instant::now();
    let paths = with_retry(policy, || catalog.chapter_pages(option.chapter_id))
        .await
        .ok()?;
    let wanted: Vec<&String> = paths.iter().take(PROBE_PAGES).collect();
    if wanted.is_empty() {
        return None;
    }
    let pages = join_all(wanted.iter().map(|path| fetch_page(catalog, path, policy))).await;
    let usable = pages
        .iter()
        .filter(|r| {
            r.as_ref()
                .is_ok_and(|bytes| image_dimensions(bytes).is_ok())
        })
        .count();
    if usable == 0 {
        return None;
    }
    Some(usable as f32 / started.elapsed().as_secs_f32().max(0.001))
}

/// The longest a whole speed test may take. A source that has not answered by
/// then is left out of the results (unmeasured), not marked as failing.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(20);

/// Probe several sources at the same time. Each entry is a source id and a
/// chapter it can supply. Not cancellable; see [`probe_sources_cancellable`].
pub async fn probe_sources<C: Catalog>(
    catalog: &C,
    probes: &[(String, ChapterRef)],
    policy: &RetryPolicy,
) -> Vec<(String, Option<f32>)> {
    probe_sources_cancellable(catalog, probes, policy, &CancelFlag::new()).await
}

/// As [`probe_sources`], but it stops at once when `cancel` is set and never
/// takes longer than [`PROBE_DEADLINE`]. Sources that had not answered by
/// then are missing from the result (in the order given for the rest).
pub async fn probe_sources_cancellable<C: Catalog>(
    catalog: &C,
    probes: &[(String, ChapterRef)],
    policy: &RetryPolicy,
    cancel: &CancelFlag,
) -> Vec<(String, Option<f32>)> {
    probe_with_deadline(catalog, probes, policy, cancel, PROBE_DEADLINE).await
}

async fn probe_with_deadline<C: Catalog>(
    catalog: &C,
    probes: &[(String, ChapterRef)],
    policy: &RetryPolicy,
    cancel: &CancelFlag,
    deadline: Duration,
) -> Vec<(String, Option<f32>)> {
    if cancel.is_cancelled() {
        return Vec::new();
    }
    let mut pending: FuturesUnordered<_> = probes
        .iter()
        .enumerate()
        .map(|(n, (source, option))| async move {
            (n, source.clone(), probe_source(catalog, option, policy).await)
        })
        .collect();
    let mut done = Vec::new();
    let timeout = tokio::time::sleep(deadline);
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            next = pending.next() => match next {
                Some(item) => done.push(item),
                None => break,
            },
            () = &mut timeout => break,
            () = cancel.cancelled() => break,
        }
    }
    // Dropping the rest stops their requests.
    drop(pending);
    done.sort_by_key(|(n, _, _)| *n);
    done.into_iter().map(|(_, source, speed)| (source, speed)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::FakeCatalog;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn png() -> Vec<u8> {
        let img = RgbaImage::from_fn(30, 40, |x, y| Rgba([(x + y) as u8, 0, 0, 255]));
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), 30, 40, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn quick() -> RetryPolicy {
        RetryPolicy {
            attempts: 1,
            base_delay: Duration::from_millis(1),
        }
    }

    fn add(c: &FakeCatalog, id: i64, pages: usize, latency: u64) {
        let paths: Vec<String> = (0..pages).map(|i| format!("/c{id}/p{i}")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        c.set_pages(id, &refs);
        for p in &paths {
            c.set_bytes(p, png());
        }
        c.set_path_latency_ms(&format!("/c{id}/"), latency);
    }

    fn option(id: i64) -> ChapterRef {
        ChapterRef {
            source_id: format!("s{id}"),
            chapter_id: id,
        }
    }

    #[tokio::test]
    async fn a_quicker_source_scores_higher() {
        let c = FakeCatalog::new();
        add(&c, 1, 10, 150);
        add(&c, 2, 10, 15);
        let slow = probe_source(&c, &option(1), &quick()).await.unwrap();
        let fast = probe_source(&c, &option(2), &quick()).await.unwrap();
        assert!(fast > slow * 3.0, "fast {fast}, slow {slow}");
    }

    #[tokio::test]
    async fn only_the_first_few_pages_are_fetched() {
        let c = FakeCatalog::new();
        add(&c, 1, 40, 1);
        probe_source(&c, &option(1), &quick()).await.unwrap();
        let fetched = c.calls().iter().filter(|x| x.starts_with("get ")).count();
        assert_eq!(fetched, PROBE_PAGES);
    }

    #[tokio::test]
    async fn a_source_that_gives_nothing_usable_scores_nothing() {
        let c = FakeCatalog::new();
        c.set_pages(1, &["/missing/p0", "/missing/p1"]);
        assert_eq!(probe_source(&c, &option(1), &quick()).await, None);
        // A page that is not an image does not count either.
        c.set_pages(2, &["/html/p0"]);
        c.set_bytes("/html/p0", b"<html>blocked</html>".to_vec());
        assert_eq!(probe_source(&c, &option(2), &quick()).await, None);
        // No pages at all.
        c.set_pages(3, &[]);
        assert_eq!(probe_source(&c, &option(3), &quick()).await, None);
    }

    #[tokio::test]
    async fn several_sources_are_probed_at_once_and_reported_by_name() {
        let c = FakeCatalog::new();
        add(&c, 1, 6, 80);
        add(&c, 2, 6, 80);
        let started = Instant::now();
        let out = probe_sources(
            &c,
            &[("a".into(), option(1)), ("b".into(), option(2))],
            &quick(),
        )
        .await;
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "probed one after the other"
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "a");
        assert!(out.iter().all(|(_, s)| s.is_some()));
    }

    #[tokio::test]
    async fn cancelling_a_speed_test_returns_at_once() {
        let c = FakeCatalog::new();
        add(&c, 1, 6, 60_000); // never answers in time
        add(&c, 2, 6, 60_000);
        let cancel = CancelFlag::new();
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            flag.cancel();
        });
        let started = Instant::now();
        let out = probe_sources_cancellable(
            &c,
            &[("a".into(), option(1)), ("b".into(), option(2))],
            &quick(),
            &cancel,
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        assert!(out.is_empty());
        // Already cancelled: nothing is even asked.
        let before = c.calls().len();
        let out = probe_sources_cancellable(&c, &[("a".into(), option(1))], &quick(), &cancel).await;
        assert!(out.is_empty());
        assert_eq!(c.calls().len(), before);
    }

    #[tokio::test]
    async fn a_source_that_never_answers_cannot_hold_the_speed_test_past_its_deadline() {
        let c = FakeCatalog::new();
        add(&c, 1, 6, 1);
        add(&c, 2, 6, 60_000);
        let started = Instant::now();
        let out = probe_with_deadline(
            &c,
            &[("fast".into(), option(1)), ("hung".into(), option(2))],
            &quick(),
            &CancelFlag::new(),
            Duration::from_millis(500),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].0, "fast");
        assert!(out[0].1.is_some());
    }
}
