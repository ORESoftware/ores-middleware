use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Notify;

pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(5);
/// Canonical ORES admission status while an HTTP server is draining.
pub const SHUTDOWN_HTTP_STATUS: u16 = 429;

const PHASE_RUNNING: u8 = 0;
const PHASE_DRAINING: u8 = 1;
const PHASE_FORCED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownPhase {
    Running,
    Draining,
    Forced,
}

#[derive(Clone, Debug)]
pub struct ShutdownCoordinator {
    inner: Arc<ShutdownInner>,
}

#[derive(Debug)]
struct ShutdownInner {
    phase: AtomicU8,
    active_requests: AtomicUsize,
    changed: Notify,
    drain_timeout: Duration,
    retry_after: Duration,
}

#[derive(Debug)]
pub struct DrainGuard {
    inner: Option<Arc<ShutdownInner>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownRejection {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
    /// End-to-end response metadata only. Protocol-specific hop-by-hop headers
    /// such as `Connection` belong to the framework/transport adapter.
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainOutcome {
    Drained { active_at_start: usize },
    TimedOut { remaining: usize },
    Forced { remaining: usize },
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new(DEFAULT_DRAIN_TIMEOUT)
    }
}

impl ShutdownCoordinator {
    #[must_use]
    pub fn new(drain_timeout: Duration) -> Self {
        Self::with_retry_after(drain_timeout, DEFAULT_RETRY_AFTER)
    }

    #[must_use]
    pub fn with_retry_after(drain_timeout: Duration, retry_after: Duration) -> Self {
        Self {
            inner: Arc::new(ShutdownInner {
                phase: AtomicU8::new(PHASE_RUNNING),
                active_requests: AtomicUsize::new(0),
                changed: Notify::new(),
                drain_timeout,
                retry_after,
            }),
        }
    }

    #[must_use]
    pub fn phase(&self) -> ShutdownPhase {
        phase_from_raw(self.inner.phase.load(Ordering::SeqCst))
    }

