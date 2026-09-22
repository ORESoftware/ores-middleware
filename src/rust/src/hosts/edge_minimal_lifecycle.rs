use std::{
    collections::{BTreeMap, btree_map::Entry},
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};

use uuid::Uuid;

use crate::{
    EdgeMiddlewareDependencies, IntegrationError, LocalEdgeMinimalHost, LocalEdgeMinimalResult,
    MiddlewareHostOutcome, MiddlewareHostRequest, RequestContext, RequestMetadata,
    provider_api::MiddlewareResultFuture,
};

const MAX_EDGE_MINIMAL_ACTIVE_SESSIONS: usize = 4096;
const MAX_EDGE_MINIMAL_SESSION_ID_BYTES: usize = 128;

/// P1-observed terminal metadata for a request admitted by `edge_minimal`.
///
/// This is intentionally *not* `MiddlewareHostFinishRequest`: that shared host
/// ABI models response-participating adapters and can require observations such
/// as response-head commitment or a body/transport completion boundary. A
/// request-only `edge_minimal` P2 cannot honestly make those observations.
///
/// P1 reports only facts it owns: terminal outcome, optional final status, the
/// number of response bytes it relayed, and elapsed request lifetime. No
/// downstream response body, headers, socket, process handle, or `EdgeNext`
/// crosses this boundary.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EdgeMinimalTerminalReport {
    pub session_id: String,
    pub outcome: MiddlewareHostOutcome,
    pub status: Option<u16>,
    pub response_bytes: u64,
    pub elapsed_ms: u64,
}

impl EdgeMinimalTerminalReport {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        if self.session_id.is_empty() || self.session_id.len() > MAX_EDGE_MINIMAL_SESSION_ID_BYTES {
            return Err(IntegrationError {
                code: "invalid_edge_minimal_session_id",
                message: "edge_minimal terminal report session id is empty or too long".to_owned(),
            });
        }
        if self
            .status
            .is_some_and(|status| !(200..=599).contains(&status))
        {
            return Err(IntegrationError {
                code: "invalid_edge_minimal_terminal_status",
                message: "edge_minimal terminal status must be a final HTTP status code".to_owned(),
            });
        }
        if self.outcome == MiddlewareHostOutcome::Completed && self.status.is_none() {
            return Err(IntegrationError {
                code: "missing_edge_minimal_terminal_status",
                message: "completed edge_minimal requests must report the final HTTP status"
                    .to_owned(),
            });
        }
        Ok(())
    }
}

/// Arguments injected into deferred `edge_minimal` finalization.
///
/// The finalizer receives the normalized admitted request/context, the same
/// approved provider-neutral dependency bundle as request admission, and only
/// P1-owned terminal metadata. It never receives the downstream response body.
#[derive(Clone)]
pub struct EdgeMinimalFinishArgs {
    pub request: RequestMetadata,
    pub context: RequestContext,
    pub deps: EdgeMiddlewareDependencies,
    pub outcome: MiddlewareHostOutcome,
    pub status: Option<u16>,
    pub response_bytes: u64,
    pub elapsed_ms: u64,
}

/// Optional out-of-band cleanup/telemetry hook for request-side middleware.
pub trait EdgeMinimalFinalizer: Send + Sync {
    fn finish<'a>(&'a self, args: EdgeMinimalFinishArgs) -> MiddlewareResultFuture<'a, ()>;
}

#[derive(Default)]
pub struct NoopEdgeMinimalFinalizer;

impl EdgeMinimalFinalizer for NoopEdgeMinimalFinalizer {
    fn finish<'a>(&'a self, _args: EdgeMinimalFinishArgs) -> MiddlewareResultFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// Closure adapter for consumers that need deferred cleanup/telemetry without
/// importing a provider SDK or defining a dedicated finalizer type.
pub struct FnEdgeMinimalFinalizer<F> {
    callback: F,
}

impl<F> FnEdgeMinimalFinalizer<F> {
    #[must_use]
    pub const fn new(callback: F) -> Self {
        Self { callback }
    }
}

#[must_use]
pub fn edge_minimal_finalizer_fn<F, Fut>(callback: F) -> FnEdgeMinimalFinalizer<F>
where
    F: Fn(EdgeMinimalFinishArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), IntegrationError>> + Send + 'static,
{
    FnEdgeMinimalFinalizer::new(callback)
}

