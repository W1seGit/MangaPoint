//! Asking every source at once and collecting what comes back.

use crate::cluster::{cluster, Candidate, Cluster};
use crate::rank::Enrichment;
use catalog::{Browse, Catalog, CatalogError, MangaSummary, Source};
use futures_util::stream::{self, Stream, StreamExt};
use library::HealthDb;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Why a source gave no results, in terms the UI can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    NeedsBrowser,
    NeedsFlareSolverr,
    Timeout,
    Other,
}

#[derive(Clone, Debug)]
pub enum SearchEvent {
    Done {
        source: Source,
        mangas: Vec<MangaSummary>,
        ms: u64,
    },
    Failed {
        source: Source,
        kind: FailKind,
        message: String,
        ms: u64,
    },
}

/// Search `sources` in parallel (at most `concurrency` at a time), yielding
/// one event per source as soon as it answers. Slow sources are cut off after
/// `per_source_timeout`, so one dead site never holds up the rest.
pub fn search_stream<'a, C: Catalog>(
    catalog: &'a C,
    sources: Vec<Source>,
    query: String,
    per_source_timeout: Duration,
    concurrency: usize,
) -> impl Stream<Item = SearchEvent> + 'a {
    stream::iter(sources)
        .map(move |source| {
            let query = query.clone();
            async move {
                let start = Instant::now();
                let result = tokio::time::timeout(
                    per_source_timeout,
                    catalog.browse(&source.id, &Browse::Search(query), 1),
                )
                .await;
                let ms = start.elapsed().as_millis() as u64;
                match result {
                    Ok(Ok(page)) => SearchEvent::Done {
                        source,
                        mangas: page.mangas,
                        ms,
                    },
                    Ok(Err(err)) => SearchEvent::Failed {
                        kind: fail_kind(&err),
                        message: err.to_string(),
                        source,
                        ms,
                    },
                    Err(_) => SearchEvent::Failed {
                        kind: FailKind::Timeout,
                        message: "the source took too long to answer".into(),
                        source,
                        ms,
                    },
                }
            }
        })
        .buffer_unordered(concurrency.max(1))
}

fn fail_kind(err: &CatalogError) -> FailKind {
    match err {
        CatalogError::NeedsBrowser => FailKind::NeedsBrowser,
        CatalogError::NeedsFlareSolverr => FailKind::NeedsFlareSolverr,
        _ => FailKind::Other,
    }
}

/// Everything received so far for one search.
#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    candidates: Vec<Candidate>,
    pub total_sources: usize,
    pub answered: usize,
    pub failures: Vec<(Source, FailKind)>,
}

impl SearchState {
    pub fn new(query: impl Into<String>, total_sources: usize) -> Self {
        Self {
            query: query.into(),
            total_sources,
            ..Default::default()
        }
    }

