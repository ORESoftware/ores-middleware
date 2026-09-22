use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use crate::{
    AuthVerifier, IntegrationError, RateLimiter, RequestContext, RequestMetadata, StageResponse,
    TelemetrySink,
};

/// Boxed async result used by provider-neutral middleware contracts.
///
/// Naming this shape keeps public provider/callback signatures readable without
/// weakening the Send/lifetime requirements that host adapters depend on.
pub type MiddlewareResultFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, IntegrationError>> + Send + 'a>>;

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
    ) -> MiddlewareResultFuture<'a, MiddlewareFetchResponse>;
}

/// Bounded key/value cache surface that can be backed by a Worker cache, KV,
/// Redis/LRU provider, or an in-process implementation without changing the
/// middleware callback API.
pub trait MiddlewareCacheProvider: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> MiddlewareResultFuture<'a, Option<Vec<u8>>>;

    fn set<'a>(
        &'a self,
        key: &'a str,
        value: Vec<u8>,
        ttl_ms: Option<u64>,
    ) -> MiddlewareResultFuture<'a, ()>;

    fn delete<'a>(&'a self, key: &'a str) -> MiddlewareResultFuture<'a, ()>;
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

/// Request surface admitted to default `edge_minimal` middleware.
///
/// Routing/body/transport facts are readable but cannot be rewritten through
/// this type. Authored middleware may mutate only request headers. The local
/// host still revalidates all ingress-owned facts after callback completion as
/// defense-in-depth, but ordinary consumers no longer discover this boundary by
/// tripping a runtime error after mutating a public `RequestMetadata` field.
#[derive(Debug, Clone)]
pub struct EdgeMinimalRequest {
    inner: RequestMetadata,
}

impl EdgeMinimalRequest {
    pub(crate) fn from_metadata(inner: RequestMetadata) -> Self {
        Self { inner }
    }

    pub(crate) fn into_metadata(self) -> RequestMetadata {
        self.inner
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.inner.method
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.inner.path
    }

    #[must_use]
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.inner.headers
    }

    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.inner.headers.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn remote_ip(&self) -> Option<&str> {
        self.inner.remote_ip.as_deref()
    }

    #[must_use]
    pub const fn content_length(&self) -> Option<u64> {
        self.inner.content_length
    }

    #[must_use]
    pub const fn transport_secure(&self) -> bool {
        self.inner.transport_secure
    }

    /// Read-only compatibility view for approved providers such as auth and
    /// telemetry adapters whose portable trait already accepts RequestMetadata.
    #[must_use]
    pub const fn as_metadata(&self) -> &RequestMetadata {
        &self.inner
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.inner.headers.insert(name.into(), value.into());
    }

    pub fn remove_header(&mut self, name: &str) -> Option<String> {
        self.inner.headers.remove(name)
    }
}

/// Parameters injected into an `edge_minimal` callback.
///
/// There is deliberately no `next`, response, filesystem, socket, process, DB,
/// or raw-handle field. The callback may read ingress-owned request facts,
/// mutate request headers, use approved providers, short-circuit, or return the
/// constrained request for handoff to P3.
#[derive(Clone)]
pub struct EdgeMinimalCallbackArgs {
    pub request: EdgeMinimalRequest,
    pub context: RequestContext,
    pub deps: EdgeMiddlewareDependencies,
}

impl EdgeMinimalCallbackArgs {
    pub fn set_request_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.request.set_header(name, value);
    }

    pub fn remove_request_header(&mut self, name: &str) -> Option<String> {
        self.request.remove_header(name)
    }
}

#[derive(Debug, Clone)]
pub enum EdgeMinimalDecision {
    Continue(EdgeMinimalRequest),
    Respond(StageResponse),
}

pub trait EdgeMinimalMiddleware: Send + Sync {
    fn call<'a>(
        &'a self,
        args: EdgeMinimalCallbackArgs,
    ) -> MiddlewareResultFuture<'a, EdgeMinimalDecision>;
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
    ) -> MiddlewareResultFuture<'a, EdgeMinimalDecision> {
        Box::pin(async move { (self.callback)(args).await })
    }
}

/// Abstract downstream continuation used only by `edge_fetch`. Host adapters
/// implement this using their native Fetch-style `next(request)` mechanism.
pub trait EdgeNext: Send + Sync {
    fn call<'a>(&'a self, request: RequestMetadata) -> MiddlewareResultFuture<'a, StageResponse>;
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
    fn call<'a>(&'a self, args: EdgeFetchCallbackArgs)
    -> MiddlewareResultFuture<'a, StageResponse>;
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
    ) -> MiddlewareResultFuture<'a, StageResponse> {
        Box::pin(async move { (self.callback)(args).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthDecision, InMemoryTokenBucket, RateLimiter, ResponseMetadata, TelemetrySink};

    struct TestFetch;

    impl MiddlewareFetchProvider for TestFetch {
        fn fetch<'a>(
            &'a self,
            request: MiddlewareFetchRequest,
        ) -> MiddlewareResultFuture<'a, MiddlewareFetchResponse> {
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
        ) -> MiddlewareResultFuture<'a, AuthDecision> {
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
    async fn edge_minimal_callback_receives_dependencies_and_constrained_request() {
        let middleware =
            edge_minimal_middleware_fn(|mut args: EdgeMinimalCallbackArgs| async move {
                assert_eq!(args.request.method(), "GET");
                assert_eq!(args.request.path(), "/");
                assert!(args.request.transport_secure());
                let response = args
                    .deps
                    .fetch
                    .fetch(MiddlewareFetchRequest::get("https://example.test/auth"))
                    .await?;
                assert_eq!(response.status, 200);
                let auth = args.deps.auth.verify(args.request.as_metadata()).await?;
                args.set_request_header("x-user-id", auth.user_id.unwrap_or_default());
                Ok(EdgeMinimalDecision::Continue(args.request))
            });

        let result = middleware
            .call(EdgeMinimalCallbackArgs {
                request: EdgeMinimalRequest::from_metadata(request()),
                context: context(),
                deps: deps(),
            })
            .await
            .expect("callback");

        let EdgeMinimalDecision::Continue(request) = result else {
            panic!("expected continue");
        };
        assert_eq!(request.header("x-user-id"), Some("user-1"));
    }
}
