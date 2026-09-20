use std::{collections::BTreeMap, fmt, future::Future, path::PathBuf, time::Duration};

use uuid::Uuid;

use crate::{
    BootstrapError, ManifestLoadError, MiddlewareStack, OperationDescriptor, OperationOutcome,
    OperationScope, OperationTransport, RequestContext, admit_server_stack_from_env,
    run_operation_boundary_with_timeout, run_operation_boundary_with_timeout_and_cancellation,
    stack_from_env,
};

const MAX_TOKEN_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LambdaInvocationTrigger {
    Queue,
    Schedule,
    Direct,
}

impl LambdaInvocationTrigger {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Schedule => "schedule",
            Self::Direct => "direct",
        }
    }
}

/// Trusted adapter metadata for a non-HTTP Lambda callback.
///
/// Provider names and public payload fields are deliberately absent. The adapter
/// supplies only bounded invocation metadata that it obtained from its runtime
/// API. In particular, payload data cannot establish user or tenant identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaInvocationMetadata {
    pub invocation_id: String,
    pub trace_id: Option<String>,
    pub function_name: String,
    pub trigger: LambdaInvocationTrigger,
    pub payload_bytes: u64,
    pub deadline_unix_ms: Option<u64>,
}

#[derive(Debug)]
pub enum LambdaInvocationError {
    Manifest(ManifestLoadError),
    Bootstrap(BootstrapError),
    InvalidMetadata(&'static str),
    PayloadTooLarge { payload_bytes: u64, limit: u64 },
}

impl LambdaInvocationError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Manifest(error) => error.code(),
            Self::Bootstrap(error) => error.code,
            Self::InvalidMetadata(_) => "invalid_lambda_invocation_metadata",
            Self::PayloadTooLarge { .. } => "lambda_payload_too_large",
        }
    }
}

impl fmt::Display for LambdaInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => write!(formatter, "lambda middleware manifest admission failed: {error}"),
            Self::Bootstrap(error) => write!(formatter, "lambda middleware bootstrap failed: {error}"),
            Self::InvalidMetadata(field) => write!(formatter, "invalid trusted Lambda metadata field: {field}"),
            Self::PayloadTooLarge {
                payload_bytes,
                limit,
            } => write!(
                formatter,
                "Lambda payload exceeds configured middleware limit ({payload_bytes} > {limit})"
            ),
        }
    }
}

impl std::error::Error for LambdaInvocationError {}

/// Fail-closed, provider-neutral middleware boundary for queue, schedule and
/// direct Lambda callbacks.
///
/// HTTP-triggered functions should continue to use the framework/Axum adapter so
/// TLS, forwarded-header, CORS and response-header policy stay on the HTTP path.
/// This boundary admits `.ores-mw.toml`, bootstraps the same middleware config,
/// applies the shared payload/deadline policy, and executes the callback through
/// the first-class `lambda` operation boundary with task-local request/log
/// context. It intentionally does not fabricate HTTP method/path/header values.
#[derive(Clone)]
pub struct LambdaInvocationBoundary {
    stack: MiddlewareStack,
    manifest_path: PathBuf,
    target_name: Option<String>,
    stack_config: String,
}

impl LambdaInvocationBoundary {
    /// Discover and admit `.ores-mw.toml`, prove the selected server stack path,
    /// and bootstrap the same environment-driven middleware configuration used
    /// by HTTP servers.
    pub fn from_env(
        service_name: impl Into<String>,
        target_name: Option<&str>,
        expected_stack_config: &str,
    ) -> Result<Self, LambdaInvocationError> {
        let manifest_path = admit_server_stack_from_env(target_name, expected_stack_config)
            .map_err(LambdaInvocationError::Manifest)?;
        let stack = stack_from_env(service_name).map_err(LambdaInvocationError::Bootstrap)?;
        Ok(Self {
            stack,
            manifest_path,
            target_name: target_name.map(ToOwned::to_owned),
            stack_config: expected_stack_config.to_owned(),
        })
    }

    #[must_use]
    pub fn manifest_path(&self) -> &std::path::Path {
        &self.manifest_path
    }

    #[must_use]
    pub fn target_name(&self) -> Option<&str> {
        self.target_name.as_deref()
    }

    #[must_use]
    pub fn stack_config(&self) -> &str {
        &self.stack_config
    }

    /// Execute one non-HTTP Lambda callback with the middleware timeout and any
    /// earlier provider runtime deadline. Invalid/expired budgets fail before the
    /// callback future is polled.
    pub async fn run<F, T, E>(
        &self,
        metadata: LambdaInvocationMetadata,
        operation: F,
    ) -> Result<OperationOutcome<T>, LambdaInvocationError>
    where
        F: Future<Output = Result<T, E>>,
    {
        let admitted = self.admit(metadata)?;
        Ok(run_operation_boundary_with_timeout(
            admitted.context,
            admitted.descriptor,
            admitted.timeout,
            operation,
        )
        .await)
    }

    /// Cancellation-aware form for runtimes that expose a shutdown/abort future.
    /// Cancellation wins a simultaneous readiness tie, matching the shared
    /// operation boundary semantics.
    pub async fn run_with_cancellation<F, C, T, E>(
        &self,
        metadata: LambdaInvocationMetadata,
        cancellation: C,
        operation: F,
    ) -> Result<OperationOutcome<T>, LambdaInvocationError>
    where
        F: Future<Output = Result<T, E>>,
        C: Future<Output = ()>,
    {
        let admitted = self.admit(metadata)?;
        Ok(run_operation_boundary_with_timeout_and_cancellation(
            admitted.context,
            admitted.descriptor,
            admitted.timeout,
            cancellation,
            operation,
        )
        .await)
    }

