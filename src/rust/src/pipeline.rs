use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::{
    config::{IntegrationMode, MiddlewareConfig, ValidationIssue, validate_config},
    context::{ContextRegistry, RequestContext},
    integrations::{
        AnonymousAuth, DynAuthVerifier, DynRateLimiter, DynSyncObserver, DynTelemetrySink,
        InMemoryTokenBucket, IntegrationError, NoopSyncObserver, RequestMetadata, ResponseMetadata,
        TracingTelemetry,
    },
    net::cidr_contains,
    rate_limit::{
        DynRateLimitKeyDeriver, HmacSha256KeyDeriver, RateLimitDecision, RateLimitDecisionKind,
        RateLimitDecisionSource, RateLimitFailureMode, RateLimitKeyDerivationMode,
        RateLimitRequest, UnavailableRateLimitKeyDeriver, derive_rate_limit_principal,
    },
};

#[derive(Debug, Clone)]
pub struct MiddlewareError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
    pub headers: BTreeMap<String, String>,
}

impl MiddlewareError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            headers: BTreeMap::new(),
        }
    }

    fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = headers;
        self
    }
}

pub struct ActiveRequest {
    pub context: RequestContext,
    pub started: Instant,
    request: RequestMetadata,
}

#[derive(Clone)]
pub struct MiddlewareStack {
    config: Arc<MiddlewareConfig>,
    auth: DynAuthVerifier,
    sync: DynSyncObserver,
    telemetry: DynTelemetrySink,
    rate_limiter: DynRateLimiter,
    local_rate_limiter: DynRateLimiter,
    rate_limit_key_deriver: DynRateLimitKeyDeriver,
    registry: ContextRegistry,
}

impl MiddlewareStack {
    pub fn new(config: MiddlewareConfig) -> Result<Self, Vec<ValidationIssue>> {
        let issues = validate_config(&config);
        if !issues.is_empty() {
            return Err(issues);
        }
        let registry = ContextRegistry::new(
            config.settings.context_registry_max_entries,
            Duration::from_millis(config.settings.context_registry_ttl_ms),
        );
        let policy = &config.settings.rate_limit;
        let local_rate_limiter: DynRateLimiter = Arc::new(InMemoryTokenBucket::with_bounds(
            policy.local_cache_max_entries,
            Duration::from_millis(policy.local_cache_ttl_ms),
        ));
        let rate_limit_key_deriver: DynRateLimitKeyDeriver = match policy.key_derivation {
            RateLimitKeyDerivationMode::EphemeralHmacSha256 => {
                let mut secret = [0_u8; 32];
                let first = Uuid::new_v4();
                let second = Uuid::new_v4();
                secret[..16].copy_from_slice(first.as_bytes());
                secret[16..].copy_from_slice(second.as_bytes());
                Arc::new(HmacSha256KeyDeriver::from_key(secret))
            }
            RateLimitKeyDerivationMode::ExternalHmacSha256 => {
                Arc::new(UnavailableRateLimitKeyDeriver)
            }
        };
        Ok(Self {
            config: Arc::new(config),
            auth: Arc::new(AnonymousAuth),
            sync: Arc::new(NoopSyncObserver),
            telemetry: Arc::new(TracingTelemetry),
            rate_limiter: local_rate_limiter.clone(),
            local_rate_limiter,
            rate_limit_key_deriver,
            registry,
        })
    }

    pub fn with_auth_verifier(mut self, verifier: DynAuthVerifier) -> Self {
        self.auth = verifier;
        self
    }

    pub fn with_sync_observer(mut self, observer: DynSyncObserver) -> Self {
        self.sync = observer;
        self
    }

    pub fn with_telemetry(mut self, telemetry: DynTelemetrySink) -> Self {
        self.telemetry = telemetry;
        self
    }

    pub fn with_rate_limiter(mut self, limiter: DynRateLimiter) -> Self {
        self.rate_limiter = limiter;
        self
    }

    pub fn with_rate_limit_key_deriver(mut self, deriver: DynRateLimitKeyDeriver) -> Self {
        self.rate_limit_key_deriver = deriver;
        self
    }

    pub fn with_rate_limit_hmac_key(
        self,
        secret: impl AsRef<[u8]>,
    ) -> Result<Self, IntegrationError> {
        let deriver = HmacSha256KeyDeriver::new(secret)?;
        Ok(self.with_rate_limit_key_deriver(Arc::new(deriver)))
    }

    pub fn config(&self) -> &MiddlewareConfig {
        &self.config
    }

