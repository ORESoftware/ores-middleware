use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use crate::{
    AuthVerifier, IntegrationError, RateLimiter, RequestContext, RequestMetadata, StageResponse,
    TelemetrySink,
};

/// Provider-neutral outbound HTTP request used by edge-compatible middleware.
///
/// Host adapters translate this value to their native Fetch/HTTP client. The
/// middleware callback therefore never imports a Cloudflare, Node, Deno, Fastly,
/// Hyper, Reqwest, or other provider-specific client SDK.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MiddlewareFetchRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl MiddlewareFetchRequest {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MiddlewareFetchResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub trait MiddlewareFetchProvider: Send + Sync {
    fn fetch<'a>(
        &'a self,
        request: MiddlewareFetchRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<MiddlewareFetchResponse, IntegrationError>> + Send + 'a,
        >,
    >;
}

/// Bounded key/value cache surface that can be backed by a Worker cache, KV,
/// Redis/LRU provider, or an in-process implementation without changing the
/// middleware callback API.
pub trait MiddlewareCacheProvider: Send + Sync {
    fn get<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, IntegrationError>> + Send + 'a>>;

    fn set<'a>(
        &'a self,
        key: &'a str,
        value: Vec<u8>,
        ttl_ms: Option<u64>,
    ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>>;

    fn delete<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>>;
}

/// The complete dependency bundle approved for portable P2 middleware.
///
/// Consumers receive these providers as callback parameters. Concrete runtime
/// SDKs stay inside the host adapter, so middleware source does not need to
/// import provider-specific auth, fetch, cache, rate-limit, or telemetry SDKs.
#[derive(Clone)]
pub struct EdgeMiddlewareDependencies {
    pub fetch: Arc<dyn MiddlewareFetchProvider>,
    pub auth: Arc<dyn AuthVerifier>,
    pub rate_limiter: Arc<dyn RateLimiter>,
    pub cache: Arc<dyn MiddlewareCacheProvider>,
    pub telemetry: Arc<dyn TelemetrySink>,
}

impl EdgeMiddlewareDependencies {
    #[must_use]
    pub fn new(
        fetch: Arc<dyn MiddlewareFetchProvider>,
        auth: Arc<dyn AuthVerifier>,
        rate_limiter: Arc<dyn RateLimiter>,
        cache: Arc<dyn MiddlewareCacheProvider>,
        telemetry: Arc<dyn TelemetrySink>,
    ) -> Self {
        Self {
            fetch,
            auth,
            rate_limiter,
            cache,
            telemetry,
        }
    }
}

/// Parameters injected into an `edge_minimal` callback.
///
/// There is deliberately no `next`, response, filesystem, socket, process, DB,
/// or raw-handle field. The callback may mutate request metadata, use approved
/// providers, short-circuit, or return the request for handoff to P3.
#[derive(Clone)]
pub struct EdgeMinimalCallbackArgs {
    pub request: RequestMetadata,
    pub context: RequestContext,
    pub deps: EdgeMiddlewareDependencies,
}

impl EdgeMinimalCallbackArgs {
    pub fn set_request_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.request.headers.insert(name.into(), value.into());
    }
}

#[derive(Debug, Clone)]
pub enum EdgeMinimalDecision {
    Continue(RequestMetadata),
    Respond(StageResponse),
}

pub trait EdgeMinimalMiddleware: Send + Sync {
    fn call<'a>(
        &'a self,
        args: EdgeMinimalCallbackArgs,
    ) -> Pin<
        Box<dyn Future<Output = Result<EdgeMinimalDecision, IntegrationError>> + Send + 'a>,
    >;
}

/// Closure adapter so consumers normally provide a callback rather than define
/// a new middleware type.
pub struct FnEdgeMinimalMiddleware<F> {
    callback: F,
}

impl<F> FnEdgeMinimalMiddleware<F> {
    #[must_use]
    pub const fn new(callback: F) -> Self {
        Self { callback }
    }
}

#[must_use]
pub fn edge_minimal_middleware_fn<F, Fut>(callback: F) -> FnEdgeMinimalMiddleware<F>
where
    F: Fn(EdgeMinimalCallbackArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<EdgeMinimalDecision, IntegrationError>> + Send + 'static,
{
    FnEdgeMinimalMiddleware::new(callback)
}

impl<F, Fut> EdgeMinimalMiddleware for FnEdgeMinimalMiddleware<F>
where
    F: Fn(EdgeMinimalCallbackArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<EdgeMinimalDecision, IntegrationError>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        args: EdgeMinimalCallbackArgs,
    ) -> Pin<
        Box<dyn Future<Output = Result<EdgeMinimalDecision, IntegrationError>> + Send + 'a>,
    > {
        Box::pin((self.callback)(args))
    }
}

