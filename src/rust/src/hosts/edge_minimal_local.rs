use std::sync::Arc;

use crate::{
    EdgeMinimalCallbackArgs, EdgeMinimalDecision, EdgeMinimalMiddleware,
    EdgeMiddlewareDependencies, IntegrationError, MiddlewareExecutionProfile,
    MiddlewareHostRequest, RequestContext, RequestMetadata, StageResponse,
    MIDDLEWARE_HOST_ABI_SCHEMA,
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
    /// The host validates both the inbound normalized request and any mutated
    /// request returned by authored middleware, so a callback cannot smuggle an
    /// uppercase/invalid header or framing byte into the P3 handoff.
    pub async fn execute(
        &self,
        request: MiddlewareHostRequest,
        context: RequestContext,
    ) -> Result<LocalEdgeMinimalResult, IntegrationError> {
        let request = request
            .into_request_metadata()
            .map_err(host_abi_error_as_integration)?;
        let result = self
            .middleware
            .call(EdgeMinimalCallbackArgs {
                request,
                context,
                deps: self.deps.clone(),
            })
            .await?;

        match result {
            EdgeMinimalDecision::Continue(request) => {
                let request = revalidate_request(request)?;
                Ok(LocalEdgeMinimalResult::Continue(request))
            }
            EdgeMinimalDecision::Respond(response) => {
                validate_short_circuit_response(&response)?;
                Ok(LocalEdgeMinimalResult::Respond(response))
            }
        }
    }
}

fn revalidate_request(request: RequestMetadata) -> Result<RequestMetadata, IntegrationError> {
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

    // P1 owns HTTP framing. P2 may author semantic response headers, but it must
    // not choose transport body framing or hop-by-hop connection semantics.
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
            Box<
                dyn Future<Output = Result<MiddlewareFetchResponse, IntegrationError>> + Send + 'a,
            >,
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
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
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
    async fn local_edge_minimal_host_injects_dependencies_and_returns_mutated_request() {
        let middleware = edge_minimal_middleware_fn(|mut args| async move {
            let fetched = args
                .deps
                .fetch
                .fetch(MiddlewareFetchRequest::get("https://example.invalid/health"))
                .await?;
            assert_eq!(fetched.status, 204);
            args.set_request_header("x-ores-edge", "admitted");
            Ok(EdgeMinimalDecision::Continue(args.request))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        assert_eq!(host.profile(), MiddlewareExecutionProfile::EdgeMinimal);

        let mut request = MiddlewareHostRequest::new("GET", "/v1/rpc");
        request.trusted_transport_secure = true;
        let result = host.execute(request, context()).await.expect("execute");
        let LocalEdgeMinimalResult::Continue(request) = result else {
            panic!("expected continue");
        };
        assert_eq!(request.headers.get("x-ores-edge").map(String::as_str), Some("admitted"));
    }

    #[tokio::test]
    async fn mutated_request_is_revalidated_before_handoff() {
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
    async fn informational_short_circuit_response_fails_closed() {
        let middleware = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(StageResponse {
                status: 103,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/hints");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("informational response is not terminal");
        assert_eq!(error.code, "invalid_edge_response_status");
    }

    #[tokio::test]
    async fn transport_framing_headers_in_short_circuit_response_fail_closed() {
        let middleware = edge_minimal_middleware_fn(|_args| async move {
            Ok(EdgeMinimalDecision::Respond(StageResponse {
                status: 200,
                headers: BTreeMap::from([("content-length".to_owned(), "999".to_owned())]),
                body: b"ok".to_vec(),
            }))
        });
        let host = LocalEdgeMinimalHost::from_middleware(middleware, deps());
        let mut request = MiddlewareHostRequest::new("GET", "/framing");
        request.trusted_transport_secure = true;
        let error = host
            .execute(request, context())
            .await
            .expect_err("P2 must not author transport response framing");
        assert_eq!(error.code, "edge_response_framing_header_forbidden");
    }
}