    pub async fn begin(&self, request: RequestMetadata) -> Result<ActiveRequest, MiddlewareError> {
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

        let request_id = request
            .headers
            .get(&self.config.settings.request_id_header)
            .filter(|value| valid_token(value))
            .cloned()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let trace_id = parse_trace_id(request.headers.get(&self.config.settings.trace_header))
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let now = RequestContext::now_ms();
        let mut context = RequestContext {
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

        let auth = self
            .auth
            .verify(&request)
            .await
            .map_err(|error| MiddlewareError::new(401, error.code, error.message))?;
        context.user_id = auth.user_id.clone();
        context.tenant_id = auth.tenant_id.clone();
        context.baggage.extend(
            auth.claims
                .iter()
                .filter(|(key, _)| key.starts_with("otel."))
                .map(|(key, value)| (key.clone(), value.clone())),
        );

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

        self.registry.insert(context.clone()).await;
        self.telemetry.request_started(&context, &request).await;
        Ok(ActiveRequest {
            context,
            started: Instant::now(),
            request,
        })
    }

    async fn evaluate_rate_limit(
        &self,
        context: &RequestContext,
        request: &RequestMetadata,
        auth: &crate::integrations::AuthDecision,
    ) -> RateLimitDecision {
        let policy = &self.config.settings.rate_limit;
        let principal = derive_rate_limit_principal(
            self.rate_limit_key_deriver.as_ref(),
            &policy.key_namespace,
            &policy.key_version,
            &policy.key_by,
            context,
            request,
            auth,
            effective_client_ip(&self.config, request).as_deref(),
        );

        let principal = match principal {
            Ok(principal) => principal,
            Err(error) => {
                return decision_for_failure(
                    policy,
                    matches!(policy.failure_mode, RateLimitFailureMode::FailOpen),
                    error.code,
                );
            }
        };

        let evaluation = RateLimitRequest {
            principal,
            policy_id: policy.policy_id.clone(),
            algorithm: policy.algorithm,
            layer: policy.layer,
            capacity: policy.capacity,
            refill_per_second: policy.refill_per_second,
            window_ms: policy.window_ms,
            cost: 1,
        };

        match self.rate_limiter.evaluate(&evaluation).await {
            Ok(decision) => decision,
            Err(error) => match (policy.layer, policy.failure_mode) {
                // Authorization is a security boundary. A primary outage must
                // not be weakened by either fail-open or a split local view.
                (crate::rate_limit::RateLimitLayer::Authorization, _) => {
                    decision_for_failure(policy, false, error.code)
                }
                (_, RateLimitFailureMode::FailOpen) => {
                    decision_for_failure(policy, true, error.code)
                }
                (_, RateLimitFailureMode::FailClosed) => {
                    decision_for_failure(policy, false, error.code)
                }
                (_, RateLimitFailureMode::LocalOnly) => {
                    match self.local_rate_limiter.evaluate(&evaluation).await {
                        Ok(mut decision) => {
                            if decision.is_allowed() {
                                decision.kind = RateLimitDecisionKind::DegradedAllowed;
                            }
                            decision.reason_code = Some(error.code.into());
                            decision
                        }
                        Err(local_error) => decision_for_failure(policy, false, local_error.code),
                    }
                }
            },
        }
    }

    pub async fn finish(
        &self,
        active: ActiveRequest,
        status: u16,
        response_bytes: Option<u64>,
    ) -> BTreeMap<String, String> {
        let response = ResponseMetadata {
            status,
            duration_ms: active.started.elapsed().as_millis() as u64,
            response_bytes,
        };
        self.telemetry
            .request_finished(&active.context, &active.request, &response)
            .await;
        let sync_result = self
            .sync
            .observe(&active.context, &active.request, &response)
            .await;
        if let Err(error) = sync_result {
            tracing::warn!(request_id = %active.context.request_id, code = error.code, message = %error.message, "opto-sync observation failed");
        }
        self.registry.remove(&active.context.request_id).await;

        let mut headers = BTreeMap::new();
        headers.insert(
            self.config.settings.request_id_header.clone(),
            active.context.request_id,
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

fn decision_for_failure(
    policy: &crate::config::RateLimitPolicy,
    allowed: bool,
    reason_code: &'static str,
) -> RateLimitDecision {
    RateLimitDecision {
        kind: if allowed {
            RateLimitDecisionKind::DegradedAllowed
        } else {
            RateLimitDecisionKind::DegradedDenied
        },
        source: RateLimitDecisionSource::Unknown,
        policy_id: policy.policy_id.clone(),
        layer: policy.layer,
        algorithm: policy.algorithm,
        limit: policy.capacity,
        remaining: 0,
        retry_after_ms: (!allowed).then_some(1_000),
        reset_after_ms: None,
        reason_code: Some(reason_code.into()),
    }
}

fn rate_limit_error(decision: &RateLimitDecision) -> MiddlewareError {
    let degraded = matches!(decision.kind, RateLimitDecisionKind::DegradedDenied);
    let status = if degraded { 503 } else { 429 };
    let code = if degraded {
        "rate_limit_unavailable"
    } else {
        "rate_limited"
    };
    let message = if degraded {
        "rate-limit enforcement is temporarily unavailable"
    } else {
        "rate limit exceeded"
    };

    let mut headers = BTreeMap::new();
    headers.insert("ratelimit-limit".into(), decision.limit.to_string());
    headers.insert("ratelimit-remaining".into(), decision.remaining.to_string());
    if let Some(reset_after_ms) = decision.reset_after_ms {
        headers.insert(
            "ratelimit-reset".into(),
            seconds_ceil(reset_after_ms).to_string(),
        );
    }
    let retry_after_ms = decision.retry_after_ms.unwrap_or(1_000);
    headers.insert(
        "retry-after".into(),
        seconds_ceil(retry_after_ms).to_string(),
    );
    headers.insert(
        "x-ores-rate-limit-policy".into(),
        decision.policy_id.clone(),
    );
    headers.insert(
        "x-ores-rate-limit-layer".into(),
        decision.layer.as_str().into(),
    );
    headers.insert(
        "x-ores-rate-limit-decision".into(),
        decision.kind.as_str().into(),
    );

    MiddlewareError::new(status, code, message).with_headers(headers)
}

const fn seconds_ceil(milliseconds: u64) -> u64 {
    milliseconds.saturating_add(999) / 1_000
}

fn enforce_transport_policy(
    config: &MiddlewareConfig,
    request: &RequestMetadata,
) -> Result<(), MiddlewareError> {
    let tls = &config.settings.tls;
    let forwarded = request
        .headers
        .get("x-forwarded-proto")
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let trusted_proxy = peer_is_trusted(request.remote_ip.as_deref(), &tls.trusted_proxy_cidrs);
    let has_forwarded_identity = request.headers.contains_key("x-forwarded-for")
        || request.headers.contains_key("cf-connecting-ip")
        || request.headers.contains_key("forwarded");

    if tls.mode == "trusted-proxy"
        && tls.strict_forwarded_headers
        && (forwarded.is_some() || has_forwarded_identity)
        && !trusted_proxy
    {
        return Err(MiddlewareError::new(
            400,
            "untrusted_forwarded_header",
            "forwarded transport or client headers came from an untrusted peer",
        ));
    }

    let secure = request.transport_secure
        || (tls.mode == "trusted-proxy" && trusted_proxy && forwarded.as_deref() == Some("https"));

    if tls.require_https && !secure {
        return Err(MiddlewareError::new(
            426,
            "https_required",
            "HTTPS is required",
        ));
    }

    Ok(())
}

fn effective_client_ip(config: &MiddlewareConfig, request: &RequestMetadata) -> Option<String> {
    let trusted_proxy = peer_is_trusted(
        request.remote_ip.as_deref(),
        &config.settings.tls.trusted_proxy_cidrs,
    );
    if trusted_proxy {
        let forwarded = request
            .headers
            .get("cf-connecting-ip")
            .map(String::as_str)
            .or_else(|| {
                request
                    .headers
                    .get("x-forwarded-for")
                    .and_then(|value| value.split(',').next())
            })
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(ip) = forwarded.and_then(|value| value.parse::<IpAddr>().ok()) {
            return Some(ip.to_string());
        }
    }
    request
        .remote_ip
        .as_deref()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .map(|ip| ip.to_string())
}

fn peer_is_trusted(remote_ip: Option<&str>, cidrs: &[String]) -> bool {
    let Some(ip) = remote_ip.and_then(|value| value.parse::<IpAddr>().ok()) else {
        return false;
    };
    cidrs.iter().any(|cidr| cidr_contains(cidr, ip))
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn parse_trace_id(header: Option<&String>) -> Option<String> {
    let value = header?;
    let mut parts = value.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?.to_ascii_lowercase();
    (trace_id.len() == 32
        && trace_id != "00000000000000000000000000000000"
        && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(trace_id)
}

#[cfg(test)]
mod tests {
    use std::{future::Future, pin::Pin};

    use super::*;
    use crate::{
        default_config,
        integrations::RateLimiter,
        rate_limit::{RateLimitAlgorithm, RateLimitLayer},
    };

    fn request(
        remote_ip: &str,
        forwarded_proto: Option<&str>,
        transport_secure: bool,
    ) -> RequestMetadata {
        let mut headers = BTreeMap::new();
        if let Some(value) = forwarded_proto {
            headers.insert("x-forwarded-proto".into(), value.into());
        }
        RequestMetadata {
            method: "GET".into(),
            path: "/health".into(),
            headers,
            remote_ip: Some(remote_ip.into()),
            content_length: None,
            transport_secure,
        }
    }

    #[test]
    fn rejects_forwarded_https_from_untrusted_peer() {
        let mut config = default_config("test-service");
        config.settings.tls.trusted_proxy_cidrs = vec!["10.0.0.0/8".into()];
        let error =
            enforce_transport_policy(&config, &request("198.51.100.10", Some("https"), false))
                .unwrap_err();
        assert_eq!(error.status, 400);
        assert_eq!(error.code, "untrusted_forwarded_header");
    }

    #[test]
    fn rejects_forwarded_client_ip_from_untrusted_peer() {
        let mut config = default_config("test-service");
        config.settings.tls.trusted_proxy_cidrs = vec!["10.0.0.0/8".into()];
        let mut request = request("198.51.100.10", None, true);
        request
            .headers
            .insert("x-forwarded-for".into(), "203.0.113.9".into());
        let error = enforce_transport_policy(&config, &request).unwrap_err();
        assert_eq!(error.code, "untrusted_forwarded_header");
    }

    #[test]
    fn uses_forwarded_client_ip_only_from_trusted_peer() {
        let mut config = default_config("test-service");
        config.settings.tls.trusted_proxy_cidrs = vec!["10.0.0.0/8".into()];
        let mut request = request("10.23.4.5", Some("https"), false);
        request
            .headers
            .insert("x-forwarded-for".into(), "203.0.113.9, 10.23.4.5".into());
        assert_eq!(
            effective_client_ip(&config, &request).as_deref(),
            Some("203.0.113.9")
        );
    }

    #[test]
    fn accepts_forwarded_https_only_from_trusted_peer() {
        let mut config = default_config("test-service");
        config.settings.tls.trusted_proxy_cidrs = vec!["10.0.0.0/8".into()];
        assert!(
            enforce_transport_policy(&config, &request("10.23.4.5", Some("https"), false),).is_ok()
        );
    }

    #[test]
    fn accepts_in_process_secure_transport_without_forwarded_headers() {
        let mut config = default_config("test-service");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        assert!(enforce_transport_policy(&config, &request("198.51.100.10", None, true),).is_ok());
    }

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
        ) -> Pin<Box<dyn Future<Output = Result<RateLimitDecision, IntegrationError>> + Send + 'a>>
        {
            Box::pin(async {
                Err(IntegrationError {
                    code: "redis_unavailable",
                    message: "backend unavailable".into(),
                })
            })
        }
    }

    #[tokio::test]
    async fn local_only_mode_falls_back_without_exposing_identity() {
        let mut config = default_config("test-service");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.capacity = 1;
        config.settings.rate_limit.refill_per_second = 0.000_001;
        config.settings.rate_limit.algorithm = RateLimitAlgorithm::TokenBucket;
        config.settings.rate_limit.layer = RateLimitLayer::Application;
        config.settings.rate_limit.failure_mode = RateLimitFailureMode::LocalOnly;
        let stack = MiddlewareStack::new(config)
            .unwrap()
            .with_rate_limiter(Arc::new(FailingLimiter));

        let first = stack
            .begin(request("203.0.113.9", None, true))
            .await
            .unwrap();
        stack.finish(first, 200, None).await;
        let error = match stack.begin(request("203.0.113.9", None, true)).await {
            Ok(_) => panic!("second request should be rate limited"),
            Err(error) => error,
        };

        assert_eq!(error.status, 429);
        assert_eq!(error.code, "rate_limited");
        assert!(error.headers.contains_key("retry-after"));
    }

    #[tokio::test]
    async fn authorization_layer_primary_failure_is_fail_closed() {
        let mut config = default_config("test-service");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.layer = RateLimitLayer::Authorization;
        config.settings.rate_limit.failure_mode = RateLimitFailureMode::LocalOnly;
        let stack = MiddlewareStack::new(config)
            .unwrap()
            .with_rate_limiter(Arc::new(FailingLimiter));

        let error = match stack.begin(request("203.0.113.9", None, true)).await {
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

    #[test]
    fn trace_id_parser_rejects_zero_and_non_hex_values() {
        let zero = "00-00000000000000000000000000000000-0123456789abcdef-01".to_string();
        let invalid = "00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-0123456789abcdef-01".to_string();
        let valid = "00-0123456789ABCDEF0123456789ABCDEF-0123456789abcdef-01".to_string();

        assert_eq!(parse_trace_id(Some(&zero)), None);
        assert_eq!(parse_trace_id(Some(&invalid)), None);
        assert_eq!(
            parse_trace_id(Some(&valid)).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
    }

    #[tokio::test]
    async fn finish_does_not_synthesize_response_traceparent_without_server_span() {
        let mut config = default_config("traceparent-policy-test");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.enabled = false;
        let stack = MiddlewareStack::new(config).unwrap();

        let active = stack
            .begin(request("198.51.100.10", None, true))
            .await
            .unwrap();
        let headers = stack.finish(active, 204, None).await;

        assert!(!headers.contains_key("traceparent"));
    }
}
