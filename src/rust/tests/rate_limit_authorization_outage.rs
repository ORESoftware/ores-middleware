use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use ores_middleware::{
    default_config, IntegrationError, MiddlewareStack, RateLimitDecision,
    RateLimitFailureMode, RateLimitLayer, RateLimitRequest, RateLimiter,
    RequestMetadata,
};

struct FailingLimiter;

impl RateLimiter for FailingLimiter {
    fn allow<'a>(
        &'a self,
        _key: &'a str,
        _capacity: u32,
        _refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async { false })
    }

    fn evaluate<'a>(
        &'a self,
        _request: &'a RateLimitRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<RateLimitDecision, IntegrationError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(IntegrationError {
                code: "redis_unavailable",
                message: "backend unavailable".into(),
            })
        })
    }
}

fn secure_request() -> RequestMetadata {
    RequestMetadata {
        method: "GET".into(),
        path: "/authorization-probe".into(),
        headers: BTreeMap::new(),
        remote_ip: Some("203.0.113.9".into()),
        content_length: None,
        transport_secure: true,
    }
}

fn authorization_config(failure_mode: RateLimitFailureMode) -> ores_middleware::MiddlewareConfig {
    let mut config = default_config("authorization-outage-test");
    config.settings.tls.mode = "in-process".into();
    config.settings.tls.trusted_proxy_cidrs.clear();
    config.settings.rate_limit.layer = RateLimitLayer::Authorization;
    config.settings.rate_limit.failure_mode = failure_mode;
    config
}

#[tokio::test]
async fn every_authorization_outage_mode_is_fail_closed() {
    // Fail-open is rejected at construction, before a request can reach the
    // evaluator. This is the preferred configuration-level safety boundary.
    assert!(MiddlewareStack::new(authorization_config(
        RateLimitFailureMode::FailOpen,
    ))
    .is_err());

    // Even the two configuration-valid outage modes must deny when the primary
    // backend is unavailable. In particular, local-only may not convert a
    // security-boundary outage into an allow.
    for failure_mode in [
        RateLimitFailureMode::FailClosed,
        RateLimitFailureMode::LocalOnly,
    ] {
        let stack = MiddlewareStack::new(authorization_config(failure_mode))
            .expect("authorization policy should be valid")
            .with_rate_limiter(Arc::new(FailingLimiter));

        let error = match stack.begin(secure_request()).await {
            Ok(_) => panic!("authorization-layer outage must fail closed"),
            Err(error) => error,
        };

        assert_eq!(error.status, 503);
        assert_eq!(error.code, "rate_limit_unavailable");
        assert_eq!(
            error
                .headers
                .get("x-ores-rate-limit-decision")
                .map(String::as_str),
            Some("degraded-denied")
        );
    }
}
