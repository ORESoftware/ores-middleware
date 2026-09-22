#![forbid(unsafe_code)]
#![expect(
    clippy::too_many_arguments,
    reason = "SharedAuthVerifiedPrincipal keeps all provider, identity, tenant, session, issuer, audience, organization, and realm evidence explicit at construction"
)]

pub mod auth_provider;
pub mod auth_stage;
mod bootstrap;
mod compat;
pub mod composition;
mod config;
pub mod config_discovery;
mod context;
pub mod docs_serving;
pub mod fallthrough;
pub mod frameworks;
pub mod hardening;
pub mod host_abi;
pub mod hosts;
mod integrations;
pub mod lambda;
pub mod lambda_capabilities;
pub mod middleware_order;
mod net;
pub mod operation;
pub mod otel;
pub mod placement;
mod pipeline;
pub mod provider_api;
pub mod rate_limit;
pub mod rate_limit_bindings;
pub mod rate_limit_routes;
pub mod rate_limit_v2;
pub mod resilience;
pub mod runtime_manifest;
pub mod runtime_manifest_evidence;
pub mod security;
pub mod shared_auth;
pub mod shutdown;
pub mod stage;
pub mod validation;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

impl std::fmt::Debug for hardening::HardenedStagePipeline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HardenedStagePipeline")
            .finish_non_exhaustive()
    }
}