/// Abstract downstream continuation used only by `edge_fetch`. Host adapters
/// implement this using their native Fetch-style `next(request)` mechanism.
pub trait EdgeNext: Send + Sync {
    fn call<'a>(
        &'a self,
        request: RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<StageResponse, IntegrationError>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct EdgeFetchCallbackArgs {
    pub request: RequestMetadata,
    pub context: RequestContext,
    pub deps: EdgeMiddlewareDependencies,
    pub next: Arc<dyn EdgeNext>,
}

impl EdgeFetchCallbackArgs {
    pub fn set_request_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.request.headers.insert(name.into(), value.into());
    }
}

pub trait EdgeFetchMiddleware: Send + Sync {
    fn call<'a>(
        &'a self,
        args: EdgeFetchCallbackArgs,
    ) -> Pin<Box<dyn Future<Output = Result<StageResponse, IntegrationError>> + Send + 'a>>;
}

pub struct FnEdgeFetchMiddleware<F> {
    callback: F,
}

impl<F> FnEdgeFetchMiddleware<F> {
    #[must_use]
    pub const fn new(callback: F) -> Self {
        Self { callback }
    }
}

#[must_use]
pub fn edge_fetch_middleware_fn<F, Fut>(callback: F) -> FnEdgeFetchMiddleware<F>
where
    F: Fn(EdgeFetchCallbackArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<StageResponse, IntegrationError>> + Send + 'static,
{
    FnEdgeFetchMiddleware::new(callback)
}

impl<F, Fut> EdgeFetchMiddleware for FnEdgeFetchMiddleware<F>
where
    F: Fn(EdgeFetchCallbackArgs) -> Fut + Send + Sync,
    Fut: Future<Output = Result<StageResponse, IntegrationError>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        args: EdgeFetchCallbackArgs,
    ) -> Pin<Box<dyn Future<Output = Result<StageResponse, IntegrationError>> + Send + 'a>> {
        Box::pin((self.callback)(args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthDecision, InMemoryTokenBucket, RateLimiter, ResponseMetadata, TelemetrySink,
    };

    struct TestFetch;

    impl MiddlewareFetchProvider for TestFetch {
        fn fetch<'a>(
            &'a self,
            request: MiddlewareFetchRequest,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<MiddlewareFetchResponse, IntegrationError>> + Send + 'a,
            >,
        > {
            Box::pin(async move {
                Ok(MiddlewareFetchResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: request.url.into_bytes(),
                })
            })
        }
    }

    struct TestAuth;

    impl AuthVerifier for TestAuth {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
            Box::pin(async {
                Ok(AuthDecision {
                    user_id: Some("user-1".into()),
                    tenant_id: None,
                    claims: BTreeMap::new(),
                })
            })
        }
    }

    struct TestCache;

    impl MiddlewareCacheProvider for TestCache {
        fn get<'a>(
            &'a self,
            _key: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, IntegrationError>> + Send + 'a>> {
            Box::pin(async { Ok(None) })
        }

        fn set<'a>(
            &'a self,
            _key: &'a str,
            _value: Vec<u8>,
            _ttl_ms: Option<u64>,
        ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }

        fn delete<'a>(
            &'a self,
            _key: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>> {
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

    fn request() -> RequestMetadata {
        RequestMetadata {
            method: "GET".into(),
            path: "/".into(),
            headers: BTreeMap::new(),
            remote_ip: None,
            content_length: None,
            transport_secure: true,
        }
    }

    fn context() -> RequestContext {
        RequestContext {
            request_id: "req-1".into(),
            trace_id: "trace-1".into(),
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
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

    #[tokio::test]
    async fn edge_minimal_callback_receives_dependencies_without_runtime_sdk_types() {
        let middleware = edge_minimal_middleware_fn(|mut args: EdgeMinimalCallbackArgs| async move {
            let response = args
                .deps
                .fetch
                .fetch(MiddlewareFetchRequest::get("https://example.test/auth"))
                .await?;
            assert_eq!(response.status, 200);
            let auth = args.deps.auth.verify(&args.request).await?;
            args.set_request_header("x-user-id", auth.user_id.unwrap_or_default());
            Ok(EdgeMinimalDecision::Continue(args.request))
        });

        let result = middleware
            .call(EdgeMinimalCallbackArgs {
                request: request(),
                context: context(),
                deps: deps(),
            })
            .await
            .expect("callback");

        let EdgeMinimalDecision::Continue(request) = result else {
            panic!("expected continue");
        };
        assert_eq!(request.headers.get("x-user-id").map(String::as_str), Some("user-1"));
    }
}