    #[must_use]
    pub fn active_requests(&self) -> usize {
        self.inner.active_requests.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn is_accepting_requests(&self) -> bool {
        self.phase() == ShutdownPhase::Running
    }

    /// Enter the in-flight request set while the server is still running.
    ///
    /// The phase is checked both before and after incrementing the active count.
    /// That second check closes the race where draining starts between the first
    /// phase read and request registration: a request that loses that race is
    /// removed from the count and rejected rather than silently admitted.
    /// Sequential consistency is intentional here: admission and the drain
    /// transition touch separate atomics, and the total order prevents a drain
    /// from observing zero after a request has already won admission.
    pub fn begin_request(&self) -> Result<DrainGuard, ShutdownRejection> {
        if self.phase() != ShutdownPhase::Running {
            return Err(self.rejection());
        }

        self.inner.active_requests.fetch_add(1, Ordering::SeqCst);
        if self.phase() == ShutdownPhase::Running {
            return Ok(DrainGuard {
                inner: Some(Arc::clone(&self.inner)),
            });
        }

        release_request(&self.inner);
        Err(self.rejection())
    }

    /// Atomically transition from accepting traffic to draining.
    ///
    /// Returns true only for the caller that performs the first transition.
    /// Consumers should use that edge to stop accepting new connections at the
    /// listener or ingress boundary. The request admission guard remains useful
    /// for keep-alive connections and races already inside the process.
    pub fn start_draining(&self) -> bool {
        let changed = self
            .inner
            .phase
            .compare_exchange(
                PHASE_RUNNING,
                PHASE_DRAINING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok();
        if changed {
            self.inner.changed.notify_waiters();
        }
        changed
    }

    /// Escalate immediately to forced shutdown, for example on a second signal.
    pub fn force_shutdown(&self) -> bool {
        let previous = self.inner.phase.swap(PHASE_FORCED, Ordering::SeqCst);
        self.inner.changed.notify_waiters();
        previous != PHASE_FORCED
    }

    /// Start draining and wait up to the configured deadline for existing
    /// requests. On timeout the coordinator enters Forced state so the caller
    /// can terminate remaining connection/request tasks deterministically.
    pub async fn drain(&self) -> DrainOutcome {
        self.start_draining();
        let active_at_start = self.active_requests();

        if self.phase() == ShutdownPhase::Forced {
            return DrainOutcome::Forced {
                remaining: active_at_start,
            };
        }
        if active_at_start == 0 {
            return DrainOutcome::Drained { active_at_start };
        }

        let wait_for_zero_or_force = async {
            loop {
                if self.active_requests() == 0 || self.phase() == ShutdownPhase::Forced {
                    return;
                }

                // Register before the second observation so a request finishing
                // between the check and await cannot produce a lost wake-up.
                let changed = self.inner.changed.notified();
                if self.active_requests() == 0 || self.phase() == ShutdownPhase::Forced {
                    return;
                }
                changed.await;
            }
        };

        match tokio::time::timeout(self.inner.drain_timeout, wait_for_zero_or_force).await {
            Ok(()) if self.active_requests() == 0 => DrainOutcome::Drained { active_at_start },
            Ok(()) => DrainOutcome::Forced {
                remaining: self.active_requests(),
            },
            Err(_) => {
                let remaining = self.active_requests();
                self.force_shutdown();
                DrainOutcome::TimedOut { remaining }
            }
        }
    }

    #[must_use]
    pub fn rejection(&self) -> ShutdownRejection {
        let whole_seconds = self.inner.retry_after.as_secs();
        let retry_after_seconds = whole_seconds
            .saturating_add(u64::from(self.inner.retry_after.subsec_nanos() != 0))
            .max(1);
        ShutdownRejection {
            status: SHUTDOWN_HTTP_STATUS,
            code: "service_draining",
            message: "service is draining and is not accepting new requests",
            headers: BTreeMap::from([(
                "retry-after".to_owned(),
                retry_after_seconds.to_string(),
            )]),
        }
    }
}

impl DrainGuard {
    /// Finish explicitly when a request lifecycle has a natural completion
    /// point. Dropping the guard has the same effect and is panic/cancellation
    /// safe for ordinary Rust task unwinding.
    pub fn finish(mut self) {
        if let Some(inner) = self.inner.take() {
            release_request(&inner);
        }
    }
}

impl Drop for DrainGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            release_request(&inner);
        }
    }
}

fn release_request(inner: &ShutdownInner) {
    let previous = inner.active_requests.fetch_sub(1, Ordering::SeqCst);
    debug_assert!(previous > 0, "request drain guard underflow");
    if previous <= 1 {
        inner.changed.notify_waiters();
    }
}

