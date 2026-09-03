from pathlib import Path


def replace_once(path: str, before: str, after: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(before)
    if count != 1:
        raise RuntimeError(f"expected exactly one match in {path}, found {count}: {before[:120]!r}")
    target.write_text(text.replace(before, after, 1), encoding="utf-8")


def replace_range(path: str, start: str, end: str, replacement: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    start_count = text.count(start)
    end_count = text.count(end)
    if start_count != 1 or end_count != 1:
        raise RuntimeError(
            f"expected unique range markers in {path}; start={start_count}, end={end_count}"
        )
    start_index = text.index(start)
    end_index = text.index(end, start_index)
    target.write_text(
        text[:start_index] + replacement + text[end_index:],
        encoding="utf-8",
    )


replace_once(
    "src/rust/src/pipeline.rs",
    '''use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use uuid::Uuid;
''',
    '''use std::{
    collections::BTreeMap,
    future::Future,
    net::IpAddr,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::FutureExt;
use tracing::Instrument;
use uuid::Uuid;
''',
)

replace_once(
    "src/rust/src/pipeline.rs",
    '''    context::{ContextRegistry, RequestContext},''',
    '''    context::{ContextRegistry, RequestContext, run_with_context},''',
)

replace_once(
    "src/rust/src/pipeline.rs",
    '''    net::cidr_contains,
    rate_limit::{''',
    '''    net::cidr_contains,
    otel::run_with_ores_log_context,
    rate_limit::{''',
)

replace_range(
    "src/rust/src/pipeline.rs",
    '''    pub async fn begin(&self, request: RequestMetadata) -> Result<ActiveRequest, MiddlewareError> {''',
    '''    async fn evaluate_rate_limit(''',
    '''    pub async fn begin(&self, request: RequestMetadata) -> Result<ActiveRequest, MiddlewareError> {
        let request_id = request
            .headers
            .get(&self.config.settings.request_id_header)
            .filter(|value| valid_token(value))
            .cloned()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let trace_id = parse_trace_id(request.headers.get(&self.config.settings.trace_header))
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let now = RequestContext::now_ms();
        let base_context = RequestContext {
            request_id,
            trace_id,
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: request.headers.get("accept-language").cloned(),
            started_at_unix_ms: now,
            deadline_unix_ms: Some(now.saturating_add(self.config.settings.timeout_ms)),
            baggage: BTreeMap::new(),
        };

        let auth = run_lifecycle_stage(&base_context, "middleware.pre_auth", async {
            if request
                .content_length
                .is_some_and(|length| length > self.config.settings.max_body_bytes as u64)
            {
                return Err(MiddlewareError::new(
                    413,
                    "payload_too_large",
                    "request body exceeds configured limit",
                ));
            }

            enforce_transport_policy(&self.config, &request)?;
            self.auth
                .verify(&request)
                .await
                .map_err(|error| MiddlewareError::new(401, error.code, error.message))
        })
        .await
        .map_err(|error| correlate_error(&self.config, &base_context, error))?;

        let context = RequestContext {
            tenant_id: auth.tenant_id.clone(),
            user_id: auth.user_id.clone(),
            baggage: auth
                .claims
                .iter()
                .filter(|(key, _)| key.starts_with("otel."))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            ..base_context
        };

        let started = run_lifecycle_stage(&context, "middleware.request_start", async {
            if !matches!(
                self.config.integrations.shared_auth.mode,
                IntegrationMode::Disabled
            ) && context.user_id.is_none()
            {
                return Err(MiddlewareError::new(
                    401,
                    "authentication_required",
                    "shared-auth did not establish a user",
                ));
            }

            if self.config.settings.rate_limit.enabled {
                let decision = self.evaluate_rate_limit(&context, &request, &auth).await;
                self.telemetry
                    .rate_limit_decision(&context, &request, &decision)
                    .await;
                if !decision.is_allowed() {
                    return Err(rate_limit_error(&decision));
                }
            }

            if self.config.settings.fault_injection.enabled
                && self.config.settings.fault_injection.latency_ms > 0
            {
                tokio::time::sleep(Duration::from_millis(
                    self.config.settings.fault_injection.latency_ms,
                ))
                .await;
            }

            self.telemetry.request_started(&context, &request).await;
            self.registry.insert(context.clone()).await;
            Ok(Instant::now())
        })
        .await;

        let started = match started {
            Ok(started) => started,
            Err(error) => {
                self.registry.remove(&context.request_id).await;
                return Err(correlate_error(&self.config, &context, error));
            }
        };

        Ok(ActiveRequest {
            context,
            started,
            request,
        })
    }

''',
)

replace_range(
    "src/rust/src/pipeline.rs",
    '''    pub async fn finish(
''',
    '''fn decision_for_failure(''',
    '''    pub async fn finish(
        &self,
        active: ActiveRequest,
        status: u16,
        response_bytes: Option<u64>,
    ) -> BTreeMap<String, String> {
        let ActiveRequest {
            context,
            started,
            request,
        } = active;
        let response = ResponseMetadata {
            status,
            duration_ms: started.elapsed().as_millis() as u64,
            response_bytes,
        };

        let _ = run_lifecycle_stage(&context, "middleware.request_finish", async {
            self.telemetry
                .request_finished(&context, &request, &response)
                .await;
            if let Err(error) = self.sync.observe(&context, &request, &response).await {
                tracing::warn!(
                    request_id = %context.request_id,
                    trace_id = %context.trace_id,
                    code = error.code,
                    "opto-sync observation failed"
                );
            }
            Ok(())
        })
        .await;

        let request_id = context.request_id.clone();
        let _ = run_lifecycle_stage(&context, "middleware.request_cleanup", async {
            self.registry.remove(&request_id).await;
            Ok(())
        })
        .await;

        let mut headers = BTreeMap::new();
        headers.insert(
            self.config.settings.request_id_header.clone(),
            context.request_id,
        );
        if self.config.settings.security_headers.enabled {
            headers.insert("x-content-type-options".into(), "nosniff".into());
            headers.insert(
                "x-frame-options".into(),
                self.config.settings.security_headers.frame_options.clone(),
            );
            headers.insert(
                "referrer-policy".into(),
                "strict-origin-when-cross-origin".into(),
            );
            headers.insert(
                "strict-transport-security".into(),
                format!(
                    "max-age={}; includeSubDomains",
                    self.config.settings.security_headers.hsts_max_age_seconds
                ),
            );
            if let Some(csp) = &self
                .config
                .settings
                .security_headers
                .content_security_policy
            {
                headers.insert("content-security-policy".into(), csp.clone());
            }
        }
        headers
    }
}

async fn run_lifecycle_stage<F, T>(
    context: &RequestContext,
    operation: &'static str,
    future: F,
) -> Result<T, MiddlewareError>
where
    F: Future<Output = Result<T, MiddlewareError>>,
{
    let request_id = context.request_id.clone();
    let trace_id = context.trace_id.clone();
    let span = tracing::info_span!(
        "ores.middleware.lifecycle",
        request_id = %context.request_id,
        trace_id = %context.trace_id,
        span_id = %context.span_id.as_deref().unwrap_or(""),
        user_id = %context.user_id.as_deref().unwrap_or(""),
        tenant_id = %context.tenant_id.as_deref().unwrap_or(""),
        operation_name = operation,
        operation_transport = "http",
        operation_scope = "request",
    );
    let guarded = async move {
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(
                    operation_outcome = "panic",
                    error_type = "panic",
                    failure_code = "middleware_panicked",
                    request_id = %request_id,
                    trace_id = %trace_id,
                    "middleware lifecycle stage failed"
                );
                Err(MiddlewareError::new(
                    500,
                    "internal_error",
                    "request processing failed",
                ))
            }
        }
    }
    .instrument(span);

    run_with_context(
        context.clone(),
        run_with_ores_log_context(context, guarded),
    )
    .await
}

fn correlate_error(
    config: &MiddlewareConfig,
    context: &RequestContext,
    mut error: MiddlewareError,
) -> MiddlewareError {
    error
        .headers
        .entry(config.settings.request_id_header.clone())
        .or_insert_with(|| context.request_id.clone());
    error
}

''',
)

replace_once(
    "src/rust/src/pipeline.rs",
    '''    use std::{future::Future, pin::Pin};''',
    '''    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };''',
)

replace_once(
    "src/rust/src/pipeline.rs",
    '''        default_config,
        integrations::RateLimiter,
        rate_limit::{RateLimitAlgorithm, RateLimitLayer},
    };''',
    '''        current_context, default_config,
        integrations::{
            AuthDecision, AuthVerifier, RateLimiter, ResponseMetadata, TelemetrySink,
        },
        otel::current_log_context,
        rate_limit::{RateLimitAlgorithm, RateLimitLayer},
    };''',
)

replace_once(
    "src/rust/src/pipeline.rs",
    '''    #[tokio::test]
    async fn finish_does_not_synthesize_response_traceparent_without_server_span() {''',
    '''    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ScopeObservation {
        phase: &'static str,
        request_id: Option<String>,
        user_id: Option<String>,
        log_request_id: Option<String>,
        log_user_id: Option<String>,
    }

    fn observe_scope(phase: &'static str) -> ScopeObservation {
        let current = current_context();
        let log_context = current_log_context();
        ScopeObservation {
            phase,
            request_id: current.as_ref().map(|value| value.request_id.clone()),
            user_id: current.and_then(|value| value.user_id),
            log_request_id: log_context
                .fields
                .get("request.id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            log_user_id: log_context
                .fields
                .get("user.id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        }
    }

    struct PanickingAuth {
        observations: Arc<Mutex<Vec<ScopeObservation>>>,
    }

    impl AuthVerifier for PanickingAuth {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
            Box::pin(async move {
                self.observations
                    .lock()
                    .expect("observation lock")
                    .push(observe_scope("auth"));
                panic!("private authentication detail")
            })
        }
    }

    struct StaticAuth;

    impl AuthVerifier for StaticAuth {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
            Box::pin(async {
                Ok(AuthDecision {
                    user_id: Some("user-42".into()),
                    tenant_id: Some("tenant-7".into()),
                    claims: BTreeMap::from([
                        ("otel.plan".into(), "pro".into()),
                        ("private".into(), "must-not-propagate".into()),
                    ]),
                })
            })
        }
    }

    struct PanickingFinishedTelemetry {
        observations: Arc<Mutex<Vec<ScopeObservation>>>,
    }

    impl TelemetrySink for PanickingFinishedTelemetry {
        fn request_started<'a>(
            &'a self,
            _context: &'a RequestContext,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.observations
                    .lock()
                    .expect("observation lock")
                    .push(observe_scope("started"));
            })
        }

        fn request_finished<'a>(
            &'a self,
            _context: &'a RequestContext,
            _request: &'a RequestMetadata,
            _response: &'a ResponseMetadata,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.observations
                    .lock()
                    .expect("observation lock")
                    .push(observe_scope("finished"));
                panic!("private telemetry detail")
            })
        }
    }

    fn lifecycle_config() -> MiddlewareConfig {
        let mut config = default_config("lifecycle-boundary-test");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.enabled = false;
        config
    }

    #[tokio::test]
    async fn authentication_panic_is_correlated_inside_base_task_and_log_context() {
        let observations = Arc::new(Mutex::new(Vec::new()));
        let stack = MiddlewareStack::new(lifecycle_config())
            .unwrap()
            .with_auth_verifier(Arc::new(PanickingAuth {
                observations: observations.clone(),
            }));
        let mut metadata = request("198.51.100.10", None, true);
        metadata
            .headers
            .insert("x-request-id".into(), "auth-panic".into());

        let error = match stack.begin(metadata).await {
            Ok(_) => panic!("panicking auth must not produce an active request"),
            Err(error) => error,
        };

        assert_eq!(error.status, 500);
        assert_eq!(error.code, "internal_error");
        assert_eq!(error.message, "request processing failed");
        assert_eq!(
            error.headers.get("x-request-id").map(String::as_str),
            Some("auth-panic")
        );
        assert_eq!(
            observations.lock().expect("observation lock").as_slice(),
            &[ScopeObservation {
                phase: "auth",
                request_id: Some("auth-panic".into()),
                user_id: None,
                log_request_id: Some("auth-panic".into()),
                log_user_id: None,
            }]
        );
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn finalization_panic_retains_actor_context_and_registry_cleanup() {
        let observations = Arc::new(Mutex::new(Vec::new()));
        let stack = MiddlewareStack::new(lifecycle_config())
            .unwrap()
            .with_auth_verifier(Arc::new(StaticAuth))
            .with_telemetry(Arc::new(PanickingFinishedTelemetry {
                observations: observations.clone(),
            }));
        let mut metadata = request("198.51.100.10", None, true);
        metadata
            .headers
            .insert("x-request-id".into(), "finish-panic".into());

        let active = stack.begin(metadata).await.expect("active request");
        assert!(stack.registry.get("finish-panic").await.is_some());
        let headers = stack.finish(active, 200, Some(2)).await;

        assert_eq!(
            headers.get("x-request-id").map(String::as_str),
            Some("finish-panic")
        );
        assert!(stack.registry.get("finish-panic").await.is_none());
        let observations = observations.lock().expect("observation lock");
        assert_eq!(observations.len(), 2);
        for observation in observations.iter() {
            assert_eq!(observation.request_id.as_deref(), Some("finish-panic"));
            assert_eq!(observation.user_id.as_deref(), Some("user-42"));
            assert_eq!(observation.log_request_id.as_deref(), Some("finish-panic"));
            assert_eq!(observation.log_user_id.as_deref(), Some("user-42"));
        }
        assert_eq!(observations[0].phase, "started");
        assert_eq!(observations[1].phase, "finished");
        assert!(current_context().is_none());
        assert_eq!(current_log_context(), Default::default());
    }

    #[tokio::test]
    async fn finish_does_not_synthesize_response_traceparent_without_server_span() {''',
)

replace_once(
    "src/rust/src/frameworks/axum.rs",
    '''use std::{
    collections::BTreeMap,
    net::SocketAddr,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};''',
    '''use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};''',
)

replace_once(
    "src/rust/src/frameworks/axum.rs",
    '''use futures_util::FutureExt;
''',
    '''''',
)

replace_once(
    "src/rust/src/frameworks/axum.rs",
    '''use crate::{
    context::run_with_context,
    integrations::{RequestMetadata, TransportSecurity},
    otel::{run_with_ores_log_context, RequestLogger},
    stack_from_env, BootstrapError, MiddlewareError, MiddlewareStack,
};''',
    '''use crate::{
    integrations::{RequestMetadata, TransportSecurity},
    operation::{
        run_operation_boundary_with_timeout, OperationDescriptor, OperationFailureKind,
        OperationOutcome, OperationScope, OperationTransport,
    },
    otel::RequestLogger,
    stack_from_env, BootstrapError, MiddlewareError, MiddlewareStack,
};''',
)

replace_range(
    "src/rust/src/frameworks/axum.rs",
    '''    let context = active.context.clone();
''',
    '''    let headers = state
''',
    '''    let context = active.context.clone();
    let timeout = Duration::from_millis(state.stack.config().settings.timeout_ms);
    let started = Instant::now();
    let outcome = run_operation_boundary_with_timeout(
        context,
        OperationDescriptor {
            transport: OperationTransport::Http,
            scope: OperationScope::Request,
            name: "middleware.handler".into(),
        },
        timeout,
        async move { Ok::<_, Infallible>(next.run(request).await) },
    )
    .await;

    let mut response = match outcome {
        OperationOutcome::Completed(response) => {
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .info(vec![Value::String("request handler completed".into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            "completed",
                            started.elapsed(),
                            Some(response.status().as_u16()),
                        )),
                    "completed",
                );
            }
            response
        }
        OperationOutcome::Failed(failure) => {
            let (outcome, status, code, detail) = match failure.kind {
                OperationFailureKind::DeadlineExceeded => (
                    "timeout",
                    504,
                    "deadline_exceeded",
                    "request deadline exceeded",
                ),
                OperationFailureKind::Cancelled => (
                    "cancelled",
                    499,
                    "request_cancelled",
                    "request was cancelled",
                ),
                OperationFailureKind::Error | OperationFailureKind::Panic => (
                    "panic",
                    500,
                    "internal_error",
                    "request handler failed",
                ),
            };
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .error(vec![Value::String("request handler failed".into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            outcome,
                            started.elapsed(),
                            Some(status),
                        )),
                    outcome,
                );
            }
            problem(MiddlewareError::new(status, code, detail))
        }
    };
''',
)
