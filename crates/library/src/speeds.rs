//! How fast each source has been lately, from short speed probes.
//!
//! Sources change: a fast one can start rate limiting, a slow one can recover.
//! So a score is a running average that favors recent measurements, and it
//! goes stale after a few hours, when it is measured again.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A score older than this is measured again before it is trusted.
pub const FRESH_SECONDS: u64 = 6 * 3600;
/// How much a new measurement counts against the running average.
const NEW_WEIGHT: f32 = 0.4;
/// A failed probe cuts the score to this share of what it was.
const FAILURE_KEEPS: f32 = 0.25;
/// What a source that has never worked is worth: practically nothing.
const FLOOR: f32 = 0.05;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceSpeed {
    /// Pages per second the source delivered when asked for a handful at once.
    pub pages_per_second: f32,
    pub samples: u32,
    /// Seconds since the Unix epoch.
    pub measured_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeedDb {
    pub sources: HashMap<String, SourceSpeed>,
}

impl SpeedDb {
    /// Fold in a measurement.
    pub fn record(&mut self, source: &str, pages_per_second: f32, now: u64) {
        let sample = if pages_per_second.is_finite() {
            pages_per_second.max(FLOOR)
        } else {
            FLOOR
        };
        let entry = self
            .sources
            .entry(source.to_string())
            .or_insert(SourceSpeed {
                pages_per_second: sample,
                samples: 0,
                measured_at: now,
            });
        entry.pages_per_second = if entry.samples == 0 {
            sample
        } else {
            entry.pages_per_second * (1.0 - NEW_WEIGHT) + sample * NEW_WEIGHT
        };
        entry.samples += 1;
        entry.measured_at = now;
    }

    /// A probe got nothing from the source: its score drops sharply.
    pub fn record_failure(&mut self, source: &str, now: u64) {
        match self.sources.get_mut(source) {
            Some(entry) => {
                entry.pages_per_second = (entry.pages_per_second * FAILURE_KEEPS).max(FLOOR);
                entry.samples += 1;
                entry.measured_at = now;
            }
            None => self.record(source, FLOOR, now),
        }
    }

    pub fn speed(&self, source: &str) -> Option<f32> {
        self.sources.get(source).map(|s| s.pages_per_second)
    }

    /// Measured recently enough to trust.
    pub fn is_fresh(&self, source: &str, now: u64) -> bool {
        self.sources
            .get(source)
            .is_some_and(|s| now.saturating_sub(s.measured_at) < FRESH_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_measurement_is_taken_as_it_is() {
        let mut db = SpeedDb::default();
        assert_eq!(db.speed("a"), None);
        db.record("a", 12.0, 100);
        assert_eq!(db.speed("a"), Some(12.0));
    }

    #[test]
    fn later_measurements_move_the_score_toward_them_without_jumping() {
        let mut db = SpeedDb::default();
        db.record("a", 10.0, 0);
        db.record("a", 2.0, 10);
        let v = db.speed("a").unwrap();
        assert!((v - 6.8).abs() < 1e-4, "{v}");
        assert!(v > 2.0 && v < 10.0);
    }

    #[test]
    fn a_failed_probe_cuts_the_score_and_never_below_the_floor() {
        let mut db = SpeedDb::default();
        db.record("a", 8.0, 0);
        db.record_failure("a", 5);
        assert_eq!(db.speed("a"), Some(2.0));
        for _ in 0..10 {
            db.record_failure("a", 6);
        }
        assert_eq!(db.speed("a"), Some(FLOOR));
        db.record_failure("never-seen", 7);
        assert_eq!(db.speed("never-seen"), Some(FLOOR));
    }

    #[test]
    fn nonsense_measurements_do_not_poison_the_score() {
        let mut db = SpeedDb::default();
        db.record("a", f32::NAN, 0);
        assert_eq!(db.speed("a"), Some(FLOOR));
        db.record("b", -3.0, 0);
        assert_eq!(db.speed("b"), Some(FLOOR));
    }

    #[test]
    fn scores_go_stale() {
        let mut db = SpeedDb::default();
        db.record("a", 5.0, 1000);
        assert!(db.is_fresh("a", 1000 + FRESH_SECONDS - 1));
        assert!(!db.is_fresh("a", 1000 + FRESH_SECONDS));
        assert!(!db.is_fresh("unknown", 1000));
    }

    #[test]
    fn survives_a_round_trip_and_an_old_empty_file() {
        let mut db = SpeedDb::default();
        db.record("a", 3.5, 9);
        let text = serde_json::to_string(&db).unwrap();
        assert_eq!(serde_json::from_str::<SpeedDb>(&text).unwrap(), db);
        assert_eq!(
            serde_json::from_str::<SpeedDb>("{}").unwrap(),
            SpeedDb::default()
        );
    }
}
