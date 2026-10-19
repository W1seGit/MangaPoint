//! How many page downloads may run at once, and backing off when a source
//! pushes back.
//!
//! The limit starts at what the user chose. When a source answers "slow down"
//! (rate limited, forbidden, overloaded, timed out) the limit is halved, so
//! the app stops adding to the problem. After a run of good answers it creeps
//! back up, one at a time, to what the user chose.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// Good answers in a row before the limit rises by one.
const GOOD_RUN: u32 = 20;
/// After slowing down, further complaints within this time are the same
/// burst (requests already in flight) and do not slow things down again.
const SAME_BURST: Duration = Duration::from_secs(2);

struct State {
    limit: usize,
    in_flight: usize,
    good_run: u32,
    last_slowdown: Option<Instant>,
}

pub struct Limiter {
    max: usize,
    state: Mutex<State>,
    freed: Notify,
}

/// Held while one request is in flight. Dropping it makes room for the next.
pub struct Permit<'a> {
    limiter: &'a Limiter,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.limiter.state.lock().unwrap().in_flight -= 1;
        self.limiter.freed.notify_waiters();
    }
}

impl Limiter {
    /// Allow up to `max` requests at once (at least one).
    pub fn new(max: usize) -> Self {
        let max = max.max(1);
        Self {
            max,
            state: Mutex::new(State {
                limit: max,
                in_flight: 0,
                good_run: 0,
                last_slowdown: None,
            }),
            freed: Notify::new(),
        }
    }

    /// The limit right now.
    pub fn current(&self) -> usize {
        self.state.lock().unwrap().limit
    }

    /// Wait for room, then take it.
    pub async fn acquire(&self) -> Permit<'_> {
        loop {
            let notified = self.freed.notified();
            tokio::pin!(notified);
            // Register before checking, so a permit freed in between is not missed.
            notified.as_mut().enable();
            {
                let mut s = self.state.lock().unwrap();
                if s.in_flight < s.limit {
                    s.in_flight += 1;
                    return Permit { limiter: self };
                }
            }
            notified.await;
        }
    }

    /// The source asked us to slow down. Returns the new limit if it changed.
    pub fn slow_down(&self) -> Option<usize> {
        let mut s = self.state.lock().unwrap();
        s.good_run = 0;
        if s.last_slowdown.is_some_and(|t| t.elapsed() < SAME_BURST) {
            return None;
        }
        s.last_slowdown = Some(Instant::now());
        let next = (s.limit / 2).max(1);
        if next == s.limit {
            return None;
        }
        s.limit = next;
        Some(next)
    }

    /// A request succeeded. Returns the new limit if it rose.
    pub fn succeeded(&self) -> Option<usize> {
        let mut s = self.state.lock().unwrap();
        if s.limit >= self.max {
            return None;
        }
        s.good_run += 1;
        if s.good_run < GOOD_RUN {
            return None;
        }
        s.good_run = 0;
        s.limit += 1;
        let limit = s.limit;
        drop(s);
        self.freed.notify_waiters();
        Some(limit)
    }
}

/// Page failures in a row (each already retried) after which a source is
/// given up on for a while.
const BREAKER_AFTER: u32 = 6;
/// How long a source that tripped the breaker is left alone.
const BREAKER_COOLDOWN: Duration = Duration::from_secs(120);

/// Stops a dead source being retried for every page of a batch: after
/// [`BREAKER_AFTER`] failures in a row it is skipped for a cooldown, so the
/// chapter moves on to another source at once.
pub struct Breaker {
    state: Mutex<(u32, Option<Instant>)>,
    /// When the last failure was counted.
    last: Mutex<Option<Instant>>,
    /// Failures closer together than this count as one: the pages of a single
    /// bad chapter all fail at once and must not shut a good source out.
    spacing: Duration,
}

/// How far apart failures must be to count separately.
const BREAKER_SPACING: Duration = Duration::from_millis(1500);

impl Default for Breaker {
    fn default() -> Self {
        Self::with_spacing(BREAKER_SPACING)
    }
}

impl Breaker {
    pub fn with_spacing(spacing: Duration) -> Self {
        Self {
            state: Mutex::new((0, None)),
            last: Mutex::new(None),
            spacing,
        }
    }

    pub fn is_open(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        match s.1 {
            Some(until) if Instant::now() < until => true,
            Some(_) => {
                // Cooled down: one fresh start.
                *s = (0, None);
                false
            }
            None => false,
        }
    }

    pub fn failed(&self) {
        let now = Instant::now();
        {
            let mut last = self.last.lock().unwrap();
            if last.is_some_and(|t| now.duration_since(t) < self.spacing) {
                return;
            }
            *last = Some(now);
        }
        let mut s = self.state.lock().unwrap();
        s.0 += 1;
        if s.0 >= BREAKER_AFTER && s.1.is_none() {
            s.1 = Some(Instant::now() + BREAKER_COOLDOWN);
        }
    }

