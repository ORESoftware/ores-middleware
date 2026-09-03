use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};

use uuid::Uuid;

use crate::{
    config::{validate_config, IntegrationMode, MiddlewareConfig, ValidationIssue},
    context::{ContextRegistry, RequestContext},
    integrations::{AnonymousAuth, DynAuthVerifier, DynRateLimiter, DynSyncObserver, DynTelemetrySink, InMemoryTokenBucket, NoopSyncObserver, RequestMetadata, ResponseMetadata, TracingTelemetry},
};

#[derive(Debug, Clone)]
pub struct MiddlewareError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
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
    registry: ContextRegistry,
}

impl MiddlewareStack {
    pub fn new(config: MiddlewareConfig) -> Result<Self, Vec<ValidationIssue>> {
        let issues = validate_config(&config);
        if !issues.is_empty() { return Err(issues); }
        let registry = ContextRegistry::new(config.settings.context_registry_max_entries, Duration::from_millis(config.settings.context_registry_ttl_ms));
        Ok(Self {
            config: Arc::new(config),
            auth: Arc::new(AnonymousAuth),
            sync: Arc::new(NoopSyncObserver),
            telemetry: Arc::new(TracingTelemetry),
            rate_limiter: Arc::new(InMemoryTokenBucket::default()),
            registry,
        })
    }

    pub fn with_auth_verifier(mut self, verifier: DynAuthVerifier) -> Self { self.auth = verifier; self }
    pub fn with_sync_observer(mut self, observer: DynSyncObserver) -> Self { self.sync = observer; self }
    pub fn with_telemetry(mut self, telemetry: DynTelemetrySink) -> Self { self.telemetry = telemetry; self }
    pub fn with_rate_limiter(mut self, limiter: DynRateLimiter) -> Self { self.rate_limiter = limiter; self }
    pub fn config(&self) -> &MiddlewareConfig { &self.config }

    pub async fn begin(&self, request: RequestMetadata) -> Result<ActiveRequest, MiddlewareError> {
        if request.content_length.is_some_and(|length| length > self.config.settings.max_body_bytes as u64) {
            return Err(MiddlewareError { status: 413, code: "payload_too_large", message: "request body exceeds configured limit".into() });
        }

        if self.config.settings.tls.require_https {
            let forwarded = request.headers.get("x-forwarded-proto").map(String::as_str);
            let direct = request.headers.get("x-ores-scheme").map(String::as_str);
            if forwarded != Some("https") && direct != Some("https") {
                return Err(MiddlewareError { status: 426, code: "https_required", message: "HTTPS is required".into() });
            }
        }

        let request_id = request.headers.get(&self.config.settings.request_id_header).filter(|value| valid_token(value)).cloned().unwrap_or_else(|| Uuid::new_v4().to_string());
        let trace_id = parse_trace_id(request.headers.get(&self.config.settings.trace_header)).unwrap_or_else(|| Uuid::new_v4().simple().to_string());
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

        let auth = self.auth.verify(&request).await.map_err(|error| MiddlewareError { status: 401, code: error.code, message: error.message })?;
        context.user_id = auth.user_id;
        context.tenant_id = auth.tenant_id;
        context.baggage.extend(auth.claims.into_iter().filter(|(key, _)| key.starts_with("otel.")));

        if !matches!(self.config.integrations.shared_auth.mode, IntegrationMode::Disabled) && context.user_id.is_none() {
            return Err(MiddlewareError { status: 401, code: "authentication_required", message: "shared-auth did not establish a user".into() });
        }

        if self.config.settings.rate_limit.enabled {
            let key = rate_limit_key(&context, &request);
            let policy = &self.config.settings.rate_limit;
            if !self.rate_limiter.allow(&key, policy.capacity, policy.refill_per_second).await {
                return Err(MiddlewareError { status: 429, code: "rate_limited", message: "rate limit exceeded".into() });
            }
        }

        if self.config.settings.fault_injection.enabled && self.config.settings.fault_injection.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.config.settings.fault_injection.latency_ms)).await;
        }

        self.registry.insert(context.clone()).await;
        self.telemetry.request_started(&context, &request).await;
        Ok(ActiveRequest { context, started: Instant::now(), request })
    }

    pub async fn finish(&self, active: ActiveRequest, status: u16, response_bytes: Option<u64>) -> BTreeMap<String, String> {
        let response = ResponseMetadata { status, duration_ms: active.started.elapsed().as_millis() as u64, response_bytes };
        self.telemetry.request_finished(&active.context, &active.request, &response).await;
        let sync_result = self.sync.observe(&active.context, &active.request, &response).await;
        if let Err(error) = sync_result {
            tracing::warn!(request_id = %active.context.request_id, code = error.code, message = %error.message, "opto-sync observation failed");
        }
        self.registry.remove(&active.context.request_id).await;

        let mut headers = BTreeMap::new();
        headers.insert(self.config.settings.request_id_header.clone(), active.context.request_id);
        headers.insert("traceparent".into(), format!("00-{}-0000000000000000-01", active.context.trace_id));
        if self.config.settings.security_headers.enabled {
            headers.insert("x-content-type-options".into(), "nosniff".into());
            headers.insert("x-frame-options".into(), self.config.settings.security_headers.frame_options.clone());
            headers.insert("referrer-policy".into(), "strict-origin-when-cross-origin".into());
            headers.insert("strict-transport-security".into(), format!("max-age={}; includeSubDomains", self.config.settings.security_headers.hsts_max_age_seconds));
            if let Some(csp) = &self.config.settings.security_headers.content_security_policy {
                headers.insert("content-security-policy".into(), csp.clone());
            }
        }
        headers
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn parse_trace_id(header: Option<&String>) -> Option<String> {
    let value = header?;
    let mut parts = value.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?;
    (trace_id.len() == 32 && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| trace_id.to_ascii_lowercase())
}

fn rate_limit_key(context: &RequestContext, request: &RequestMetadata) -> String {
    format!("{}:{}:{}:{}", context.tenant_id.as_deref().unwrap_or("_"), context.user_id.as_deref().unwrap_or("_"), request.remote_ip.as_deref().unwrap_or("_"), request.path)
}
