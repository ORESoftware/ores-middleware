#![forbid(unsafe_code)]

use std::time::Duration;

use ores_middleware::{DrainOutcome, ShutdownCoordinator, ShutdownPhase};

#[tokio::test]
async fn many_guards_can_finish_concurrently_while_drain_waits() {
    let coordinator = ShutdownCoordinator::new(Duration::from_secs(2));
    let guards = (0..64)
        .map(|_| coordinator.begin_request().expect("request admitted"))
        .collect::<Vec<_>>();
    assert_eq!(coordinator.active_requests(), 64);
    assert!(coordinator.start_draining());

    let waiter = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.drain().await })
    };

    let releases = guards
        .into_iter()
        .map(|guard| tokio::spawn(async move { drop(guard) }))
        .collect::<Vec<_>>();
    for release in releases {
        release.await.expect("release task joins");
    }

    assert!(matches!(
        waiter.await.expect("drain task joins"),
        DrainOutcome::Drained { .. }
    ));
    assert_eq!(coordinator.active_requests(), 0);
    assert_eq!(coordinator.phase(), ShutdownPhase::Draining);
}

#[tokio::test]
async fn concurrent_drain_waiters_are_all_released_when_last_request_finishes() {
    let coordinator = ShutdownCoordinator::new(Duration::from_secs(2));
    let guard = coordinator.begin_request().expect("request admitted");
    assert!(coordinator.start_draining());

    let first = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.drain().await })
    };
    let second = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.drain().await })
    };

    tokio::task::yield_now().await;
    drop(guard);

    assert!(matches!(
        first.await.expect("first drain joins"),
        DrainOutcome::Drained { .. }
    ));
    assert!(matches!(
        second.await.expect("second drain joins"),
        DrainOutcome::Drained { .. }
    ));
    assert_eq!(coordinator.active_requests(), 0);
}

#[tokio::test]
async fn admission_stays_closed_after_all_pre_drain_requests_finish() {
    let coordinator = ShutdownCoordinator::default();
    let guard = coordinator.begin_request().expect("request admitted");
    assert!(coordinator.start_draining());
    drop(guard);

    assert!(matches!(
        coordinator.drain().await,
        DrainOutcome::Drained { .. }
    ));
    assert_eq!(coordinator.phase(), ShutdownPhase::Draining);
    assert!(coordinator.begin_request().is_err());
}

#[tokio::test]
async fn force_before_any_work_is_monotonic_and_reports_zero_remaining() {
    let coordinator = ShutdownCoordinator::default();
    assert!(coordinator.force_shutdown());
    assert!(!coordinator.start_draining());
    assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
    assert_eq!(
        coordinator.drain().await,
        DrainOutcome::Forced { remaining: 0 }
    );
    assert!(!coordinator.force_shutdown());
    assert!(coordinator.begin_request().is_err());
}

#[tokio::test]
async fn timeout_force_cannot_regress_after_late_request_release() {
    let coordinator = ShutdownCoordinator::new(Duration::ZERO);
    let guard = coordinator.begin_request().expect("request admitted");

    assert_eq!(
        coordinator.drain().await,
        DrainOutcome::TimedOut { remaining: 1 }
    );
    assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
    assert!(!coordinator.start_draining());
    assert!(!coordinator.force_shutdown());

    drop(guard);
    assert_eq!(coordinator.active_requests(), 0);
    assert_eq!(coordinator.phase(), ShutdownPhase::Forced);
    assert!(coordinator.begin_request().is_err());
}
