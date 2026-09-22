use std::sync::Arc;

use crate::{
    EdgeMiddlewareDependencies, EdgeMinimalCallbackArgs, EdgeMinimalDecision,
    EdgeMinimalMiddleware, EdgeMinimalRequest, IntegrationError, MIDDLEWARE_HOST_ABI_SCHEMA,
    MiddlewareExecutionProfile, MiddlewareHostRequest, RequestContext, RequestMetadata,
    StageResponse,
};

/// Result of one request-side `edge_minimal` middleware invocation.
///
/// `Continue` carries the normalized request after middleware header mutations;
/// the local P1 host may then hand it to P3. `Respond` short-circuits before P3.
#[derive(Debug, Clone)]
pub enum LocalEdgeMinimalResult {
    Continue(RequestMetadata),
    Respond(StageResponse),
}

/// Immutable request fields owned by the ingress host rather than authored
/// `edge_minimal` middleware. The public callback request type already prevents
/// ordinary consumers from mutating these facts; this snapshot remains a
/// defense-in-depth check for crate-internal adapters and future refactors.
#[derive(Debug, Clone, Eq, PartialEq)]
struct EdgeMinimalTrustedFacts {
    method: String,
    path: String,
    remote_ip: Option<String>,
    content_length: Option<u64>,
    transport_secure: bool,
}

impl EdgeMinimalTrustedFacts {
    fn capture(request: &RequestMetadata) -> Self {
        Self {
            method: request.method.clone(),
            path: request.path.clone(),
            remote_ip: request.remote_ip.clone(),
            content_length: request.content_length,
            transport_secure: request.transport_secure,
        }
    }

    fn validate_unchanged(&self, request: &RequestMetadata) -> Result<(), IntegrationError> {
        if self.method != request.method
            || self.path != request.path
            || self.remote_ip != request.remote_ip
            || self.content_length != request.content_length
            || self.transport_secure != request.transport_secure
        {
            return Err(IntegrationError {
                code: "edge_request_trusted_fact_mutation",
                message: "edge_minimal middleware may mutate request headers but not method, path, content length, remote IP, or transport security facts".to_owned(),
            });
        }
        Ok(())
    }
}

/// Local-process host for the provider-neutral `edge_minimal` callback ABI.
///
/// This host deliberately has no response-finalization/session API. An
/// `edge_minimal` middleware unit only participates in request-side admission
/// and may hand the normalized request off after it returns `Continue`.
pub struct LocalEdgeMinimalHost<M> {
    middleware: Arc<M>,
    deps: EdgeMiddlewareDependencies,
}

impl<M> LocalEdgeMinimalHost<M>
where
    M: EdgeMinimalMiddleware,
{
    #[must_use]
    pub fn new(middleware: Arc<M>, deps: EdgeMiddlewareDependencies) -> Self {
        Self { middleware, deps }
    }

    #[must_use]
    pub fn from_middleware(middleware: M, deps: EdgeMiddlewareDependencies) -> Self {
        Self::new(Arc::new(middleware), deps)
    }

    #[must_use]
    pub const fn profile(&self) -> MiddlewareExecutionProfile {
        MiddlewareExecutionProfile::EdgeMinimal
    }

    /// Execute one request-side middleware callback with only the approved edge
    /// dependency bundle. Provider-specific SDK values never cross this API.
    ///
    /// The callback receives a constrained `EdgeMinimalRequest`: headers are the
    /// only writable request surface. The host then revalidates the normalized
    /// request and independently verifies all ingress-owned facts are unchanged.
    pub async fn execute(
        &self,
        request: MiddlewareHostRequest,
        context: RequestContext,
    ) -> Result<LocalEdgeMinimalResult, IntegrationError> {
        let request = request
            .into_request_metadata()
            .map_err(host_abi_error_as_integration)?;
        let trusted_facts = EdgeMinimalTrustedFacts::capture(&request);
        let result = self
            .middleware
            .call(EdgeMinimalCallbackArgs {
                request: EdgeMinimalRequest::from_metadata(request),
                context,
                deps: self.deps.clone(),
            })
            .await?;

        match result {
            EdgeMinimalDecision::Continue(request) => {
                let request = revalidate_request(request.into_metadata(), &trusted_facts)?;
                Ok(LocalEdgeMinimalResult::Continue(request))
            }
            EdgeMinimalDecision::Respond(response) => {
                validate_short_circuit_response(&response)?;
                Ok(LocalEdgeMinimalResult::Respond(response))
            }
        }
    }
}

