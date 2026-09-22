#![forbid(unsafe_code)]

mod adapters;
mod adapters_v2;
mod auth;
mod backend;
mod compression;
mod config;
mod config_discovery;
mod context;
mod host_abi;
mod hosts;
mod integrations;
mod lambda;
mod lambda_capabilities;
mod middleware_order;
mod operation;
mod otel;
mod pipeline;
mod placement;
mod provider_api;
mod rate_limit;
mod rate_limit_bindings;
mod rate_limit_routes;
mod rate_limit_v2;
mod redis_lru;
mod request_context;
mod response_headers;
mod route_policy;
mod runtime;
mod runtime_adapter;
mod runtime_capabilities;
mod serverless;
mod stage;
mod transport;

pub use adapters::{
    adapt_auth, adapt_rate_limiter, adapt_telemetry, AuthProvider, RateLimitProvider,
    TelemetryProvider,
};
pub use adapters_v2::{
    AuthAdapter, AuthFailure, AuthFailureKind, AuthInput, CacheAdapter, CacheDeleteInput,
    CacheGetInput, CacheGetResult, CachePutInput, HttpAdapter, HttpRequest, HttpResponse,
    RateLimitAdapter, RateLimitInput, RateLimitResult, RateLimitResultKind, TelemetryAdapter,
    TelemetryEvent,
};
pub use auth::{
    AuthDecision as MiddlewareAuthDecision, AuthError, AuthErrorKind, AuthPolicy, AuthRequest,
    AuthRequirement, AuthResult, AuthenticatedPrincipal, ClaimRequirement,
};
pub use backend::{
    BackendMiddleware, BackendMiddlewareContext, BackendMiddlewareError, BackendMiddlewareRequest,
    BackendMiddlewareResponse, BackendNext,
};
pub use compression::{
    choose_compression, CompressionChoice, CompressionConfig, CompressionEncoding,
    CompressionError, CompressionRequest, CompressionResponse,
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
pub use redis_lru::{
    CacheEntry, CacheError, CacheErrorKind, CachePolicy, CacheRequest, CacheResult,
    RedisLruCache,
};
pub use request_context::{
    build_request_context, RequestContextBuildError, RequestContextInput,
};
pub use response_headers::{
    apply_response_headers, ResponseHeaderPolicy, ResponseHeadersError,
};
pub use route_policy::{
    compile_route_policy, CompiledRoutePolicy, RoutePolicy, RoutePolicyError, RoutePolicyRule,
};
pub use runtime::{
    middleware_for_runtime, MiddlewareRuntime, MiddlewareRuntimeError, RuntimeMiddleware,
};
pub use runtime_adapter::{
    RuntimeAdapter, RuntimeAdapterError, RuntimeRequest, RuntimeResponse,
};
pub use runtime_capabilities::{
    runtime_capabilities, RuntimeCapabilities, RuntimeCapability,
};
pub use serverless::{
    ServerlessMiddleware, ServerlessMiddlewareError, ServerlessMiddlewareRequest,
    ServerlessMiddlewareResponse,
};
pub use stage::{MiddlewareStageOutcome, StageResponse};
pub use transport::{
    AdmissionDecision, AdmissionError, AdmissionErrorKind, AdmissionPolicy, TransportAdmission,
    TransportFacts,
};