    fn admit(
        &self,
        metadata: LambdaInvocationMetadata,
    ) -> Result<AdmittedInvocation, LambdaInvocationError> {
        if !valid_token(&metadata.invocation_id) {
            return Err(LambdaInvocationError::InvalidMetadata("invocation_id"));
        }
        if !valid_token(&metadata.function_name) {
            return Err(LambdaInvocationError::InvalidMetadata("function_name"));
        }
        let max_payload = self.stack.config().settings.max_body_bytes as u64;
        if metadata.payload_bytes > max_payload {
            return Err(LambdaInvocationError::PayloadTooLarge {
                payload_bytes: metadata.payload_bytes,
                limit: max_payload,
            });
        }

        let trace_id = match metadata.trace_id {
            Some(trace_id) if valid_trace_id(&trace_id) => trace_id.to_ascii_lowercase(),
            Some(_) => return Err(LambdaInvocationError::InvalidMetadata("trace_id")),
            None => Uuid::new_v4().simple().to_string(),
        };
        let now = RequestContext::now_ms();
        let configured_deadline = now.saturating_add(self.stack.config().settings.timeout_ms);
        let deadline_unix_ms = metadata
            .deadline_unix_ms
            .map_or(configured_deadline, |provider| provider.min(configured_deadline));
        let timeout = Duration::from_millis(deadline_unix_ms.saturating_sub(now));

        let baggage = BTreeMap::from([
            (
                "otel.lambda.trigger".to_owned(),
                metadata.trigger.as_str().to_owned(),
            ),
            (
                "otel.lambda.function".to_owned(),
                metadata.function_name.clone(),
            ),
        ]);
        let context = RequestContext {
            request_id: metadata.invocation_id,
            trace_id,
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: now,
            deadline_unix_ms: Some(deadline_unix_ms),
            baggage,
        };
        let descriptor = OperationDescriptor {
            transport: OperationTransport::Lambda,
            scope: OperationScope::Callback,
            name: format!("lambda.{}", metadata.function_name),
        };
        Ok(AdmittedInvocation {
            context,
            descriptor,
            timeout,
        })
    }

    #[cfg(test)]
    fn from_stack_for_test(stack: MiddlewareStack) -> Self {
        Self {
            stack,
            manifest_path: PathBuf::from(".ores-mw.toml"),
            target_name: Some("lambda".into()),
            stack_config: "config/middleware.json".into(),
        }
    }
}

struct AdmittedInvocation {
    context: RequestContext,
    descriptor: OperationDescriptor,
    timeout: Duration,
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

fn valid_trace_id(value: &str) -> bool {
    value.len() == 32
        && value != "00000000000000000000000000000000"
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::{Arc, atomic::{AtomicBool, Ordering}}};

    use super::*;
    use crate::{MiddlewareStack, OperationFailureKind, current_context, default_config};

    fn boundary() -> LambdaInvocationBoundary {
        LambdaInvocationBoundary::from_stack_for_test(
            MiddlewareStack::new(default_config("lambda-test")).expect("valid stack"),
        )
    }

    fn metadata() -> LambdaInvocationMetadata {
        LambdaInvocationMetadata {
            invocation_id: "invoke-1".into(),
            trace_id: Some("0123456789abcdef0123456789abcdef".into()),
            function_name: "orders-consume".into(),
            trigger: LambdaInvocationTrigger::Queue,
            payload_bytes: 32,
            deadline_unix_ms: None,
        }
    }

    #[tokio::test]
    async fn callback_runs_with_lambda_context_and_no_payload_identity() {
        let outcome = boundary()
            .run(metadata(), async {
                let context = current_context().expect("callback context");
                assert_eq!(context.request_id, "invoke-1");
                assert_eq!(context.user_id, None);
                assert_eq!(context.tenant_id, None);
                assert_eq!(
                    context.baggage.get("otel.lambda.trigger").map(String::as_str),
                    Some("queue")
                );
                Ok::<_, Infallible>(42)
            })
            .await
            .expect("metadata admitted");
        assert_eq!(outcome, OperationOutcome::Completed(42));
        assert!(current_context().is_none());
    }

    #[tokio::test]
    async fn oversized_payload_is_rejected_before_callback_poll() {
        let mut input = metadata();
        input.payload_bytes = u64::MAX;
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let result = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await;
        assert!(matches!(result, Err(LambdaInvocationError::PayloadTooLarge { .. })));
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn expired_provider_deadline_never_polls_callback() {
        let mut input = metadata();
        input.deadline_unix_ms = Some(0);
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let outcome = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await
            .expect("metadata admitted");
        assert!(matches!(
            outcome.failure().map(|failure| failure.kind),
            Some(OperationFailureKind::DeadlineExceeded)
        ));
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn invalid_trace_id_is_rejected_before_callback_poll() {
        let mut input = metadata();
        input.trace_id = Some("caller-controlled-not-a-trace".into());
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let result = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await;
        assert!(matches!(
            result,
            Err(LambdaInvocationError::InvalidMetadata("trace_id"))
        ));
        assert!(!polled.load(Ordering::SeqCst));
    }
}
