use std::{
    convert::Infallible,
    future::{pending, ready},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ores_middleware::otel::current_log_context;
use ores_middleware::{
    OperationDescriptor, OperationFailureKind, OperationOutcome, OperationScope,
    OperationTransport, RequestContext, current_context,
    run_operation_boundary_with_timeout_and_cancellation,
};

fn context() -> RequestContext {
    RequestContext {
        request_id: "controlled-1".into(),
        trace_id: "0123456789abcdef0123456789abcdef".into(),
        span_id: None,
        tenant_id: Some("tenant-1".into()),
        user_id: Some("user-1".into()),
        locale: None,
        started_at_unix_ms: 0,
        deadline_unix_ms: None,
        baggage: Default::default(),
    }
}

fn descriptor() -> OperationDescriptor {
    OperationDescriptor {
        transport: OperationTransport::Tcp,
        scope: OperationScope::Message,
        name: "tcp.message".into(),
    }
}

struct DropMark(Arc<AtomicBool>);
impl Drop for DropMark {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn assert_failed<T>(outcome: &OperationOutcome<T>, kind: OperationFailureKind) {
    let failure = outcome.failure().expect("expected a terminal failure");
    assert_eq!(failure.kind, kind);
    assert_eq!(failure.request_id, context().request_id);
    assert_eq!(failure.trace_id, context().trace_id);
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn cancellation_wins_three_way_ready_tie_without_dispatch() {
    let called = Arc::new(AtomicBool::new(false));
    let marker = called.clone();
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::ZERO,
        ready(()),
        async move {
            marker.store(true, Ordering::SeqCst);
            Ok::<_, Infallible>(())
        },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::Cancelled);
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn overflowing_deadline_fails_closed_without_dispatch_or_panic() {
    let called = Arc::new(AtomicBool::new(false));
    let marker = called.clone();
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::MAX,
        pending(),
        async move {
            marker.store(true, Ordering::SeqCst);
            Ok::<_, Infallible>(())
        },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::DeadlineExceeded);
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn pending_operation_times_out_and_drops_its_resources() {
    let dropped = Arc::new(AtomicBool::new(false));
    let resource = DropMark(dropped.clone());
    let started = tokio::time::Instant::now();
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::from_secs(3),
        pending(),
        async move {
            let _resource = resource;
            assert_eq!(current_context(), Some(context()));
            pending::<()>().await;
            Ok::<_, Infallible>(())
        },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::DeadlineExceeded);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(started.elapsed() >= Duration::from_secs(3));
}

#[tokio::test]
async fn active_operation_cancellation_drops_resources() {
    let (started, received) = tokio::sync::oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let resource = DropMark(dropped.clone());
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::from_secs(30),
        async {
            received
                .await
                .expect("handler must begin before cancellation");
        },
        async move {
            let _resource = resource;
            assert_eq!(current_context(), Some(context()));
            started.send(()).expect("cancellation receiver exists");
            pending::<()>().await;
            Ok::<_, Infallible>(())
        },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::Cancelled);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn successful_operation_drops_unused_cancellation_and_returns_value() {
    let dropped = Arc::new(AtomicBool::new(false));
    let resource = DropMark(dropped.clone());
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::from_secs(30),
        async move {
            let _resource = resource;
            pending::<()>().await;
        },
        async {
            tokio::task::yield_now().await;
            assert_eq!(current_context(), Some(context()));
            Ok::<_, Infallible>("complete")
        },
    )
    .await;
    assert_eq!(outcome, OperationOutcome::Completed("complete"));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn returned_error_is_not_reclassified_as_cancellation() {
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::from_secs(30),
        pending(),
        async { Err::<(), _>(std::io::Error::other("private payload")) },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::Error);
    let encoded = serde_json::to_string(outcome.failure().unwrap()).unwrap();
    assert!(!encoded.contains("private payload"));
    assert_eq!(outcome.failure().unwrap().code, "operation_failed");
}

#[tokio::test]
async fn panic_is_contained_and_context_is_restored() {
    let outcome = run_operation_boundary_with_timeout_and_cancellation(
        context(),
        descriptor(),
        Duration::from_secs(30),
        pending(),
        async {
            panic!("synthetic handler panic");
            #[allow(unreachable_code)]
            Ok::<(), Infallible>(())
        },
    )
    .await;
    assert_failed(&outcome, OperationFailureKind::Panic);
    assert_eq!(outcome.failure().unwrap().code, "operation_panicked");
}
