//! How well each source has been working: explicit tests (the Sources page's
//! "Retest") and real use (searches and downloads). Search ranking reads the
//! combined [`HealthDb::score`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Outcome of a source test.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    /// Works only with browser support turned on.
    NeedsBrowser,
    /// Needs a FlareSolverr service for Cloudflare.
    NeedsFlareSolverr,
    #[default]
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StepTiming {
    pub name: String,
    pub ms: u64,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestRecord {
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub verdict: Verdict,
    pub steps: Vec<StepTiming>,
    /// Plain-language summary for the UI.
    pub message: String,
}

impl TestRecord {
    pub fn total_ms(&self) -> u64 {
        self.steps.iter().map(|s| s.ms).sum()
    }
}

const HISTORY_LIMIT: usize = 20;
/// Weight of a new observation in the moving averages.
const ALPHA: f32 = 0.2;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceRecord {
    pub name: String,
    /// Newest last.
    pub history: Vec<TestRecord>,
    /// Moving average of real-use success (1.0 = always works). `None` until used.
    pub use_success: Option<f32>,
    /// Moving average of real-use latency in milliseconds.
    pub use_ms: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthDb {
    /// Keyed by Suwayomi source id.
    pub sources: BTreeMap<String, SourceRecord>,
}

impl HealthDb {
    pub fn record_test(&mut self, source_id: &str, name: &str, record: TestRecord) {
        let entry = self.sources.entry(source_id.to_string()).or_default();
        entry.name = name.to_string();
        entry.history.push(record);
        let excess = entry.history.len().saturating_sub(HISTORY_LIMIT);
        entry.history.drain(..excess);
    }

    /// Note the result of a real search/download against `source_id`.
    pub fn record_use(&mut self, source_id: &str, ok: bool, ms: u64) {
        let entry = self.sources.entry(source_id.to_string()).or_default();
        let ema = |old: Option<f32>, new: f32| Some(old.map_or(new, |o| o + ALPHA * (new - o)));
        entry.use_success = ema(entry.use_success, if ok { 1.0 } else { 0.0 });
        if ok {
            entry.use_ms = ema(entry.use_ms, ms as f32);
        }
    }

    pub fn latest(&self, source_id: &str) -> Option<&TestRecord> {
        self.sources.get(source_id)?.history.last()
    }

    /// 0.0 (unusable) to 1.0 (reliable); 0.5 when nothing is known.
    ///
    /// A source whose latest test says it needs a browser or FlareSolverr
    /// scores 0: it cannot be used as things are configured.
    pub fn score(&self, source_id: &str) -> f32 {
        let Some(rec) = self.sources.get(source_id) else {
            return 0.5;
        };
        if matches!(
            rec.history.last().map(|r| r.verdict),
            Some(Verdict::NeedsBrowser | Verdict::NeedsFlareSolverr)
        ) {
            return 0.0;
        }
        let recent: Vec<&TestRecord> = rec.history.iter().rev().take(5).collect();
        let tests = (!recent.is_empty()).then(|| {
            recent.iter().filter(|r| r.verdict == Verdict::Pass).count() as f32
                / recent.len() as f32
        });
        match (tests, rec.use_success) {
            (Some(t), Some(u)) => 0.6 * t + 0.4 * u,
            (Some(t), None) => t,
            (None, Some(u)) => u,
            (None, None) => 0.5,
        }
    }

    /// Typical real-use latency, if known; falls back to the last passing test.
    pub fn typical_ms(&self, source_id: &str) -> Option<f32> {
        let rec = self.sources.get(source_id)?;
        rec.use_ms.or_else(|| {
            rec.history
                .iter()
                .rev()
                .find(|r| r.verdict == Verdict::Pass)
                .map(|r| r.total_ms() as f32)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(verdict: Verdict, ms: u64) -> TestRecord {
        TestRecord {
            at: 0,
            verdict,
            steps: vec![StepTiming {
                name: "all".into(),
                ms,
                ok: verdict == Verdict::Pass,
                detail: None,
            }],
            message: String::new(),
        }
    }

    #[test]
    fn unknown_sources_are_neutral() {
        assert_eq!(HealthDb::default().score("x"), 0.5);
    }

    #[test]
    fn score_follows_recent_tests() {
        let mut db = HealthDb::default();
        for _ in 0..4 {
            db.record_test("s", "S", rec(Verdict::Pass, 100));
        }
        assert_eq!(db.score("s"), 1.0);
        db.record_test("s", "S", rec(Verdict::Failed, 100));
        assert!((db.score("s") - 0.8).abs() < 1e-6); // 4 of the last 5
    }

    #[test]
    fn sources_needing_browser_or_flaresolverr_score_zero() {
        let mut db = HealthDb::default();
        db.record_test("a", "A", rec(Verdict::Pass, 1));
        db.record_test("a", "A", rec(Verdict::NeedsBrowser, 1));
        assert_eq!(db.score("a"), 0.0);
        db.record_test("b", "B", rec(Verdict::NeedsFlareSolverr, 1));
        assert_eq!(db.score("b"), 0.0);
        // Fixing it (browser turned on) and retesting recovers the score.
        db.record_test("a", "A", rec(Verdict::Pass, 1));
        assert!(db.score("a") > 0.0);
    }

    #[test]
    fn real_use_moves_the_score_and_latency() {
        let mut db = HealthDb::default();
        db.record_test("s", "S", rec(Verdict::Pass, 500));
        let before = db.score("s");
        for _ in 0..10 {
            db.record_use("s", false, 0);
        }
        assert!(db.score("s") < before);
        db.record_use("s", true, 200);
        assert!(db.typical_ms("s").unwrap() > 0.0);
    }

    #[test]
    fn latency_falls_back_to_the_last_passing_test() {
        let mut db = HealthDb::default();
        db.record_test("s", "S", rec(Verdict::Pass, 700));
        db.record_test("s", "S", rec(Verdict::Failed, 9));
        assert_eq!(db.typical_ms("s"), Some(700.0));
        assert_eq!(db.typical_ms("unknown"), None);
    }

    #[test]
    fn history_is_capped() {
        let mut db = HealthDb::default();
        for i in 0..50 {
            db.record_test("s", "S", rec(Verdict::Pass, i));
        }
        assert_eq!(db.sources["s"].history.len(), HISTORY_LIMIT);
        assert_eq!(db.latest("s").unwrap().total_ms(), 49);
    }
}