    pub fn succeeded(&self) {
        let mut s = self.state.lock().unwrap();
        if s.1.is_none() {
            s.0 = 0;
        }
    }

    /// The error given for a request that was not even tried.
    pub fn error() -> catalog::CatalogError {
        catalog::CatalogError::BadResponse(
            "this source kept failing, so it is being skipped for a while".into(),
        )
    }
}

/// The limiters for a download: one shared by every source, or one for each
/// source (so a slow source cannot hold a fast one back, and each is asked for
/// no more than the allowance).
pub struct LimiterSet {
    max: usize,
    per_source: bool,
    limiters: Mutex<HashMap<String, Arc<Limiter>>>,
    breakers: Mutex<HashMap<String, Arc<Breaker>>>,
}

impl LimiterSet {
    pub fn new(max: usize, per_source: bool) -> Self {
        Self {
            max,
            per_source,
            limiters: Mutex::new(HashMap::new()),
            breakers: Mutex::new(HashMap::new()),
        }
    }

    /// The give-up record of `source` (always one per source).
    pub fn breaker(&self, source: &str) -> Arc<Breaker> {
        self.breakers
            .lock()
            .unwrap()
            .entry(source.to_string())
            .or_default()
            .clone()
    }

    /// The limiter requests to `source` go through.
    pub fn get(&self, source: &str) -> Arc<Limiter> {
        let key = if self.per_source { source } else { "" };
        self.limiters
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Limiter::new(self.max)))
            .clone()
    }
}

#[cfg(test)]
mod breaker_tests {
    use super::*;

    #[test]
    fn the_failures_of_one_bad_chapter_at_once_do_not_shut_a_source_out() {
        let b = Breaker::default();
        // Ten pages of one chapter fail in the same instant.
        for _ in 0..10 {
            b.failed();
        }
        assert!(!b.is_open(), "one burst is one failure, not six");
    }

    #[test]
    fn failures_spread_over_time_do_shut_a_dead_source_out() {
        let b = Breaker::with_spacing(Duration::ZERO);
        for _ in 0..BREAKER_AFTER {
            b.failed();
        }
        assert!(b.is_open());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn sources_share_one_allowance_or_each_have_their_own() {
        let shared = LimiterSet::new(4, false);
        assert!(Arc::ptr_eq(&shared.get("a"), &shared.get("b")));
        let own = LimiterSet::new(4, true);
        assert!(!Arc::ptr_eq(&own.get("a"), &own.get("b")));
        assert!(Arc::ptr_eq(&own.get("a"), &own.get("a")));
        own.get("a").slow_down();
        assert_eq!(own.get("a").current(), 2);
        assert_eq!(
            own.get("b").current(),
            4,
            "one source easing off leaves the others alone"
        );
    }

    #[test]
    fn at_least_one_request_is_always_allowed() {
        assert_eq!(Limiter::new(0).current(), 1);
        assert_eq!(Limiter::new(6).current(), 6);
    }

    #[tokio::test]
    async fn never_more_requests_than_the_limit_run_at_once() {
        let limiter = Arc::new(Limiter::new(3));
        let (now, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let (l, now, peak) = (limiter.clone(), now.clone(), peak.clone());
            tasks.push(tokio::spawn(async move {
                let _permit = l.acquire().await;
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                now.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_freed_permit_wakes_a_waiter() {
        let limiter = Arc::new(Limiter::new(1));
        let first = limiter.acquire().await;
        let l = limiter.clone();
        let waiter = tokio::spawn(async move {
            let _p = l.acquire().await;
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!waiter.is_finished());
        drop(first);
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("the waiter is woken")
            .unwrap();
    }

    #[test]
    fn slowing_down_halves_the_limit_but_not_below_one_and_not_twice_in_one_burst() {
        let l = Limiter::new(12);
        assert_eq!(l.slow_down(), Some(6));
        assert_eq!(l.slow_down(), None, "same burst");
        assert_eq!(l.current(), 6);

        let one = Limiter::new(1);
        assert_eq!(one.slow_down(), None);
        assert_eq!(one.current(), 1);
    }

    #[test]
    fn a_run_of_good_answers_brings_the_limit_back_up_one_at_a_time() {
        let l = Limiter::new(4);
        assert_eq!(l.slow_down(), Some(2));
        for _ in 0..GOOD_RUN - 1 {
            assert_eq!(l.succeeded(), None);
        }
        assert_eq!(l.succeeded(), Some(3));
        for _ in 0..GOOD_RUN - 1 {
            l.succeeded();
        }
        assert_eq!(l.succeeded(), Some(4));
        assert_eq!(l.succeeded(), None, "back at the chosen limit");
        assert_eq!(l.current(), 4);
    }

    #[test]
    fn a_complaint_restarts_the_good_run() {
        let l = Limiter::new(4);
        l.slow_down();
        for _ in 0..GOOD_RUN - 1 {
            l.succeeded();
        }
        l.slow_down(); // within the same burst: no change, but the run restarts
        assert_eq!(l.succeeded(), None);
    }
}
