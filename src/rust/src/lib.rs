use serde::{Deserialize, Serialize};

pub mod adapter;
pub mod audit;
pub mod auth;
pub mod cache;
pub mod compression;
pub mod config;
pub mod content_negotiation;
pub mod contracts;
pub mod cors;
pub mod csrf;
pub mod deadline;
pub mod error;
pub mod fault_injection;
pub mod frameworks;
pub mod hardening;
pub mod headers;
pub mod idempotency;
pub mod ip_policy;
pub mod json;
pub mod metrics;
pub mod middleware;
pub mod observability;
pub mod payload;
pub mod persistence;
pub mod rate_limit;
pub mod request_context;
pub mod resilience;
pub mod runtime_manifest;
pub mod security;
pub mod shared_auth;
pub mod shutdown;
pub mod stage;
pub mod validation;

pub use adapter::{
    Adapter, AdapterContext, AdapterError, AdapterMetadata, MiddlewareAdapter, MiddlewareAdapterResult,
};
pub use audit::{AuditEvent, AuditOutcome, AuditRecord, AuditSink, AuditStage};
pub use auth::{
    AuthContext, AuthDecision, AuthError, AuthProvider, AuthProviderKind, AuthStage, AuthenticatedUser,
    AuthorizationDecision, AuthorizationPolicy, AuthorizationStage,
};
pub use cache::{CacheControl, CachePolicy, CacheStage, EtagPolicy};
pub use compression::{CompressionAlgorithm, CompressionPolicy, CompressionStage};
pub use config::{MiddlewareConfig, MiddlewareConfigError, MiddlewareConfigLoader};
pub use content_negotiation::{ContentNegotiationPolicy, ContentNegotiationStage};
pub use contracts::{
    ContractDigest, ContractDigestError, ContractManifest, ContractManifestError, ContractVersion,
};
pub use cors::{CorsError, CorsOrigin, CorsOriginPattern};
pub use csrf::{CsrfDecision, CsrfError, CsrfRequestContext};
pub use deadline::{Deadline, DeadlineError, DeadlinePolicy, DeadlineStage};
pub use error::{MiddlewareError, MiddlewareErrorKind, MiddlewareResult};
pub use fault_injection::{FaultInjectionPolicy, FaultInjectionStage};
pub use hardening::{
    SanitizedProblem, StageHeaderPolicy, StageProblemPolicy, sanitized_problem_response,
};
pub use headers::{HeaderPolicy, HeaderPolicyError, HeaderStage};
pub use idempotency::{
    IdempotencyDecision, IdempotencyError, IdempotencyKey, IdempotencyPolicy, IdempotencyStage,
};
pub use ip_policy::{IpDecision, IpPolicy, IpPolicyError, IpStage};
pub use json::{JsonBodyPolicy, JsonStage};
pub use metrics::{MetricsRecorder, MetricsStage, RedMetric};
pub use middleware::{
    MiddlewareChain, MiddlewareContext, MiddlewareDecision, MiddlewareHandler, MiddlewareRequest,
    MiddlewareResponse,
};
pub use observability::{ObservabilityContext, ObservabilityStage};
pub use payload::{PayloadLimitPolicy, PayloadLimitStage};
pub use persistence::{
    PersistenceAdmission, PersistenceBackend, PersistenceError, PersistencePolicy, PersistenceStage,
};
pub use rate_limit::{
    RateLimitAdmission, RateLimitDecision, RateLimitError, RateLimitKey, RateLimitPolicy,
    RateLimitPolicyViolation,
};
pub use resilience::{
    Bulkhead, BulkheadRejected, CircuitAdmission, CircuitBreaker, CircuitBreakerConfig,
    CircuitStateSnapshot, ResilienceConfigError,
};
pub use runtime_manifest::{RuntimeManifestError, admit_server_stack};
pub use security::{CorsPolicy, CorsStage, CsrfPolicy, CsrfStage};
pub use shared_auth::{
    NEON_ADMIN_DATABASE_URL_ENV, NEON_AUTH_DATABASE_URL_ENV, SUPABASE_ADMIN_DATABASE_URL_ENV,
    SUPABASE_AUTH_DATABASE_URL_ENV, SharedAuthDataPlane, SharedAuthDatabaseEnvKeys,
    SharedAuthDecisionMode, SharedAuthProvider, SharedAuthProviderContext,
    SharedAuthProviderFailure, SharedAuthProviderFailureKind, SharedAuthProviderTopology,
    SharedAuthProviderVerifier, SharedAuthReadyStack, SharedAuthRuntimeTopology,
    SharedAuthServerRole, SharedAuthVerifiedPrincipal,
};
pub use shutdown::{
    DEFAULT_DRAIN_TIMEOUT, DEFAULT_RETRY_AFTER, DrainGuard, DrainOutcome, SHUTDOWN_HTTP_STATUS,
    ShutdownCoordinator, ShutdownPhase, ShutdownRejection,
};
pub use stage::{
    MiddlewareStageHandler, StageDecision, StageInput, StagePipeline, StageRejection, StageResponse,
};
pub use validation::{ContractViolation, RequestContractValidator, ValidationStage};

pub const CONTRACT_VERSION: &str = "1.0.0";
pub const CAPABILITIES: &[&str] = &[
    "request-context", "panic-recovery", "request-id", "trace-context",
    "structured-logging", "metrics-red", "deadline-timeout", "payload-limit",
    "rate-limit", "auth", "sync-observer", "json", "headers", "compression",
    "tls-policy", "security-headers", "idempotency", "ip-policy", "cache-etag",
    "content-negotiation", "fault-injection", "test-auth-bypass", "schema-capture",
    "graceful-shutdown",
];

pub fn capabilities() -> &'static [&'static str] { CAPABILITIES }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterDescriptor {
    pub contract_version: String,
    pub language: String,
    pub framework: String,
    pub capabilities: Vec<String>,
}

pub fn descriptor(language: impl Into<String>, framework: impl Into<String>) -> AdapterDescriptor {
    AdapterDescriptor {
        contract_version: CONTRACT_VERSION.to_string(),
        language: language.into(),
        framework: framework.into(),
        capabilities: capabilities().iter().map(|value| (*value).to_string()).collect(),
    }
}
