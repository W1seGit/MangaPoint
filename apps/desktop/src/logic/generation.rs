//! Telling a newer piece of work from an older one that is still running.

use std::future::Future;
use std::time::Duration;

/// Is work started as number `mine` still the latest, given the latest number?
pub fn is_current(latest: u64, mine: u64) -> bool {
    latest == mine
}

/// Run `work`, but give it up (dropping it, so whatever it was waiting on
/// stops being polled) as soon as `is_stale` says so. `None` means it was
/// given up. `is_stale` is looked at first, and then every `poll`.
pub async fn unless_stale<T>(
    work: impl Future<Output = T>,
    is_stale: impl Fn() -> bool,
    poll: Duration,
) -> Option<T> {
    if is_stale() {
        return None;
    }
    let watch = async {
        loop {
            tokio::time::sleep(poll).await;
            if is_stale() {
                return;
            }
        }
    };
    tokio::select! {
        out = work => Some(out),
        () = watch => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn only_the_latest_generation_is_current() {
        assert!(is_current(3, 3));
        assert!(!is_current(4, 3));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn work_that_is_replaced_is_dropped_not_run_to_the_end() {
        struct Flag<'a>(&'a Cell<bool>);
        impl Drop for Flag<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let latest = Cell::new(1u64);
        let dropped = Cell::new(false);
        let finished = Cell::new(false);
        let slow = async {
            let _flag = Flag(&dropped);
            tokio::time::sleep(Duration::from_secs(30)).await;
            finished.set(true);
        };
        let replace = async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            latest.set(2);
        };
        let started = std::time::Instant::now();
        let (out, ()) = tokio::join!(
            unless_stale(slow, || !is_current(latest.get(), 1), Duration::from_millis(10)),
            replace
        );
        assert!(out.is_none());
        assert!(dropped.get() && !finished.get());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn work_that_is_not_replaced_finishes_and_a_stale_start_never_begins() {
        let out = unless_stale(async { 7 }, || false, Duration::from_millis(10)).await;
        assert_eq!(out, Some(7));
        let ran = Cell::new(false);
        let out = unless_stale(async { ran.set(true) }, || true, Duration::from_millis(10)).await;
        assert!(out.is_none() && !ran.get());
    }
}