fn revalidate_request(
    request: RequestMetadata,
    trusted_facts: &EdgeMinimalTrustedFacts,
) -> Result<RequestMetadata, IntegrationError> {
    trusted_facts.validate_unchanged(&request)?;
    MiddlewareHostRequest {
        schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
        method: request.method,
        path: request.path,
        headers: request.headers,
        trusted_remote_ip: request.remote_ip,
        content_length: request.content_length,
        trusted_transport_secure: request.transport_secure,
    }
    .into_request_metadata()
    .map_err(host_abi_error_as_integration)
}

fn validate_short_circuit_response(response: &StageResponse) -> Result<(), IntegrationError> {
    if !(200..=599).contains(&response.status) {
        return Err(IntegrationError {
            code: "invalid_edge_response_status",
            message: "edge_minimal short-circuit response must use a final HTTP status code"
                .to_owned(),
        });
    }

    // P2 owns application response metadata, not transport framing. P1/provider
    // adapters remain the sole authority for body framing and connection state.
    for name in response.headers.keys() {
        if matches!(
            name.as_str(),
            "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "proxy-connection"
                | "upgrade"
                | "te"
                | "trailer"
        ) {
            return Err(IntegrationError {
                code: "edge_response_framing_header_forbidden",
                message: format!(
                    "edge_minimal short-circuit response may not author transport framing header {name:?}"
                ),
            });
        }
    }

    // Reuse the host ABI's canonical header-name/value admission rather than
    // maintaining a second edge header validator.
    MiddlewareHostRequest {
        schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
        method: "GET".to_owned(),
        path: "/".to_owned(),
        headers: response.headers.clone(),
        trusted_remote_ip: None,
        content_length: None,
        trusted_transport_secure: true,
    }
    .validate()
    .map_err(host_abi_error_as_integration)
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
        AuthDecision, AuthVerifier, MiddlewareCacheProvider, MiddlewareFetchProvider,
        MiddlewareFetchRequest, MiddlewareFetchResponse, RateLimiter, TelemetrySink,
        edge_minimal_middleware_fn,
    };
    use std::{collections::BTreeMap, future::Future, pin::Pin};

    struct TestFetch;

    impl MiddlewareFetchProvider for TestFetch {
        fn fetch<'a>(
            &'a self,
            _request: MiddlewareFetchRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<MiddlewareFetchResponse, IntegrationError>> + Send + 'a>,
        > {
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
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>>
        {
            Box::pin(async { Ok(AuthDecision::default()) })
        }
    }

    struct TestRateLimiter;

    impl RateLimiter for TestRateLimiter {
        fn allow<'a>(
            &'a self,
            _key: &'a str,
            _capacity: u32,
            _refill_per_second: f64,
        ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
            Box::pin(async { true })
        }
    }

    struct TestCache;

    impl MiddlewareCacheProvider for TestCache {
        fn get<'a>(
            &'a self,
            _key: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, IntegrationError>> + Send + 'a>>
        {
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
            _response: &'a crate::ResponseMetadata,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }
    }

    fn deps() -> EdgeMiddlewareDependencies {
        EdgeMiddlewareDependencies::new(
            Arc::new(TestFetch),
            Arc::new(TestAuth),
            Arc::new(TestRateLimiter),
            Arc::new(TestCache),
            Arc::new(TestTelemetry),
        )
    }

    fn context() -> RequestContext {
        RequestContext {
            request_id: "req-edge".to_owned(),
            trace_id: "trace-edge".to_owned(),
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn local_edge_minimal_host_injects_dependencies_and_returns_mutated_headers() {
        let middleware = edge_minimal_middleware_fn(|mut args| async move {
            let fetched = args
                .deps
                .fetch
                .fetch(MiddlewareFetchRequest::get(
                    "https://example.invalid/health",
                ))
                .await?;
            assert_eq!(fetched.status, 204);
            assert_eq!(args.request.method(), "GET");
            assert_eq!(args.request.path(), "/v1/rpc");
            assert_eq!(args.request.remote_ip(), Some("127.0.0.1"));
            assert_eq!(args.request.content_length(), Some(42));
            assert!(args.request.transport_secure());
            args.set_request_header("x-ores-edge", "admitted");
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        assert_eq!(host.profile(), MiddlewareExecutionProfile::EdgeMinimal);

        let mut request = MiddlewareHostRequest::new("GET", "/v1/rpc");
        request.trusted_remote_ip = Some("127.0.0.1".to_owned());
        request.content_length = Some(42);
        request.trusted_transport_secure = true;
        let result = host.execute(request, context()).await.expect("execute");
        let LocalEdgeMinimalResult::Continue(request) = result else {
            panic!("expected continue");
        };
        assert_eq!(
            request.headers.get("x-ores-edge").map(String::as_str),
            Some("admitted")
        );
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/rpc");
        assert_eq!(request.remote_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(request.content_length, Some(42));
        assert!(request.transport_secure);
    }

    #[tokio::test]
    async fn mutated_request_headers_are_revalidated_before_handoff() {
        let middleware = edge_minimal_middleware_fn(|mut args| async move {
            args.set_request_header("Authorization", "not-canonical");
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("uppercase header must fail closed");
        assert_eq!(error.code, "invalid_header_name");
    }

    #[tokio::test]
    async fn internal_adapter_cannot_bypass_ingress_fact_recheck() {
        struct InternalMutation;

        impl EdgeMinimalMiddleware for InternalMutation {
            fn call<'a>(
                &'a self,
                args: EdgeMinimalCallbackArgs,
            ) -> crate::MiddlewareResultFuture<'a, EdgeMinimalDecision> {
                Box::pin(async move {
                    let mut request = args.request.into_metadata();
                    request.method = "POST".to_owned();
                    Ok(EdgeMinimalDecision::Continue(EdgeMinimalRequest::from_metadata(
                        request,
                    )))
                })
            }
        }

        let host = LocalEdgeMinimalHost::from_middleware(InternalMutation, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/original");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("host defense must reject crate-internal trusted-fact mutation");
        assert_eq!(error.code, "edge_request_trusted_fact_mutation");
    }

    #[tokio::test]
    async fn short_circuit_response_is_returned_without_a_downstream_continuation() {
        let middleware = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(StageResponse {
                status: 403,
                headers: BTreeMap::from([("cache-control".to_owned(), "no-store".to_owned())]),
                body: b"forbidden".to_vec(),
            }))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/private");
        request.trusted_transport_secure = true;
        let result = host.execute(request, context()).await.expect("execute");
        let LocalEdgeMinimalResult::Respond(response) = result else {
            panic!("expected response");
        };
        assert_eq!(response.status, 403);
        assert_eq!(response.body, b"forbidden");
    }

    #[tokio::test]
    async fn informational_and_framing_control_short_circuits_fail_closed() {
        let informational = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(StageResponse {
                status: 103,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }))
        });
        let host = LocalEdgeMinimalHost::from_middleware(informational, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/hints");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("informational response is not terminal");
        assert_eq!(error.code, "invalid_edge_response_status");

        let framing = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(StageResponse {
                status: 200,
                headers: BTreeMap::from([("content-length".to_owned(), "999".to_owned())]),
                body: b"ok".to_vec(),
            }))
        });
        let host = LocalEdgeMinimalHost::from_middleware(framing, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("P2 must not author transport body framing");
        assert_eq!(error.code, "edge_response_framing_header_forbidden");
    }
}