impl<F, Fut> EdgeMinimalFinalizer for FnEdgeMinimalFinalizer<F>
where
    F: Fn(EdgeMinimalFinishArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), IntegrationError>> + Send + 'static,
{
    fn finish<'a>(&'a self, args: EdgeMinimalFinishArgs) -> MiddlewareResultFuture<'a, ()> {
        Box::pin(async move { (self.callback)(args).await })
    }
}

/// Result of lifecycle-aware request admission.
#[derive(Debug, Clone)]
pub struct LocalEdgeMinimalBeginResult {
    pub session_id: String,
    pub result: LocalEdgeMinimalResult,
}

#[derive(Clone)]
struct ActiveEdgeMinimalSession {
    request: RequestMetadata,
    context: RequestContext,
}

/// Lifecycle wrapper for a request-side `LocalEdgeMinimalHost`.
///
/// `begin` admits or short-circuits the request. P1 then owns the sibling P3
/// process and all HTTP response framing/streaming. Once P1 reaches a terminal
/// state it sends one [`EdgeMinimalTerminalReport`]. Session state is removed
/// before consumer finalization is awaited, so replay/double-finish fails closed.
pub struct LocalEdgeMinimalLifecycleHost<M> {
    inner: LocalEdgeMinimalHost<M>,
    deps: EdgeMiddlewareDependencies,
    finalizer: Arc<dyn EdgeMinimalFinalizer>,
    active: Arc<Mutex<BTreeMap<String, ActiveEdgeMinimalSession>>>,
}

