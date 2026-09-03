use std::{
    any::type_name,
    future::Future,
    panic::AssertUnwindSafe,
    time::Duration,
};

use futures_util::FutureExt;
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use crate::{run_with_context, run_with_ores_log_context, RequestContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationTransport {
    Http,
    Tcp,
    WebSocket,
}

impl OperationTransport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::WebSocket => "websocket",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationScope {
    Request,
    Connection,
    Message,
    Callback,
}

impl OperationScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Connection => "connection",
            Self::Message => "message",
            Self::Callback => "callback",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationFailureKind {
    Error,
    Panic,
    Cancelled,
    DeadlineExceeded,
}

impl OperationFailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Panic => "panic",
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline_exceeded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDescriptor {
    pub transport: OperationTransport,
    pub scope: OperationScope,
    /// Stable low-cardinality name such as `orders.read` or `chat.message`.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFailure {
    pub kind: OperationFailureKind,
    pub code: String,
    pub transport: OperationTransport,
    pub scope: OperationScope,
    pub operation: String,
    pub request_id: String,
    pub trace_id: String,
    pub error_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome<T> {
    Completed(T),
    Failed(OperationFailure),
}

impl<T> OperationOutcome<T> {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }

    pub fn failure(&self) -> Option<&OperationFailure> {
        match self {
            Self::Completed(_) => None,
            Self::Failed(failure) => Some(failure),
        }
    }
}

/// Executes one HTTP request, TCP callback, or WebSocket message inside the
/// middleware task-local, ores-otel task context, and a `tracing` span.
/// Returned errors and unwind panics become typed outcomes instead of
/// unwinding the listener task.
pub async fn run_operation_boundary<F, T, E>(
    context: RequestContext,
    descriptor: OperationDescriptor,
    operation: F,
) -> OperationOutcome<T>
where
    F: Future<Output = Result<T, E>>,
{
    run_scoped(context, normalize_descriptor(descriptor), operation).await
}

/// Deadline-bounded variant. The timed-out operation future is dropped, which
/// also drops both task-local scopes. The timeout failure is then reported in a
/// fresh scope carrying the same request and tracing identifiers.
pub async fn run_operation_boundary_with_timeout<F, T, E>(
    context: RequestContext,
    descriptor: OperationDescriptor,
    timeout: Duration,
    operation: F,
) -> OperationOutcome<T>
where
    F: Future<Output = Result<T, E>>,
{
    let descriptor = normalize_descriptor(descriptor);
    match tokio::time::timeout(
        timeout,
        run_scoped(context.clone(), descriptor.clone(), operation),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => {
            report_terminal_failure(
                context,
                descriptor,
                OperationFailureKind::DeadlineExceeded,
                "DeadlineExceeded",
            )
            .await
        }
    }
}

/// Cooperative cancellation variant for connection loops and server shutdown.
/// Resolving `cancellation` drops the guarded operation and reports a typed,
/// request-correlated cancellation outcome.
pub async fn run_operation_boundary_with_cancellation<F, C, T, E>(
    context: RequestContext,
    descriptor: OperationDescriptor,
    cancellation: C,
    operation: F,
) -> OperationOutcome<T>
where
    F: Future<Output = Result<T, E>>,
    C: Future<Output = ()>,
{
    let descriptor = normalize_descriptor(descriptor);
    let context_for_failure = context.clone();
    let descriptor_for_failure = descriptor.clone();
    tokio::select! {
        outcome = run_scoped(context, descriptor, operation) => outcome,
        _ = cancellation => {
            report_terminal_failure(
                context_for_failure,
                descriptor_for_failure,
                OperationFailureKind::Cancelled,
                "Cancelled",
            ).await
        }
    }
}

async fn run_scoped<F, T, E>(
    context: RequestContext,
    descriptor: OperationDescriptor,
    operation: F,
) -> OperationOutcome<T>
where
    F: Future<Output = Result<T, E>>,
{
    let log_context_source = context.clone();
    let request_id = context.request_id.clone();
    let trace_id = context.trace_id.clone();
    let error_type = bounded_error_type::<E>();
    let span = operation_span(&context, &descriptor);

    let guarded = async move {
        match AssertUnwindSafe(operation).catch_unwind().await {
            Ok(Ok(value)) => OperationOutcome::Completed(value),
            Ok(Err(_)) => operation_failure(
                &descriptor,
                &request_id,
                &trace_id,
                OperationFailureKind::Error,
                error_type,
            ),
            Err(_) => operation_failure(
                &descriptor,
                &request_id,
                &trace_id,
                OperationFailureKind::Panic,
                "panic",
            ),
        }
    }
    .instrument(span);

    run_with_context(
        context,
        run_with_ores_log_context(&log_context_source, guarded),
    )
    .await
}

async fn report_terminal_failure<T>(
    context: RequestContext,
    descriptor: OperationDescriptor,
    kind: OperationFailureKind,
    error_type: &'static str,
) -> OperationOutcome<T> {
    let log_context_source = context.clone();
    let request_id = context.request_id.clone();
    let trace_id = context.trace_id.clone();
    let span = operation_span(&context, &descriptor);
    let report = async move {
        operation_failure(
            &descriptor,
            &request_id,
            &trace_id,
            kind,
            error_type,
        )
    }
    .instrument(span);

    run_with_context(
        context,
        run_with_ores_log_context(&log_context_source, report),
    )
    .await
}

fn operation_span(context: &RequestContext, descriptor: &OperationDescriptor) -> tracing::Span {
    tracing::info_span!(
        "ores.operation",
        request_id = %context.request_id,
        trace_id = %context.trace_id,
        span_id = %context.span_id.as_deref().unwrap_or(""),
        user_id = %context.user_id.as_deref().unwrap_or(""),
        tenant_id = %context.tenant_id.as_deref().unwrap_or(""),
        operation_name = %descriptor.name,
        operation_transport = descriptor.transport.as_str(),
        operation_scope = descriptor.scope.as_str(),
    )
}

fn operation_failure<T>(
    descriptor: &OperationDescriptor,
    request_id: &str,
    trace_id: &str,
    kind: OperationFailureKind,
    error_type: &str,
) -> OperationOutcome<T> {
    let code = match kind {
        OperationFailureKind::Error => "operation_failed",
        OperationFailureKind::Panic => "operation_panicked",
        OperationFailureKind::Cancelled => "operation_cancelled",
        OperationFailureKind::DeadlineExceeded => "operation_deadline_exceeded",
    };
    tracing::error!(
        operation_outcome = kind.as_str(),
        error_type = %error_type,
        failure_code = code,
        request_id = %request_id,
        trace_id = %trace_id,
        "operation failed"
    );
    OperationOutcome::Failed(OperationFailure {
        kind,
        code: code.into(),
        transport: descriptor.transport,
        scope: descriptor.scope,
        operation: descriptor.name.clone(),
        request_id: request_id.into(),
        trace_id: trace_id.into(),
        error_type: error_type.into(),
    })
}

fn normalize_descriptor(mut descriptor: OperationDescriptor) -> OperationDescriptor {
    if !safe_token(&descriptor.name, 128) {
        descriptor.name = "operation".into();
    }
    descriptor
}

fn bounded_error_type<E>() -> &'static str {
    let name = type_name::<E>();
    let short = name.rsplit("::").next().unwrap_or("error");
    if safe_token(short, 64) {
        short
    } else {
        "error"
    }
}

