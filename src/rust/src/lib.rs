#![forbid(unsafe_code)]

mod bootstrap;
mod compat;
mod config;
mod context;
pub mod docs_serving;
pub mod frameworks;
mod integrations;
pub mod middleware_order;
mod net;
pub mod operation;
pub mod otel;
mod pipeline;
pub mod rate_limit;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use bootstrap::{config_from_env, stack_from_env, BootstrapError};
pub use config::{
    default_config, validate_config, MiddlewareConfig, RateLimitPolicy, RuntimeEnvironment,
    ValidationIssue,
};
pub use context::{
    current_context, current_logged_in_user_id, current_request_id, current_tenant_id,
    current_trace_id, current_user_id, run_with_context, ContextRegistry, RequestContext,
};
pub use integrations::{
    AuthDecision, AuthVerifier, InMemoryTokenBucket, IntegrationError, RateLimiter,
    RequestMetadata, ResponseMetadata, SyncObserver, TelemetrySink, TransportSecurity,
};
pub use middleware_order::{
    rate_limit_posture, validate_middleware_order, MiddlewareStage, OperationClass,
    OrderViolation, RateLimitConsistency, RateLimitPosture, DEFAULT_MIDDLEWARE_ORDER,
};
pub use operation::{
    run_operation_boundary, run_operation_boundary_with_cancellation,
    run_operation_boundary_with_timeout, OperationDescriptor, OperationFailure,
    OperationFailureKind, OperationOutcome, OperationScope, OperationTransport,
};
pub use otel::{run_with_ores_log_context, to_ores_log_context, RequestLogger};
pub use pipeline::{ActiveRequest, MiddlewareError, MiddlewareStack};
pub use rate_limit::{
    derive_rate_limit_principal, DynRateLimitKeyDeriver, HmacSha256KeyDeriver,
    RateLimitAlgorithm, RateLimitDecision, RateLimitDecisionKind, RateLimitDecisionSource,
    RateLimitFailureMode, RateLimitKeyDerivationMode, RateLimitKeyDeriver, RateLimitLayer,
    RateLimitPrincipal, RateLimitRequest, RateLimitSignal, UnavailableRateLimitKeyDeriver,
};

pub const CONTRACT_VERSION: &str = "1.0.0";
pub const CAPABILITIES: &[&str] = &[
    "request-context",
    "panic-recovery",
    "request-id",
    "trace-context",
    "structured-logging",
    "metrics-red",
    "deadline-timeout",
    "payload-limit",
    "rate-limit",
    "auth",
    "sync-observer",
    "json",
    "headers",
    "compression",
    "tls-policy",
    "security-headers",
    "idempotency",
    "ip-policy",
    "cache-etag",
    "content-negotiation",
    "fault-injection",
    "test-auth-bypass",
    "schema-capture",
];

pub fn capabilities() -> &'static [&'static str] {
    CAPABILITIES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterDescriptor {
    pub contract_version: String,
    pub language: String,
    pub runtime: String,
    pub package_name: String,
    pub framework_adapters: Vec<String>,
    pub capabilities: Vec<String>,
    pub operation_symbols: BTreeMap<String, String>,
}

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        contract_version: CONTRACT_VERSION.into(),
        language: "rust".into(),
        runtime: "tokio".into(),
        package_name: "ores-middleware".into(),
        framework_adapters: vec![
            "axum".into(),
            "mash".into(),
            "leptos".into(),
            "dioxus".into(),
        ],
        capabilities: CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        operation_symbols: BTreeMap::from([
            ("descriptor".into(), "descriptor".into()),
            ("defaultConfig".into(), "default_config".into()),
            ("validateConfig".into(), "validate_config".into()),
            ("createMiddleware".into(), "MiddlewareStack::new".into()),
            ("runWithContext".into(), "run_with_context".into()),
            ("currentContext".into(), "current_context".into()),
            ("capabilities".into(), "capabilities".into()),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_rejects_test_only_middleware() {
        let mut config = default_config("test-service");
        config.environment = RuntimeEnvironment::Production;
        config.settings.fault_injection.enabled = true;
        config.settings.test_auth_bypass.enabled = true;
        let issues = validate_config(&config);
        assert!(
            issues
                .iter()
                .any(|issue| issue.path.contains("faultInjection"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.path.contains("testAuthBypass"))
        );
    }

    #[tokio::test]
    async fn request_context_is_task_scoped() {
        let context = RequestContext {
            request_id: "r1".into(),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: Default::default(),
        };
        run_with_context(context, async {
            assert_eq!(current_context().unwrap().request_id, "r1");
            assert_eq!(current_request_id().as_deref(), Some("r1"));
        })
        .await;
        assert!(current_context().is_none());
        assert!(current_request_id().is_none());
    }

    #[test]
    fn descriptor_has_standard_operations() {
        let value = descriptor();
        assert_eq!(value.operation_symbols.len(), 7);
        assert_eq!(value.capabilities.len(), CAPABILITIES.len());
    }

    #[test]
    fn disabled_tls_cannot_claim_to_enforce_https() {
        let mut config = default_config("test-service");
        config.settings.tls.mode = "disabled".into();
        config.settings.tls.require_https = true;
        assert!(
            validate_config(&config)
                .iter()
                .any(|issue| issue.code == "disabled_tls_requires_false")
        );
    }
}
