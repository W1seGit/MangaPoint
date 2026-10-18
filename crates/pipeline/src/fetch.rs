//! Downloading page images reliably.

use crate::limiter::{Breaker, Limiter};
use crate::types::CancelFlag;
use catalog::{Catalog, CatalogError};
use std::future::Future;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// Total tries per request, including the first.
    pub attempts: u32,
    /// Wait before the second try; doubles each time (capped at 5 s).
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            base_delay: Duration::from_millis(400),
        }
    }
}

/// Worth trying again? Rate limits, server errors and network hiccups are;
/// a missing page, or a source that needs the browser, is not.
pub fn is_retryable(err: &CatalogError) -> bool {
    match err {
        CatalogError::Http(_) | CatalogError::Io(_) => true,
        CatalogError::SourceHttp(code) => *code == 429 || (500..600).contains(code),
        _ => false,
    }
}

/// Is another try allowed after attempt number `attempt` (1-based) failed
/// with `err`? A request that timed out is tried one more time at most: each
/// timeout can cost minutes, and a source that hangs twice is not going to
/// answer on the third.
fn may_retry(err: &CatalogError, attempt: u32, attempts: u32) -> bool {
    let timed_out = matches!(err, CatalogError::Http(e) if e.is_timeout());
    let allowed = if timed_out { attempts.min(2) } else { attempts };
    attempt < allowed && is_retryable(err)
}

/// Wait `delay`; `false` if the job was cancelled first.
async fn rest(cancel: &CancelFlag, delay: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => true,
        () = cancel.cancelled() => false,
    }
}

/// Run `op` up to `policy.attempts` times, waiting longer after each failure.
pub async fn with_retry<T, F, Fut>(policy: &RetryPolicy, op: F) -> Result<T, CatalogError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CatalogError>>,
{
    with_retry_cancellable(policy, &CancelFlag::new(), op).await
}