pub use auth_provider::{
    auth_provider_fn, dyn_auth_provider, shared_auth_provider_fn, FnAuthProvider,
    FnSharedAuthProvider, StaticAuthVerifier, StaticSharedAuthProviderVerifier,
};
pub use auth_stage::{AuthDecisionEnricher, AuthStage, NoopAuthDecisionEnricher};
pub use bootstrap::{config_from_env, stack_from_env, BootstrapError};
pub use composition::{
    validate_consumer_middleware_order, validate_declared_middleware_plan,
    validate_runtime_middleware_plan, MiddlewareCompositionPlan, MiddlewareOrderIssue,
    MiddlewareOrderPolicy, MiddlewareOrderingRule, OrderIssueSeverity,
};
pub use config::{
    default_config, validate_config, MiddlewareConfig, RateLimitPolicy, RuntimeEnvironment,
    ValidationIssue,
};
pub use config_discovery::{Discovered, DiscoveryError, Origin, Start};
pub use context::{
    capture_request_context, current_context, current_correlation_id,
    current_logged_in_user_id, current_request_id, current_session_id, current_tenant_id,
    current_trace_id, current_user_id, run_with_captured_context, run_with_context,
    spawn_with_current_context, ContextRegistry, RequestContext,
};
pub use host_abi::{
    middleware_config_sha256, MiddlewareHostAbiError, MiddlewareHostBeginResult,
    MiddlewareHostDescriptor, MiddlewareHostFinishRequest, MiddlewareHostFinishResult,
    MiddlewareHostKind, MiddlewareHostOutcome, MiddlewareHostRequest, MAX_HOST_HEADER_BYTES,
    MAX_HOST_HEADER_COUNT, MAX_HOST_METHOD_BYTES, MAX_HOST_PATH_BYTES,
    MIDDLEWARE_HOST_ABI_SCHEMA, MIDDLEWARE_HOST_ABI_VERSION,
};
pub use hosts::edge_minimal_local::{LocalEdgeMinimalHost, LocalEdgeMinimalResult};
pub use hosts::local::LocalMiddlewareHost;
pub use integrations::{
    AuthDecision, AuthVerifier, InMemoryTokenBucket, IntegrationError, RateLimiter,
    RequestMetadata, ResponseMetadata, SyncObserver, TelemetrySink, TransportSecurity,
};
pub use lambda::{
    LambdaInvocationBoundary, LambdaInvocationError, LambdaInvocationMetadata,
    LambdaInvocationTrigger,
};
pub use lambda_capabilities::{
    lambda_invocation_capabilities, LAMBDA_INVOCATION_CAPABILITIES,
};
pub use middleware_order::{
    rate_limit_posture, validate_middleware_order, MiddlewareStage, OperationClass, OrderViolation,
    RateLimitConsistency, RateLimitPosture, DEFAULT_MIDDLEWARE_ORDER,
};
pub use operation::{
    run_operation_boundary, run_operation_boundary_with_cancellation,
    run_operation_boundary_with_timeout, run_operation_boundary_with_timeout_and_cancellation,
    OperationDescriptor, OperationFailure, OperationFailureKind, OperationOutcome, OperationScope,
    OperationTransport,
};
pub use otel::{
    load_server_otel_runtime, load_server_otel_runtime_from_process_env,
    run_with_ores_log_context, server_otel_runtime_from_resolved, should_sample_trace,
    to_ores_log_context, RequestLogger, ServerOtelRuntime, ServerOtelRuntimeError,
};
pub use pipeline::{ActiveRequest, MiddlewareError, MiddlewareStack};
pub use placement::{
    MiddlewareCapabilities, MiddlewareExecutionProfile, MiddlewareExecutionTarget,
    MiddlewarePlacement, MiddlewarePlacementViolation,
};
pub use provider_api::{
    edge_fetch_middleware_fn, edge_minimal_middleware_fn, EdgeFetchCallbackArgs,
    EdgeFetchMiddleware, EdgeMinimalCallbackArgs, EdgeMinimalDecision, EdgeMinimalMiddleware,
    EdgeMinimalRequest, EdgeMiddlewareDependencies, EdgeNext, FnEdgeFetchMiddleware,
    FnEdgeMinimalMiddleware, MiddlewareCacheProvider, MiddlewareFetchProvider,
    MiddlewareFetchRequest, MiddlewareFetchResponse, MiddlewareResultFuture,
};
pub use rate_limit::{
    derive_rate_limit_principal, DynRateLimitKeyDeriver, HmacSha256KeyDeriver, RateLimitAlgorithm,
    RateLimitDecision, RateLimitDecisionKind, RateLimitDecisionSource, RateLimitFailureMode,
    RateLimitKeyDerivationMode, RateLimitKeyDeriver, RateLimitLayer, RateLimitPrincipal,
    RateLimitRequest, RateLimitSignal, UnavailableRateLimitKeyDeriver,
};
pub use rate_limit_bindings::{
    ResolvedRouteRateLimitBinding, RouteRateLimitBinding, RouteRateLimitBindingRequest,
    RouteRateLimitBindingResolutionError, RouteRateLimitBindingSelector,
    RouteRateLimitBindingSource, RouteRateLimitBindingTable, RouteRateLimitBindingViolation,
    MAX_ROUTE_METHODS, MAX_ROUTE_RATE_LIMIT_BINDINGS, MAX_ROUTE_RATE_LIMIT_REQUEST_PATH_LENGTH,
    ROUTE_RATE_LIMIT_BINDING_SCHEMA,
};
pub use rate_limit_routes::{
    RateLimitRouteSelector, ResolvedRouteRateLimitPolicy, RouteRateLimitPolicySource,
    RouteRateLimitRequest, RouteRateLimitResolutionError, RouteRateLimitRule, RouteRateLimitTable,
    RouteRateLimitViolation,
};
pub use rate_limit_v2::{
    RateLimitAlgorithmV2, RateLimitEnforcementMode, RateLimitPolicyDecodeError, RateLimitPolicyV2,
    RateLimitPolicyViolation,
};
pub use resilience::{
    Bulkhead, BulkheadRejected, CircuitAdmission, CircuitBreaker, CircuitBreakerConfig,
    CircuitStateSnapshot, ResilienceConfigError,
};
pub use runtime_manifest::{
    admit_server_stack, admit_server_stack_from, admit_server_stack_from_env, ManifestLoadError,
    RuntimeManifestError, MANIFEST_ENV_PREFIX, MANIFEST_FILE_NAME,
};
pub use runtime_manifest_evidence::{
    admit_server_stack_with_evidence_from, admit_server_stack_with_evidence_from_env,
    ManifestAdmissionEvidence, MANIFEST_ADMISSION_EVIDENCE_SCHEMA,
};
pub use security::{CorsPolicy, CorsStage, CsrfPolicy, CsrfStage};
pub use shared_auth::{
    SharedAuthDataPlane, SharedAuthDatabaseEnvKeys, SharedAuthDecisionMode, SharedAuthProvider,
    SharedAuthProviderContext, SharedAuthProviderFailure, SharedAuthProviderFailureKind,
    SharedAuthProviderTopology, SharedAuthProviderVerifier, SharedAuthReadyStack,
    SharedAuthRuntimeTopology, SharedAuthServerRole, SharedAuthVerifiedPrincipal,
    NEON_ADMIN_DATABASE_URL_ENV, NEON_AUTH_DATABASE_URL_ENV, SUPABASE_ADMIN_DATABASE_URL_ENV,
    SUPABASE_AUTH_DATABASE_URL_ENV,
};
pub use shutdown::{
    DrainGuard, DrainOutcome, ShutdownCoordinator, ShutdownPhase, ShutdownRejection,
    DEFAULT_DRAIN_TIMEOUT, DEFAULT_RETRY_AFTER, SHUTDOWN_HTTP_STATUS,
};
pub use stage::{
    MiddlewareStageHandler, StageDecision, StageInput, StagePipeline, StageRejection, StageResponse,
};
pub use validation::{ContractViolation, RequestContractValidator, ValidationStage};

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
        capabilities: CAPABILITIES.iter().map(|value| (*value).to_owned()).collect(),
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
