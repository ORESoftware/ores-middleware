use std::{
    convert::Infallible,
    future::{Future, pending, ready},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use ores_middleware::otel::current_log_context;
use ores_middleware::{
    OperationDescriptor, OperationFailureKind, OperationOutcome, OperationScope,
    OperationTransport, RequestContext, current_context, run_operation_boundary_with_cancellation,
    run_operation_boundary_with_timeout, run_with_context, run_with_ores_log_context,
};

fn context(slot: usize) -> RequestContext {
    RequestContext {
        request_id: format!("admission-{slot}"),
        trace_id: format!("{slot:032x}"),
        span_id: None,
        tenant_id: Some(format!("tenant-{slot}")),
        user_id: Some(format!("user-{slot}")),
        locale: None,
        started_at_unix_ms: 0,
        deadline_unix_ms: None,
        baggage: Default::default(),
    }
}

fn descriptor(transport: OperationTransport) -> OperationDescriptor {
    OperationDescriptor {
        transport,
        scope: OperationScope::Message,
        name: "transport.admission".into(),
    }
}

fn assert_failure<T>(outcome: &OperationOutcome<T>, kind: OperationFailureKind, slot: usize) {
    let failure = outcome.failure().expect("work must not be admitted");
    assert_eq!(failure.kind, kind);
    assert_eq!(failure.request_id, format!("admission-{slot}"));
    assert_eq!(failure.trace_id, format!("{slot:032x}"));
}

#[tokio::test]
async fn already_cancelled_never_polls_a_ready_handler() {
    for transport in [
        OperationTransport::Http,
        OperationTransport::Tcp,
        OperationTransport::WebSocket,
    ] {
        let dispatched = Arc::new(AtomicUsize::new(0));
        for slot in 0..128 {
            let count = dispatched.clone();
            let outcome = run_operation_boundary_with_cancellation(
                context(slot),
                descriptor(transport),
                ready(()),
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, Infallible>(())
                },
            )
            .await;
            assert_failure(&outcome, OperationFailureKind::Cancelled, slot);
        }
        assert_eq!(dispatched.load(Ordering::SeqCst), 0);
    }
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn zero_budget_never_polls_a_ready_handler() {
    for transport in [
        OperationTransport::Http,
        OperationTransport::Tcp,
        OperationTransport::WebSocket,
    ] {
        let dispatched = Arc::new(AtomicBool::new(false));
        let called = dispatched.clone();
        let outcome = run_operation_boundary_with_timeout(
            context(2),
            descriptor(transport),
            Duration::ZERO,
            async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            },
        )
        .await;
        assert_failure(&outcome, OperationFailureKind::DeadlineExceeded, 2);
        assert!(!dispatched.load(Ordering::SeqCst));
    }
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

struct DropProbe {
    polled: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl Future for DropProbe {
    type Output = Result<(), Infallible>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polled.store(true, Ordering::SeqCst);
        Poll::Pending
    }
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancellation_drops_unpolled_handler_before_returning() {
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let outcome = run_operation_boundary_with_cancellation(
        context(3),
        descriptor(OperationTransport::Tcp),
        ready(()),
        DropProbe {
            polled: polled.clone(),
            dropped: dropped.clone(),
        },
    )
    .await;
    assert_failure(&outcome, OperationFailureKind::Cancelled, 3);
    assert!(!polled.load(Ordering::SeqCst));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn zero_budget_drops_unpolled_handler_before_returning() {
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let outcome = run_operation_boundary_with_timeout(
        context(4),
        descriptor(OperationTransport::WebSocket),
        Duration::ZERO,
        DropProbe {
            polled: polled.clone(),
            dropped: dropped.clone(),
        },
    )
    .await;
    assert_failure(&outcome, OperationFailureKind::DeadlineExceeded, 4);
    assert!(!polled.load(Ordering::SeqCst));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn pending_cancellation_allows_handler_and_preserves_context() {
    let expected = context(5);
    let outcome = run_operation_boundary_with_cancellation(
        expected.clone(),
        descriptor(OperationTransport::Tcp),
        pending(),
        async {
            assert_eq!(current_context(), Some(expected));
            Ok::<_, Infallible>(42)
        },
    )
    .await;
    assert_eq!(outcome, OperationOutcome::Completed(42));
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn cancelled_child_restores_outer_request_and_logger_context() {
    let outer = context(6);
    let log_source = outer.clone();
    run_with_context(
        outer.clone(),
        run_with_ores_log_context(&log_source, async {
            let outer_log = current_log_context();
            let outcome = run_operation_boundary_with_cancellation(
                context(7),
                descriptor(OperationTransport::WebSocket),
                ready(()),
                async { Ok::<_, Infallible>(()) },
            )
            .await;
            assert_failure(&outcome, OperationFailureKind::Cancelled, 7);
            assert_eq!(current_context(), Some(outer));
            assert_eq!(current_log_context(), outer_log);
        }),
    )
    .await;
    assert!(current_context().is_none());
    assert_eq!(current_log_context(), Default::default());
}

#[tokio::test]
async fn concurrent_cancelled_operations_do_not_dispatch_or_leak_context() {
    let tasks = (10..74).map(|slot| async move {
        let outcome = run_operation_boundary_with_cancellation(
            context(slot),
            descriptor(OperationTransport::Tcp),
            ready(()),
            async {
                panic!("a cancelled operation must never dispatch");
                #[allow(unreachable_code)]
                Ok::<(), Infallible>(())
            },
        )
        .await;
        assert_failure(&outcome, OperationFailureKind::Cancelled, slot);
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    });
    futures_util::future::join_all(tasks).await;
}
