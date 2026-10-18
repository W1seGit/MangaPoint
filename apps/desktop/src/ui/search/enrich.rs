//! What is remembered of the chapter lists of series already looked at.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What was learned of the chapter lists of series already looked at, for a
/// while: searching for the same thing again (or something that turns up the
/// same series) does not ask the sources a second time.
type Remembered = HashMap<(String, i64), (Instant, search::Enrichment)>;
static ENRICHED: std::sync::Mutex<Option<Remembered>> = std::sync::Mutex::new(None);

/// How long what was learned is trusted.
const ENRICHED_FOR: Duration = Duration::from_secs(20 * 60);

/// The chapter lists of `candidates`, from what is remembered where it is fresh
/// and from the sources for the rest.
pub(super) async fn enrich_remembered(
    client: &catalog::SuwayomiClient,
    candidates: &[search::Candidate],
) -> HashMap<(String, i64), search::Enrichment> {
    let mut found = HashMap::new();
    let mut missing = Vec::new();
    {
        let cache = ENRICHED.lock().ok();
        let known = cache.as_ref().and_then(|c| c.as_ref());
        for candidate in candidates {
            match known.and_then(|k| k.get(&candidate.key())) {
                Some((at, enrichment)) if at.elapsed() < ENRICHED_FOR => {
                    found.insert(candidate.key(), enrichment.clone());
                }
                _ => missing.push(candidate.clone()),
            }
        }
    }
    // Eight lookups at once, and no source is waited on for long: a series
    // whose list is slow keeps what it had.
    let fetched = search::enrich(client, &missing, Duration::from_secs(10), 8).await;
    if let Ok(mut cache) = ENRICHED.lock() {
        let map = cache.get_or_insert_with(HashMap::new);
        for (key, enrichment) in &fetched {
            map.insert(key.clone(), (Instant::now(), enrichment.clone()));
        }
    }
    found.extend(fetched);
    found
}