    pub fn apply(&mut self, event: &SearchEvent) {
        self.answered += 1;
        match event {
            SearchEvent::Done { source, mangas, .. } => {
                self.candidates
                    .extend(mangas.iter().map(|m| Candidate::new(source, m)));
            }
            SearchEvent::Failed { source, kind, .. } => {
                self.failures.push((source.clone(), *kind));
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.answered >= self.total_sources
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// Series found so far. Cheap enough to call after every event.
    pub fn clusters(&self) -> Vec<Cluster> {
        cluster(&self.candidates)
    }
}

/// Feed a search event into the health record: real use is the best signal
/// for how reliable a source is. Sources that need the browser or
/// FlareSolverr are left to the explicit tests.
pub fn record_health(health: &mut HealthDb, event: &SearchEvent) {
    match event {
        SearchEvent::Done { source, ms, .. } => health.record_use(&source.id, true, *ms),
        SearchEvent::Failed {
            source, kind, ms, ..
        } => {
            if !matches!(kind, FailKind::NeedsBrowser | FailKind::NeedsFlareSolverr) {
                health.record_use(&source.id, false, *ms);
            }
        }
    }
}

/// Look at each candidate's chapter list to learn how complete and current
/// it is. Candidates whose lookup fails are simply left out.
pub async fn enrich<C: Catalog>(
    catalog: &C,
    candidates: &[Candidate],
    timeout: Duration,
    concurrency: usize,
) -> HashMap<(String, i64), Enrichment> {
    stream::iter(candidates)
        .map(|cand| async move {
            let result = tokio::time::timeout(timeout, catalog.fetch_chapters(cand.manga_id)).await;
            match result {
                Ok(Ok(chapters)) => Some((
                    cand.key(),
                    Enrichment {
                        chapter_count: Some(chapters.len() as u32),
                        latest_upload_ms: chapters
                            .iter()
                            .map(|c| c.upload_date)
                            .filter(|&d| d > 0)
                            .max(),
                    },
                )),
                _ => None,
            }
        })
        .buffer_unordered(concurrency.max(1))
        .filter_map(|x| async move { x })
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::{chapter, FakeCatalog, Fault};

    fn source(id: &str) -> Source {
        Source {
            id: id.into(),
            name: format!("Source {id}"),
            lang: "en".into(),
            is_nsfw: false,
            extension_pkg: None,
        }
    }

    fn catalog() -> FakeCatalog {
        let c = FakeCatalog::new();
        c.add_manga("a", 1, "Berserk");
        c.add_manga("a", 2, "Berserk of Gluttony");
        c.add_manga("b", 3, "BERSERK");
        c.add_manga("c", 4, "Berserk");
        c.set_chapters(1, vec![chapter(10, 1.0, 1_000), chapter(11, 2.0, 9_000)]);
        c.set_chapters(3, vec![chapter(30, 1.0, 5_000)]);
        c
    }

    async fn run(c: &FakeCatalog, ids: &[&str]) -> (SearchState, Vec<SearchEvent>) {
        let sources: Vec<Source> = ids.iter().map(|i| source(i)).collect();
        let mut state = SearchState::new("berserk", sources.len());
        let mut events = Vec::new();
        let mut s = Box::pin(search_stream(
            c,
            sources,
            "berserk".into(),
            Duration::from_secs(5),
            4,
        ));
        while let Some(ev) = s.next().await {
            state.apply(&ev);
            events.push(ev);
        }
        (state, events)
    }

    #[tokio::test]
    async fn every_source_is_asked_and_results_are_grouped_into_series() {
        let c = catalog();
        let (state, events) = run(&c, &["a", "b", "c"]).await;
        assert_eq!(events.len(), 3);
        assert!(state.is_finished());
        assert_eq!(state.candidates().len(), 4);
        let clusters = state.clusters();
        assert_eq!(
            clusters.len(),
            2,
            "Berserk (3 sources) and Berserk of Gluttony"
        );
        let main = clusters.iter().find(|c| c.members.len() == 3).unwrap();
        assert_eq!(main.title, "Berserk");
    }

    #[tokio::test]
    async fn one_failing_source_does_not_stop_the_others() {
        let c = catalog();
        c.fail_source("b", Fault::Http(500));
        c.fail_source("c", Fault::NeedsBrowser);
        let (state, _) = run(&c, &["a", "b", "c"]).await;
        assert!(state.is_finished());
        assert_eq!(state.candidates().len(), 2); // only source a answered
        let kinds: Vec<_> = state
            .failures
            .iter()
            .map(|(s, k)| (s.id.as_str(), *k))
            .collect();
        assert!(kinds.contains(&("b", FailKind::Other)));
        assert!(kinds.contains(&("c", FailKind::NeedsBrowser)));
    }

    #[tokio::test]
    async fn partial_state_is_usable_before_the_search_finishes() {
        let c = catalog();
        let mut state = SearchState::new("berserk", 3);
        let mut s = Box::pin(search_stream(
            &c,
            vec![source("a"), source("b"), source("c")],
            "berserk".into(),
            Duration::from_secs(5),
            1,
        ));
        let first = s.next().await.unwrap();
        state.apply(&first);
        assert!(!state.is_finished());
        assert!(!state.clusters().is_empty());
    }

    #[tokio::test]
    async fn health_records_real_use_but_not_browser_problems() {
        let c = catalog();
        c.fail_source("b", Fault::Http(500));
        c.fail_source("c", Fault::NeedsFlareSolverr);
        let (_, events) = run(&c, &["a", "b", "c"]).await;
        let mut h = HealthDb::default();
        for ev in &events {
            record_health(&mut h, ev);
        }
        assert_eq!(h.sources["a"].use_success, Some(1.0));
        assert_eq!(h.sources["b"].use_success, Some(0.0));
        assert!(!h.sources.contains_key("c"));
    }

    #[tokio::test]
    async fn enrichment_counts_chapters_and_finds_the_newest_upload() {
        let c = catalog();
        let state = run(&c, &["a", "b", "c"]).await.0;
        let map = enrich(&c, state.candidates(), Duration::from_secs(5), 4).await;
        let a = &map[&("a".to_string(), 1)];
        assert_eq!(a.chapter_count, Some(2));
        assert_eq!(a.latest_upload_ms, Some(9_000));
        assert_eq!(map[&("b".to_string(), 3)].chapter_count, Some(1));
        // Candidate 4 has no chapters registered: counted as zero, not missing.
        assert_eq!(map[&("c".to_string(), 4)].chapter_count, Some(0));
        assert_eq!(map[&("c".to_string(), 4)].latest_upload_ms, None);
    }

    #[tokio::test]
    async fn enrichment_skips_candidates_whose_lookup_fails() {
        let c = catalog();
        let state = run(&c, &["a", "b", "c"]).await.0;
        c.fail_source("b", Fault::Http(500));
        let map = enrich(&c, state.candidates(), Duration::from_secs(5), 4).await;
        assert!(!map.contains_key(&("b".to_string(), 3)));
        assert!(map.contains_key(&("a".to_string(), 1)));
    }
}
