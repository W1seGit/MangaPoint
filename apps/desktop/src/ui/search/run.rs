//! Running a search across the sources and publishing the results as they come in.

use crate::ui::widgets::{cover_key, load_cover_race};
use crate::logic;
use crate::state::now_secs;
use crate::state::spawn_forever;
use crate::state::{AppState, SearchView};
use dioxus::prelude::*;
use futures_util::StreamExt;
use search::fanout::record_health;
use search::{rank_series, search_stream, RankContext, SearchEvent, SearchState};
use sources::SourceManager;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use super::enrich::enrich_remembered;

/// How often a running search looks whether a newer one has replaced it.
const STALE_POLL: Duration = Duration::from_millis(100);

/// Run a search across all installed sources, filling the results in as
/// each source answers.
/// Forget the results and show the start of the Search page again. A search
/// still running is dropped (it stops publishing once the generation moves on).
pub fn clear_search(state: AppState) {
    let mut view = state.search;
    let generation = view.peek().generation + 1;
    view.set(SearchView {
        generation,
        ..Default::default()
    });
}

pub fn run_search(state: AppState, query: String) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let lang = state.settings.read().language.clone();
        let preferred = state.settings.read().preferred_sources.clone();

        let mut view = state.search;
        let generation = state.search.peek().generation + 1;
        view.set(SearchView {
            query: query.clone(),
            searching: true,
            generation,
            ..Default::default()
        });

        // Everything below that waits is given up the moment a newer search
        // starts, so an old one neither keeps the sources busy nor writes over
        // the newer one's results.
        let stale = || !logic::is_current(state.search.peek().generation, generation);
        let Some(installed) = logic::unless_stale(
            SourceManager::new(&*client).installed(&lang),
            stale,
            STALE_POLL,
        )
        .await
        else {
            return;
        };
        let installed = installed.unwrap_or_default();
        let excluded = state.settings.peek().search_excluded_sources.clone();
        let all: Vec<_> = installed.into_iter().map(|i| i.source).collect();
        let none_installed = all.is_empty();
        let sources: Vec<_> = all
            .into_iter()
            .filter(|s| !excluded.contains(&s.id))
            .collect();
        if !none_installed && sources.is_empty() {
            if stale() {
                return;
            }
            view.set(SearchView {
                query,
                none_selected: true,
                generation,
                ..Default::default()
            });
            return;
        }
        if sources.is_empty() {
            if stale() {
                return;
            }
            view.set(SearchView {
                query,
                no_sources: true,
                generation,
                ..Default::default()
            });
            return;
        }

        info!("Searching for \"{query}\" on {} source(s)", sources.len());
        let started = Instant::now();
        let mut search = SearchState::new(query.clone(), sources.len());
        let mut enrichment = HashMap::new();
        let mut stream = Box::pin(search_stream(
            &*client,
            sources,
            query.clone(),
            Duration::from_secs(20),
            8,
        ));
        loop {
            let Some(next) = logic::unless_stale(stream.next(), stale, STALE_POLL).await else {
                info!("Search for \"{query}\" was replaced by a newer search");
                return;
            };
            let Some(event) = next else { break };
            match &event {
                SearchEvent::Done { source, mangas, ms } => {
                    info!("  {}: {} result(s) in {ms} ms", source.name, mangas.len());
                }
                SearchEvent::Failed {
                    source, message, ..
                } => {
                    warn!("  {}: no answer ({message})", source.name);
                }
            }
            search.apply(&event);
            {
                let mut health = state.health;
                record_health(&mut health.write(), &event);
            }
            if !publish(
                state,
                generation,
                &search,
                &query,
                &enrichment,
                &lang,
                &preferred,
                Phase::Asking,
            ) {
                info!("Search for \"{query}\" was replaced by a newer search");
                return;
            }
        }
        drop(stream);
        // Every source has answered: the results are all there to use. What
        // follows only improves which source each one is shown from.
        if !publish(state, generation, &search, &query, &enrichment, &lang, &preferred, Phase::Checking) {
            return;
        }

        // Look at the chapter lists of the best-looking series to rank their sources properly.
        let top: Vec<_> = state
            .search
            .read()
            .ranked
            .iter()
            .take(8)
            .flat_map(|s| s.members.iter().take(3).map(|m| m.candidate.clone()))
            .collect();
        info!(
            "Got {} result(s) in {:.1}s; checking chapter lists to pick the best source for each…",
            search.candidates().len(),
            started.elapsed().as_secs_f32()
        );
        let Some(found) =
            logic::unless_stale(enrich_remembered(&client, &top), stale, STALE_POLL).await
        else {
            return;
        };
        enrichment = found;
        let current = publish(
            state,
            generation,
            &search,
            &query,
            &enrichment,
            &lang,
            &preferred,
            Phase::Done,
        );
        state.save_health();
        if !current {
            return;
        }
        info!(
            "Search finished: {} series in {:.1}s",
            state.search.read().ranked.len(),
            started.elapsed().as_secs_f32()
        );
    });
}

/// How far a search has got.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Sources are still being asked.
    Asking,
    /// Every source has answered; chapter lists are being looked at.
    Checking,
    /// All done.
    Done,
}

/// Show the results so far. Returns false, doing nothing, if a newer search
/// has replaced this one.
#[allow(clippy::too_many_arguments)]
fn publish(
    state: AppState,
    generation: u64,
    search: &SearchState,
    query: &str,
    enrichment: &HashMap<(String, i64), search::Enrichment>,
    lang: &str,
    preferred: &[String],
    phase: Phase,
) -> bool {
    if !logic::is_current(state.search.peek().generation, generation) {
        return false;
    }
    let health = state.health.read().clone();
    let ctx = RankContext {
        health: &health,
        language: lang,
        preferred_sources: preferred,
        now_ms: (now_secs() * 1000) as i64,
        enrichment,
    };
    let ranked = rank_series(search.clusters(), query, &ctx);

    // Point every result at its cover. Nothing is fetched here: the page asks
    // for the covers in view, and when the series is on several sources they
    // are all asked, the first picture to arrive being the one shown.
    let mut ahead: Vec<(String, Vec<String>)> = Vec::new();
    for (position, series) in ranked.iter().enumerate() {
        let best = &series.best().candidate;
        let mut paths: Vec<String> = best.thumbnail.iter().cloned().collect();
        for member in series.members.iter().take(6) {
            if let Some(path) = &member.candidate.thumbnail {
                if !paths.contains(path) {
                    paths.push(path.clone());
                }
            }
        }
        load_cover_race(
            state,
            cover_key(&best.source_id, best.manga_id),
            &library::series_id(&series.title),
            paths.clone(),
        );
        // The first screenfuls are fetched now, while the results are looked at.
        if position < 18 {
            ahead.push((library::series_id(&series.title), paths));
        }
    }
    crate::covers::prefetch(ahead);

    let mut view = state.search;
    view.set(SearchView {
        query: query.to_string(),
        ranked,
        searching: phase == Phase::Asking,
        enriching: phase == Phase::Checking,
        answered: search.answered,
        total: search.total_sources,
        failed: search
            .failures
            .iter()
            .map(|(s, _)| s.name.clone())
            .collect(),
        no_sources: false,
        none_selected: false,
        generation,
    });
    true
}