fn safe_token(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, convert::Infallible};

    use super::*;
    use crate::{current_context, otel::current_log_context};

    fn request_context(slot: usize) -> RequestContext {
        RequestContext {
            request_id: format!("request-{slot}"),
            trace_id: format!("{slot:032x}"),
            span_id: None,
            tenant_id: Some(format!("tenant-{slot}")),
            user_id: Some(format!("user-{slot}")),
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn websocket_panic_isolated_and_later_message_runs() {
        let context = request_context(1);
        let failed = run_operation_boundary(
            context.clone(),
            OperationDescriptor {
                transport: OperationTransport::WebSocket,
                scope: OperationScope::Message,
                name: "chat.message".into(),
            },
            async {
                panic!("handler panic");
                #[allow(unreachable_code)]
                Ok::<(), Infallible>(())
            },
        )
        .await;
        let failure = failed.failure().expect("panic must be captured");
        assert_eq!(failure.kind, OperationFailureKind::Panic);
        assert_eq!(failure.request_id, "request-1");
        assert_eq!(failure.error_type, "panic");

        let succeeded = run_operation_boundary(
            context,
            OperationDescriptor {
                transport: OperationTransport::WebSocket,
                scope: OperationScope::Message,
                name: "chat.message".into(),
            },
            async { Ok::<_, Infallible>("ok") },
        )
        .await;
        assert_eq!(succeeded, OperationOutcome::Completed("ok"));
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn returned_error_is_correlated_without_copying_its_message() {
        #[derive(Debug)]
        struct ConnectionError;

        let failed = run_operation_boundary(
            request_context(2),
            OperationDescriptor {
                transport: OperationTransport::Tcp,
                scope: OperationScope::Connection,
                name: "smtp.accept".into(),
            },
            async { Err::<(), _>(ConnectionError) },
        )
        .await;
        let failure = failed.failure().expect("error must be captured");
        assert_eq!(failure.kind, OperationFailureKind::Error);
        assert_eq!(failure.error_type, "ConnectionError");
        assert_eq!(failure.request_id, "request-2");
    }

    #[tokio::test]
    async fn timeout_drops_scoped_future_and_restores_context() {
        let failed = run_operation_boundary_with_timeout(
            request_context(3),
            OperationDescriptor {
                transport: OperationTransport::Http,
                scope: OperationScope::Request,
                name: "orders.read".into(),
            },
            Duration::from_millis(5),
            async {
                std::future::pending::<()>().await;
                Ok::<_, Infallible>(())
            },
        )
        .await;
        assert_eq!(
            failed.failure().map(|failure| failure.kind),
            Some(OperationFailureKind::DeadlineExceeded)
        );
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn cancellation_drops_scoped_future_and_restores_context() {
        let failed = run_operation_boundary_with_cancellation(
            request_context(4),
            OperationDescriptor {
                transport: OperationTransport::Tcp,
                scope: OperationScope::Connection,
                name: "tcp.accept".into(),
            },
            std::future::ready(()),
            async {
                std::future::pending::<()>().await;
                Ok::<_, Infallible>(())
            },
        )
        .await;
        assert_eq!(
            failed.failure().map(|failure| failure.kind),
            Some(OperationFailureKind::Cancelled)
        );
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn parallel_operations_do_not_bleed_context() {
        let operations = (0..48).map(|slot| {
            let context = request_context(slot);
            async move {
                run_operation_boundary(
                    context.clone(),
                    OperationDescriptor {
                        transport: OperationTransport::Tcp,
                        scope: OperationScope::Callback,
                        name: "tcp.read".into(),
                    },
                    async move {
                        tokio::task::yield_now().await;
                        assert_eq!(
                            current_context().map(|value| value.request_id),
                            Some(context.request_id)
                        );
                        Ok::<_, Infallible>(slot)
                    },
                )
                .await
            }
        });
        let outcomes = futures_util::future::join_all(operations).await;
        assert!(outcomes.iter().all(|outcome| outcome.is_completed()));
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn unbounded_operation_name_is_normalized() {
        let failed = run_operation_boundary(
            request_context(5),
            OperationDescriptor {
                transport: OperationTransport::Tcp,
                scope: OperationScope::Callback,
                name: format!("customer/{}", "x".repeat(300)),
            },
            async { Err::<(), _>(std::io::Error::other("private payload")) },
        )
        .await;
        assert_eq!(
            failed.failure().map(|failure| failure.operation.as_str()),
            Some("operation")
        );
    }
}
