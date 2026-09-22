use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};

use uuid::Uuid;

use crate::{
    EdgeMiddlewareDependencies, IntegrationError, LocalEdgeMinimalHost, LocalEdgeMinimalResult,
    MiddlewareHostFinishRequest, MiddlewareHostFinishResult, MiddlewareHostOutcome,
    MiddlewareHostRequest, RequestContext, RequestMetadata, MIDDLEWARE_HOST_ABI_SCHEMA,
    provider_api::MiddlewareResultFuture,
};

/// Terminal metadata injected into deferred `edge_minimal` finalization.
///
/// This surface intentionally contains no downstream response body, `EdgeNext`,
/// socket, process, filesystem, or provider SDK handle. Finalizers receive only
/// normalized request/context facts, the same approved dependency bundle as the
/// admission callback, and bounded terminal metadata from P1.
#[derive(Clone)]
pub struct EdgeMinimalFinishArgs {
    pub request: RequestMetadata,
    pub context: RequestContext,
    pub deps: EdgeMiddlewareDependencies,
    pub status: u16,
    pub response_bytes: Option<u64>,
    pub outcome: MiddlewareHostOutcome,
}

/// Deferred finalization hook for request-side `edge_minimal` middleware.
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

/// Closure adapter for consumers that want deferred cleanup/telemetry without
/// defining a dedicated finalizer type.
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

/// Result returned by the lifecycle-aware `edge_minimal` begin phase.
///
/// The opaque session id is the only token P1 needs to retain while P3 runs.
/// The nested result remains the request-side decision from `LocalEdgeMinimalHost`.
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