/// As [`with_retry`], but a cancel during the wait between tries ends it at
/// once with the last error.
pub async fn with_retry_cancellable<T, F, Fut>(
    policy: &RetryPolicy,
    cancel: &CancelFlag,
    mut op: F,
) -> Result<T, CatalogError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CatalogError>>,
{
    let attempts = policy.attempts.max(1);
    let mut delay = policy.base_delay;
    let mut attempt = 1;
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) if may_retry(&e, attempt, attempts) => {
                if !rest(cancel, delay).await {
                    return Err(e);
                }
                delay = (delay * 2).min(Duration::from_secs(5));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Did the source ask us to ease off (as opposed to simply failing)?
pub fn is_throttle(err: &CatalogError) -> bool {
    match err {
        CatalogError::SourceHttp(code) => matches!(code, 403 | 429 | 503),
        CatalogError::Http(e) => e.is_timeout(),
        _ => false,
    }
}

/// Fetch a page within the limiter's allowance, retrying like [`with_retry`].
/// A "slow down" answer lowers the allowance; `on_limit` is told the new
/// number whenever it changes.
pub async fn fetch_page_limited<C: Catalog>(
    catalog: &C,
    path: &str,
    policy: &RetryPolicy,
    limiter: &Limiter,
    breaker: &Breaker,
    cancel: &CancelFlag,
    on_limit: &(dyn Fn(usize) + Sync),
) -> Result<Vec<u8>, CatalogError> {
    if breaker.is_open() {
        return Err(Breaker::error());
    }
    let attempts = policy.attempts.max(1);
    let mut delay = policy.base_delay;
    let mut attempt = 1;
    loop {
        // The permit is held only while the request runs, not while waiting to retry.
        let result = {
            let _permit = limiter.acquire().await;
            catalog.get_bytes(path).await
        };
        match result {
            Ok(bytes) => {
                breaker.succeeded();
                if let Some(limit) = limiter.succeeded() {
                    on_limit(limit);
                }
                return Ok(bytes);
            }
            Err(e) => {
                let throttled = is_throttle(&e);
                if throttled {
                    if let Some(limit) = limiter.slow_down() {
                        on_limit(limit);
                    }
                }
                if !may_retry(&e, attempt, attempts) {
                    // Only a source that is failing counts against it, not
                    // a page that is simply gone.
                    if is_retryable(&e) {
                        breaker.failed();
                    }
                    return Err(e);
                }
                if breaker.is_open() {
                    return Err(e);
                }
                // A source that is pushing back gets a longer rest.
                let pause = if throttled {
                    delay.max(Duration::from_secs(1))
                } else {
                    delay
                };
                if !self::rest(cancel, pause).await {
                    return Err(e);
                }
                delay = (delay * 2).min(Duration::from_secs(5));
                attempt += 1;
            }
        }
    }
}

pub async fn fetch_page<C: Catalog>(
    catalog: &C,
    path: &str,
    policy: &RetryPolicy,
) -> Result<Vec<u8>, CatalogError> {
    with_retry(policy, || catalog.get_bytes(path)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::FakeCatalog;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn quick(attempts: u32) -> RetryPolicy {
        RetryPolicy {
            attempts,
            base_delay: Duration::from_millis(1),
        }
    }

    #[tokio::test]
    async fn retries_transient_errors_until_they_clear() {
        let c = FakeCatalog::new();
        c.set_bytes("/p", vec![1, 2, 3]);
        c.flaky_path("/p", 2); // fails twice, then works
        assert_eq!(
            fetch_page(&c, "/p", &quick(3)).await.unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(c.calls().iter().filter(|x| *x == "get /p").count(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_the_last_attempt() {
        let c = FakeCatalog::new();
        c.set_bytes("/p", vec![1]);
        c.flaky_path("/p", 5);
        let err = fetch_page(&c, "/p", &quick(3)).await.unwrap_err();
        assert!(matches!(err, CatalogError::SourceHttp(503)));
        assert_eq!(c.calls().iter().filter(|x| *x == "get /p").count(), 3);
    }

    #[tokio::test]
    async fn does_not_retry_a_missing_page() {
        let c = FakeCatalog::new();
        let err = fetch_page(&c, "/gone", &quick(5)).await.unwrap_err();
        assert!(matches!(err, CatalogError::SourceHttp(404)));
        assert_eq!(c.calls().len(), 1, "404 is final");
    }

    #[test]
    fn retry_classification() {
        assert!(is_retryable(&CatalogError::SourceHttp(503)));
        assert!(is_retryable(&CatalogError::SourceHttp(429)));
        assert!(!is_retryable(&CatalogError::SourceHttp(404)));
        assert!(!is_retryable(&CatalogError::SourceHttp(403)));
        assert!(!is_retryable(&CatalogError::NeedsBrowser));
        assert!(!is_retryable(&CatalogError::NeedsFlareSolverr));
    }

    #[tokio::test]
    async fn backoff_waits_between_attempts() {
        let calls = AtomicU32::new(0);
        let policy = RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(20),
        };
        let start = std::time::Instant::now();
        let r: Result<(), CatalogError> = with_retry(&policy, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(CatalogError::SourceHttp(500)) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // 20 ms then 40 ms.
        assert!(
            start.elapsed() >= Duration::from_millis(55),
            "{:?}",
            start.elapsed()
        );
    }

    /// A real "timed out" error: a server that never answers, a short timeout.
    async fn timeout_error() -> CatalogError {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(40))
            .build()
            .unwrap();
        let e = client.get(url).send().await.unwrap_err();
        assert!(e.is_timeout());
        CatalogError::Http(e)
    }

    #[tokio::test]
    async fn a_timeout_is_retried_only_once() {
        let calls = AtomicU32::new(0);
        let r = with_retry(&quick(5), || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<(), _>(timeout_error().await) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelling_ends_the_wait_between_tries() {
        let policy = RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_secs(10),
        };
        let cancel = CancelFlag::new();
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            flag.cancel();
        });
        let started = std::time::Instant::now();
        let r: Result<(), CatalogError> =
            with_retry_cancellable(&policy, &cancel, || async { Err(CatalogError::SourceHttp(500)) })
                .await;
        assert!(r.is_err());
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn a_source_that_keeps_failing_is_given_up_on() {
        let c = FakeCatalog::new();
        c.set_bytes("/p", vec![1]);
        c.flaky_path("/p", 1000);
        let limiter = Limiter::new(2);
        let breaker = Breaker::with_spacing(Duration::ZERO);
        let cancel = CancelFlag::new();
        for _ in 0..20 {
            let _ = fetch_page_limited(&c, "/p", &quick(1), &limiter, &breaker, &cancel, &|_| {}).await;
        }
        let tried = c.calls().iter().filter(|x| *x == "get /p").count();
        assert_eq!(tried, 6, "after six failures in a row the rest are not even tried");
        let err = fetch_page_limited(&c, "/p", &quick(1), &limiter, &breaker, &cancel, &|_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, CatalogError::BadResponse(_)));
    }

    #[tokio::test]
    async fn missing_pages_do_not_count_against_a_source() {
        let c = FakeCatalog::new();
        let (limiter, breaker, cancel) = (Limiter::new(2), Breaker::default(), CancelFlag::new());
        for _ in 0..20 {
            let _ = fetch_page_limited(&c, "/gone", &quick(1), &limiter, &breaker, &cancel, &|_| {}).await;
        }
        assert_eq!(c.calls().len(), 20);
    }
}
