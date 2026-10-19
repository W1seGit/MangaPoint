//! Trying something again later, but not at once and not forever fast: for
//! fetching a series' details, which is asked for again whenever the library
//! changes.

use std::collections::HashMap;

/// First wait after a failure, doubled for each further one.
const FIRST_WAIT_SECS: u64 = 30;
/// The longest wait.
const LONGEST_WAIT_SECS: u64 = 30 * 60;

/// When each thing may be tried again.
#[derive(Debug, Default)]
pub struct Backoff {
    /// Failures so far and the time before which it is not tried again.
    failed: HashMap<String, (u32, u64)>,
}

impl Backoff {
    pub fn new() -> Self {
        Self {
            failed: HashMap::new(),
        }
    }

    /// May `id` be tried at `now` (seconds)?
    pub fn may_try(&self, id: &str, now: u64) -> bool {
        self.failed.get(id).is_none_or(|&(_, not_before)| now >= not_before)
    }

    /// The attempt at `id` failed at `now`.
    pub fn failed(&mut self, id: &str, now: u64) {
        let entry = self.failed.entry(id.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        let wait = FIRST_WAIT_SECS
            .saturating_mul(1u64 << (entry.0 - 1).min(16))
            .min(LONGEST_WAIT_SECS);
        entry.1 = now + wait;
    }

    /// `id` worked: forget its failures.
    pub fn succeeded(&mut self, id: &str) {
        self.failed.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_holds_off_the_next_try_for_longer_each_time_up_to_a_limit() {
        let mut b = Backoff::new();
        assert!(b.may_try("a", 0), "never tried");
        b.failed("a", 1000);
        assert!(!b.may_try("a", 1000), "not at once");
        assert!(!b.may_try("a", 1029));
        assert!(b.may_try("a", 1030));
        b.failed("a", 1030);
        assert!(!b.may_try("a", 1089), "the wait doubled");
        assert!(b.may_try("a", 1090));
        for _ in 0..30 {
            b.failed("a", 0);
        }
        assert!(!b.may_try("a", LONGEST_WAIT_SECS - 1));
        assert!(b.may_try("a", LONGEST_WAIT_SECS), "never longer than the limit");
    }

    #[test]
    fn series_are_held_off_separately_and_success_forgets_failures() {
        let mut b = Backoff::new();
        b.failed("a", 0);
        assert!(b.may_try("b", 1));
        assert!(!b.may_try("a", 1));
        b.succeeded("a");
        assert!(b.may_try("a", 1));
    }
}