/// Lifecycle wrapper for the local `edge_minimal` host used by P2.
///
/// P2 remains request-side only: `begin` may mutate/short-circuit the request,
/// then P1 owns downstream P3 execution and response streaming. After P3 reaches
/// a terminal state, P1 sends a bounded `MiddlewareHostFinishRequest`; `finish`
/// removes the opaque session before awaiting the finalizer so double-finish and
/// races fail closed.
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

    /// Run request-side admission and retain only the state required for a later
    /// terminal finalization. The response body never crosses this boundary.
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

        let session_id = Uuid::new_v4().to_string();
        let previous = self.lock_active()?.insert(
            session_id.clone(),
            ActiveEdgeMinimalSession {
                request: finalizer_request,
                context,
            },
        );
        if previous.is_some() {
            return Err(IntegrationError {
                code: "session_collision",
                message: "edge_minimal lifecycle host generated a duplicate session id".to_owned(),
            });
        }

        Ok(LocalEdgeMinimalBeginResult { session_id, result })
    }

    /// Finalize exactly one admitted request after downstream completion,
    /// disconnect, timeout, or child failure.
    ///
    /// Session removal happens before awaiting consumer finalization. A failing
    /// finalizer therefore cannot leave a replayable session behind.
    pub async fn finish(
        &self,
        request: MiddlewareHostFinishRequest,
    ) -> Result<MiddlewareHostFinishResult, IntegrationError> {
        request.validate().map_err(host_abi_error_as_integration)?;
        let active = self
            .lock_active()?
            .remove(&request.session_id)
            .ok_or_else(|| IntegrationError {
                code: "unknown_session",
                message: "edge_minimal lifecycle session is unknown or already finalized".to_owned(),
            })?;

        self.finalizer
            .finish(EdgeMinimalFinishArgs {
                request: active.request,
                context: active.context,
                deps: self.deps.clone(),
                status: request.status,
                response_bytes: request.response_bytes,
                outcome: request.outcome,
            })
            .await?;

        Ok(MiddlewareHostFinishResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            outcome: request.outcome,
            response_headers: BTreeMap::new(),
        })
    }

    pub fn active_count(&self) -> Result<usize, IntegrationError> {
        Ok(self.lock_active()?.len())
    }

    fn lock_active(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<String, ActiveEdgeMinimalSession>>, IntegrationError> {
        self.active.lock().map_err(|_| IntegrationError {
            code: "host_state_poisoned",
            message: "edge_minimal lifecycle state is unavailable after a panic".to_owned(),
        })
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
    async fn begin_handoff_then_finish_runs_out_of_band_finalizer() {
        let middleware = edge_minimal_middleware_fn(|mut args| async move {
            args.set_request_header("x-ores-edge", "admitted");
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_by_finalizer = observed.clone();
        let finalizer = edge_minimal_finalizer_fn(move |args| {
            let observed = observed_by_finalizer.clone();
            async move {
                assert_eq!(args.request.headers.get("x-ores-edge").map(String::as_str), Some("admitted"));
                assert_eq!(args.status, 200);
                assert_eq!(args.response_bytes, Some(12));
                observed.lock().expect("observed").push(args.outcome);
                Ok(())
            }
        });
        let host = LocalEdgeMinimalLifecycleHost::with_finalizer(
            Arc::new(middleware),
            deps(),
            Arc::new(finalizer),
        );

        let begin = host.begin(secure_request(), context()).await.expect("begin");
        let LocalEdgeMinimalResult::Continue(request) = begin.result else {
            panic!("expected continue");
        };
        assert_eq!(request.headers.get("x-ores-edge").map(String::as_str), Some("admitted"));
        assert_eq!(host.active_count().unwrap(), 1);

        let finish = host
            .finish(MiddlewareHostFinishRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: begin.session_id,
                status: 200,
                response_bytes: Some(12),
                outcome: MiddlewareHostOutcome::Completed,
            })
            .await
            .expect("finish");
        assert_eq!(finish.outcome, MiddlewareHostOutcome::Completed);
        assert!(finish.response_headers.is_empty());
        assert_eq!(host.active_count().unwrap(), 0);
        assert_eq!(observed.lock().unwrap().as_slice(), &[MiddlewareHostOutcome::Completed]);
    }

    #[tokio::test]
    async fn all_abnormal_terminal_outcomes_use_same_finish_boundary() {
        for (status, outcome) in [
            (499, MiddlewareHostOutcome::Disconnected),
            (504, MiddlewareHostOutcome::TimedOut),
            (502, MiddlewareHostOutcome::ChildFailed),
        ] {
            let middleware = edge_minimal_middleware_fn(|args| async move {
                Ok(EdgeMinimalDecision::Continue(args.request))
            });
            let observed = Arc::new(Mutex::new(Vec::new()));
            let observed_by_finalizer = observed.clone();
            let finalizer = edge_minimal_finalizer_fn(move |args| {
                let observed = observed_by_finalizer.clone();
                async move {
                    observed.lock().expect("observed").push(args.outcome);
                    Ok(())
                }
            });
            let host = LocalEdgeMinimalLifecycleHost::with_finalizer(
                Arc::new(middleware),
                deps(),
                Arc::new(finalizer),
            );
            let begin = host.begin(secure_request(), context()).await.expect("begin");
            host.finish(MiddlewareHostFinishRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: begin.session_id,
                status,
                response_bytes: None,
                outcome,
            })
            .await
            .expect("finish");
            assert_eq!(observed.lock().unwrap().as_slice(), &[outcome]);
            assert_eq!(host.active_count().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn double_finish_fails_closed_after_session_removal() {
        let middleware = edge_minimal_middleware_fn(|args| async move {
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalLifecycleHost::from_middleware(middleware, deps());
        let begin = host.begin(secure_request(), context()).await.expect("begin");
        let finish = MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id: begin.session_id,
            status: 200,
            response_bytes: Some(0),
            outcome: MiddlewareHostOutcome::Completed,
        };
        host.finish(finish.clone()).await.expect("first finish");
        let error = host.finish(finish).await.expect_err("double finish");
        assert_eq!(error.code, "unknown_session");
    }

    #[tokio::test]
    async fn short_circuit_still_requires_terminal_finish() {
        let middleware = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(crate::StageResponse {
                status: 403,
                headers: BTreeMap::new(),
                body: b"forbidden".to_vec(),
            }))
        });
        let host = LocalEdgeMinimalLifecycleHost::from_middleware(middleware, deps());
        let begin = host.begin(secure_request(), context()).await.expect("begin");
        assert!(matches!(begin.result, LocalEdgeMinimalResult::Respond(_)));
        assert_eq!(host.active_count().unwrap(), 1);
        host.finish(MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id: begin.session_id,
            status: 403,
            response_bytes: Some(9),
            outcome: MiddlewareHostOutcome::Completed,
        })
        .await
        .expect("finish");
        assert_eq!(host.active_count().unwrap(), 0);
    }
}