impl<M> LocalEdgeMinimalLifecycleHost<M>
where
    M: crate::EdgeMinimalMiddleware,
{
    #[must_use]
    pub fn new(middleware: Arc<M>, deps: EdgeMiddlewareDependencies) -> Self {
        Self::with_finalizer(middleware, deps, Arc::new(NoopEdgeMinimalFinalizer))
    }

    #[must_use]
    pub fn from_middleware(middleware: M, deps: EdgeMiddlewareDependencies) -> Self {
        Self::new(Arc::new(middleware), deps)
    }

    #[must_use]
    pub fn with_finalizer(
        middleware: Arc<M>,
        deps: EdgeMiddlewareDependencies,
        finalizer: Arc<dyn EdgeMinimalFinalizer>,
    ) -> Self {
        Self {
            inner: LocalEdgeMinimalHost::new(middleware, deps.clone()),
            deps,
            finalizer,
            active: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Run request-side admission and retain only state required for optional
    /// deferred finalization. No downstream response data enters this method.
    pub async fn begin(
        &self,
        request: MiddlewareHostRequest,
        context: RequestContext,
    ) -> Result<LocalEdgeMinimalBeginResult, IntegrationError> {
        let original_request = request
            .clone()
            .into_request_metadata()
            .map_err(host_abi_error_as_integration)?;
        let result = self.inner.execute(request, context.clone()).await?;
        let finalizer_request = match &result {
            LocalEdgeMinimalResult::Continue(request) => request.clone(),
            LocalEdgeMinimalResult::Respond(_) => original_request,
        };

        let mut active = self.lock_active()?;
        if active.len() >= MAX_EDGE_MINIMAL_ACTIVE_SESSIONS {
            return Err(IntegrationError {
                code: "edge_minimal_session_capacity",
                message: "edge_minimal lifecycle host reached its active-session limit".to_owned(),
            });
        }

        let session = ActiveEdgeMinimalSession {
            request: finalizer_request,
            context,
        };
        let session_id = loop {
            let candidate = Uuid::new_v4().to_string();
            if let Entry::Vacant(entry) = active.entry(candidate.clone()) {
                entry.insert(session.clone());
                break candidate;
            }
        };
        drop(active);

        Ok(LocalEdgeMinimalBeginResult { session_id, result })
    }

    /// Finalize exactly one request after P1 observes completion, disconnect,
    /// timeout, or P3 failure.
    ///
    /// The terminal report is validated before session removal. Once accepted,
    /// the session is removed before awaiting the consumer finalizer so a failing
    /// callback cannot leave replayable lifecycle state behind.
    pub async fn finish(&self, report: EdgeMinimalTerminalReport) -> Result<(), IntegrationError> {
        report.validate()?;
        let active = self
            .lock_active()?
            .remove(&report.session_id)
            .ok_or_else(|| IntegrationError {
                code: "unknown_edge_minimal_session",
                message: "edge_minimal lifecycle session is unknown or already finalized"
                    .to_owned(),
            })?;

        self.finalizer
            .finish(EdgeMinimalFinishArgs {
                request: active.request,
                context: active.context,
                deps: self.deps.clone(),
                outcome: report.outcome,
                status: report.status,
                response_bytes: report.response_bytes,
                elapsed_ms: report.elapsed_ms,
            })
            .await
    }

    fn lock_active(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<String, ActiveEdgeMinimalSession>>, IntegrationError> {
        self.active.lock().map_err(|_| IntegrationError {
            code: "edge_minimal_host_state_poisoned",
            message: "edge_minimal lifecycle state is unavailable after a panic".to_owned(),
        })
    }

    #[cfg(test)]
    fn active_count(&self) -> Result<usize, IntegrationError> {
        Ok(self.lock_active()?.len())
    }
}

fn host_abi_error_as_integration(error: crate::MiddlewareHostAbiError) -> IntegrationError {
    IntegrationError {
        code: error.code,
        message: error.message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthDecision, AuthVerifier, EdgeMinimalDecision, InMemoryTokenBucket,
        MiddlewareCacheProvider, MiddlewareFetchProvider, MiddlewareFetchRequest,
        MiddlewareFetchResponse, RateLimiter, ResponseMetadata, TelemetrySink,
        edge_minimal_middleware_fn,
    };
    use std::{collections::BTreeMap, pin::Pin};

    struct TestFetch;

    impl MiddlewareFetchProvider for TestFetch {
        fn fetch<'a>(
            &'a self,
            _request: MiddlewareFetchRequest,
        ) -> MiddlewareResultFuture<'a, MiddlewareFetchResponse> {
            Box::pin(async {
                Ok(MiddlewareFetchResponse {
                    status: 204,
                    headers: BTreeMap::new(),
                    body: Vec::new(),
                })
            })
        }
    }

    struct TestAuth;

    impl AuthVerifier for TestAuth {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
        ) -> MiddlewareResultFuture<'a, AuthDecision> {
            Box::pin(async { Ok(AuthDecision::default()) })
        }
    }

    struct TestCache;

    impl MiddlewareCacheProvider for TestCache {
        fn get<'a>(&'a self, _key: &'a str) -> MiddlewareResultFuture<'a, Option<Vec<u8>>> {
            Box::pin(async { Ok(None) })
        }

        fn set<'a>(
            &'a self,
            _key: &'a str,
            _value: Vec<u8>,
            _ttl_ms: Option<u64>,
        ) -> MiddlewareResultFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn delete<'a>(&'a self, _key: &'a str) -> MiddlewareResultFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    struct TestTelemetry;

    impl TelemetrySink for TestTelemetry {
        fn request_started<'a>(
            &'a self,
            _context: &'a RequestContext,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }

        fn request_finished<'a>(
            &'a self,
            _context: &'a RequestContext,
            _request: &'a RequestMetadata,
            _response: &'a ResponseMetadata,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }
    }

    fn deps() -> EdgeMiddlewareDependencies {
        EdgeMiddlewareDependencies::new(
            Arc::new(TestFetch),
            Arc::new(TestAuth),
            Arc::new(InMemoryTokenBucket::default()) as Arc<dyn RateLimiter>,
            Arc::new(TestCache),
            Arc::new(TestTelemetry),
        )
    }

    fn context() -> RequestContext {
        RequestContext {
            request_id: "req-lifecycle".to_owned(),
            trace_id: "trace-lifecycle".to_owned(),
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
        }
    }

    fn secure_request() -> MiddlewareHostRequest {
        let mut request = MiddlewareHostRequest::new("GET", "/v1/rpc");
        request.trusted_transport_secure = true;
        request
    }

    #[tokio::test]
    async fn begin_handoff_then_finish_runs_metadata_only_finalizer() {
        let middleware = edge_minimal_middleware_fn(|mut args| async move {
            args.set_request_header("x-ores-edge", "admitted");
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_by_finalizer = observed.clone();
        let finalizer = edge_minimal_finalizer_fn(move |args| {
            let observed = observed_by_finalizer.clone();
            async move {
                assert_eq!(
                    args.request.headers.get("x-ores-edge").map(String::as_str),
                    Some("admitted")
                );
                assert_eq!(args.status, Some(200));
                assert_eq!(args.response_bytes, 12);
                assert_eq!(args.elapsed_ms, 9);
                observed.lock().expect("observed").push(args.outcome);
                Ok(())
            }
        });
        let host = LocalEdgeMinimalLifecycleHost::with_finalizer(
            Arc::new(middleware),
            deps(),
            Arc::new(finalizer),
        );

        let begin = host
            .begin(secure_request(), context())
            .await
            .expect("begin");
        assert!(matches!(begin.result, LocalEdgeMinimalResult::Continue(_)));
        assert_eq!(host.active_count().unwrap(), 1);

        host.finish(EdgeMinimalTerminalReport {
            session_id: begin.session_id,
            outcome: MiddlewareHostOutcome::Completed,
            status: Some(200),
            response_bytes: 12,
            elapsed_ms: 9,
        })
        .await
        .expect("finish");
        assert_eq!(host.active_count().unwrap(), 0);
        assert_eq!(
            observed.lock().unwrap().as_slice(),
            &[MiddlewareHostOutcome::Completed]
        );
    }

    #[tokio::test]
    async fn abnormal_outcomes_do_not_require_fabricated_http_status() {
        for outcome in [
            MiddlewareHostOutcome::Disconnected,
            MiddlewareHostOutcome::TimedOut,
            MiddlewareHostOutcome::ChildFailed,
        ] {
            let middleware = edge_minimal_middleware_fn(|args| async move {
                Ok(EdgeMinimalDecision::Continue(args.request))
            });
            let observed = Arc::new(Mutex::new(Vec::new()));
            let observed_by_finalizer = observed.clone();
            let finalizer = edge_minimal_finalizer_fn(move |args| {
                let observed = observed_by_finalizer.clone();
                async move {
                    assert_eq!(args.status, None);
                    observed.lock().expect("observed").push(args.outcome);
                    Ok(())
                }
            });
            let host = LocalEdgeMinimalLifecycleHost::with_finalizer(
                Arc::new(middleware),
                deps(),
                Arc::new(finalizer),
            );
            let begin = host
                .begin(secure_request(), context())
                .await
                .expect("begin");
            host.finish(EdgeMinimalTerminalReport {
                session_id: begin.session_id,
                outcome,
                status: None,
                response_bytes: 0,
                elapsed_ms: 90_000,
            })
            .await
            .expect("finish");
            assert_eq!(observed.lock().unwrap().as_slice(), &[outcome]);
            assert_eq!(host.active_count().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn invalid_terminal_report_does_not_consume_session() {
        let middleware = edge_minimal_middleware_fn(|args| async move {
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalLifecycleHost::from_middleware(middleware, deps());
        let begin = host
            .begin(secure_request(), context())
            .await
            .expect("begin");
        let error = host
            .finish(EdgeMinimalTerminalReport {
                session_id: begin.session_id.clone(),
                outcome: MiddlewareHostOutcome::Completed,
                status: None,
                response_bytes: 0,
                elapsed_ms: 1,
            })
            .await
            .expect_err("completed report without status must fail");
        assert_eq!(error.code, "missing_edge_minimal_terminal_status");
        assert_eq!(host.active_count().unwrap(), 1);

        host.finish(EdgeMinimalTerminalReport {
            session_id: begin.session_id,
            outcome: MiddlewareHostOutcome::Completed,
            status: Some(204),
            response_bytes: 0,
            elapsed_ms: 2,
        })
        .await
        .expect("valid retry");
        assert_eq!(host.active_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn double_finish_fails_closed_after_session_removal() {
        let middleware = edge_minimal_middleware_fn(|args| async move {
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalLifecycleHost::from_middleware(middleware, deps());
        let begin = host
            .begin(secure_request(), context())
            .await
            .expect("begin");
        let report = EdgeMinimalTerminalReport {
            session_id: begin.session_id,
            outcome: MiddlewareHostOutcome::Completed,
            status: Some(200),
            response_bytes: 0,
            elapsed_ms: 1,
        };
        host.finish(report.clone()).await.expect("first finish");
        let error = host.finish(report).await.expect_err("double finish");
        assert_eq!(error.code, "unknown_edge_minimal_session");
    }

    #[tokio::test]
    async fn short_circuit_remains_active_until_delivery_reaches_terminal_state() {
        let middleware = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(crate::StageResponse {
                status: 403,
                headers: BTreeMap::new(),
                body: b"forbidden".to_vec(),
            }))
        });
        let host = LocalEdgeMinimalLifecycleHost::from_middleware(middleware, deps());
        let begin = host
            .begin(secure_request(), context())
            .await
            .expect("begin");
        assert!(matches!(begin.result, LocalEdgeMinimalResult::Respond(_)));
        assert_eq!(host.active_count().unwrap(), 1);
        host.finish(EdgeMinimalTerminalReport {
            session_id: begin.session_id,
            outcome: MiddlewareHostOutcome::Completed,
            status: Some(403),
            response_bytes: 9,
            elapsed_ms: 3,
        })
        .await
        .expect("finish");
        assert_eq!(host.active_count().unwrap(), 0);
    }
}