fn phase_from_raw(raw: u8) -> ShutdownPhase {
    match raw {
        PHASE_RUNNING => ShutdownPhase::Running,
        PHASE_DRAINING => ShutdownPhase::Draining,
        PHASE_FORCED => ShutdownPhase::Forced,
        _ => unreachable!("shutdown phase is written only by this module"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_active_requests_while_running() {
        let coordinator = ShutdownCoordinator::default();
        let first = coordinator.begin_request().expect("first request admitted");
        let second = coordinator.begin_request().expect("second request admitted");
        assert_eq!(coordinator.active_requests(), 2);

        first.finish();
        assert_eq!(coordinator.active_requests(), 1);
        drop(second);
        assert_eq!(coordinator.active_requests(), 0);
    }

    #[test]
    fn draining_rejects_new_requests_with_retry_metadata_only() {
        let coordinator = ShutdownCoordinator::default();
        let existing = coordinator.begin_request().expect("existing request admitted");
        assert!(coordinator.start_draining());
        assert!(!coordinator.start_draining());

        let rejection = coordinator.begin_request().expect_err("new request rejected");
        assert_eq!(rejection.status, SHUTDOWN_HTTP_STATUS);
        assert_eq!(rejection.status, 429);
        assert_eq!(rejection.code, "service_draining");
        assert!(!rejection.headers.contains_key("connection"));
        assert_eq!(rejection.headers.get("retry-after").map(String::as_str), Some("5"));
        assert_eq!(coordinator.active_requests(), 1);

        drop(existing);
        assert_eq!(coordinator.active_requests(), 0);
    }

    #[test]
    fn retry_after_rounds_fractional_durations_up_to_delay_seconds() {
        let coordinator = ShutdownCoordinator::with_retry_after(
            Duration::from_secs(5),
            Duration::from_millis(1501),
        );
        coordinator.start_draining();
        let rejection = coordinator.rejection();
        assert_eq!(rejection.headers.get("retry-after").map(String::as_str), Some("2"));

        let subsecond = ShutdownCoordinator::with_retry_after(
            Duration::from_secs(5),
            Duration::from_millis(1),
        );
        subsecond.start_draining();
        assert_eq!(
            subsecond.rejection().headers.get("retry-after").map(String::as_str),
            Some("1")
        );
    }

    #[tokio::test]
    async fn drain_waits_for_existing_requests() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(5));
        let request = coordinator.begin_request().expect("request admitted");
        let waiter = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.drain().await })
        };

        tokio::task::yield_now().await;
        assert_eq!(coordinator.phase(), ShutdownPhase::Draining);
        assert!(!coordinator.is_accepting_requests());
        drop(request);

        assert_eq!(
            waiter.await.expect("drain task joins"),
            DrainOutcome::Drained { active_at_start: 1 }
        );
    }

    #[tokio::test]
    async fn drain_without_active_requests_is_immediately_drained() {
        let coordinator = ShutdownCoordinator::default();
        assert_eq!(
            coordinator.drain().await,
            DrainOutcome::Drained { active_at_start: 0 }
        );
        assert_eq!(coordinator.phase(), ShutdownPhase::Draining);
        assert!(!coordinator.is_accepting_requests());
    }

    #[tokio::test(start_paused = true)]
    async fn drain_timeout_escalates_to_forced_shutdown() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(5));
        let _request = coordinator.begin_request().expect("request admitted");
        let waiter = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.drain().await })
        };

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(5)).await;
        tokio::task::yield_now().await;

        assert_eq!(
            waiter.await.expect("drain task joins"),
            DrainOutcome::TimedOut { remaining: 1 }
        );
        assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
    }

    #[tokio::test]
    async fn explicit_force_interrupts_drain_without_waiting_for_timeout() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(60));
        let _request = coordinator.begin_request().expect("request admitted");
        let waiter = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.drain().await })
        };

        tokio::task::yield_now().await;
        assert!(coordinator.force_shutdown());
        assert!(!coordinator.force_shutdown());
        assert_eq!(
            waiter.await.expect("drain task joins"),
            DrainOutcome::Forced { remaining: 1 }
        );
    }

    #[tokio::test]
    async fn force_from_running_rejects_new_requests_and_drain_reports_forced() {
        let coordinator = ShutdownCoordinator::new(Duration::from_secs(60));
        let request = coordinator.begin_request().expect("request admitted");
        assert!(coordinator.force_shutdown());
        assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
        assert!(coordinator.begin_request().is_err());
        assert_eq!(
            coordinator.drain().await,
            DrainOutcome::Forced { remaining: 1 }
        );
        drop(request);
        assert_eq!(coordinator.active_requests(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn zero_drain_timeout_forces_inflight_requests_immediately() {
        let coordinator = ShutdownCoordinator::new(Duration::ZERO);
        let _request = coordinator.begin_request().expect("request admitted");
        assert_eq!(
            coordinator.drain().await,
            DrainOutcome::TimedOut { remaining: 1 }
        );
        assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
    }
}
