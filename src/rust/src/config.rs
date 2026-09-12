use serde::{Deserialize, Serialize};

use crate::{
    CAPABILITIES, CONTRACT_VERSION,
    net::valid_cidr,
    rate_limit::{
        RateLimitAlgorithm, RateLimitFailureMode, RateLimitKeyDerivationMode, RateLimitLayer,
        RateLimitSignal,
    },
};

const MAX_LOCAL_RATE_LIMIT_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeEnvironment {
    Development,
    Test,
    Staging,
    Production,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationMode {
    Disabled,
    Http,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPolicy {
    pub enabled: bool,
    pub capacity: u32,
    pub refill_per_second: f64,
    pub key_by: Vec<RateLimitSignal>,
    #[serde(default = "default_rate_limit_policy_id")]
    pub policy_id: String,
    #[serde(default)]
    pub algorithm: RateLimitAlgorithm,
    #[serde(default)]
    pub layer: RateLimitLayer,
    #[serde(default)]
    pub failure_mode: RateLimitFailureMode,
    #[serde(default = "default_rate_limit_window_ms")]
    pub window_ms: u64,
    #[serde(default = "default_rate_limit_local_cache_max_entries")]
    pub local_cache_max_entries: usize,
    #[serde(default = "default_rate_limit_local_cache_ttl_ms")]
    pub local_cache_ttl_ms: u64,
    #[serde(default = "default_rate_limit_key_namespace")]
    pub key_namespace: String,
    #[serde(default = "default_rate_limit_key_version")]
    pub key_version: String,
    #[serde(default)]
    pub key_derivation: RateLimitKeyDerivationMode,
}

fn default_rate_limit_policy_id() -> String {
    "default".into()
}

const fn default_rate_limit_window_ms() -> u64 {
    1_000
}

const fn default_rate_limit_local_cache_max_entries() -> usize {
    MAX_LOCAL_RATE_LIMIT_ENTRIES
}

const fn default_rate_limit_local_cache_ttl_ms() -> u64 {
    30_000
}

fn default_rate_limit_key_namespace() -> String {
    "ores-middleware".into()
}

fn default_rate_limit_key_version() -> String {
    "v1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionPolicy {
    pub enabled: bool,
    pub minimum_bytes: usize,
    pub algorithms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TlsPolicy {
    pub mode: String,
    pub require_https: bool,
    pub strict_forwarded_headers: bool,
    pub trusted_proxy_cidrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityHeaderPolicy {
    pub enabled: bool,
    pub hsts_max_age_seconds: u64,
    pub content_security_policy: Option<String>,
    pub frame_options: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdempotencyPolicy {
    pub enabled: bool,
    pub header_name: String,
    pub ttl_seconds: u64,
    pub required_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaultInjectionPolicy {
    pub enabled: bool,
    pub latency_ms: u64,
    pub error_rate: f64,
    pub drop_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAuthBypassPolicy {
    pub enabled: bool,
    pub header_name: String,
    pub allowed_cidrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareSettings {
    pub request_id_header: String,
    pub trace_header: String,
    pub timeout_ms: u64,
    pub max_body_bytes: usize,
    pub context_registry_max_entries: usize,
    pub context_registry_ttl_ms: u64,
    pub rate_limit: RateLimitPolicy,
    pub compression: CompressionPolicy,
    pub tls: TlsPolicy,
    pub security_headers: SecurityHeaderPolicy,
    pub idempotency: IdempotencyPolicy,
    pub fault_injection: FaultInjectionPolicy,
    pub test_auth_bypass: TestAuthBypassPolicy,
    pub content_representations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedAuthIntegration {
    pub mode: IntegrationMode,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub jwks_uri: Option<String>,
    pub introspection_url: Option<String>,
    pub fail_open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptoSyncIntegration {
    pub mode: IntegrationMode,
    pub endpoint: Option<String>,
    pub outbox_topic: Option<String>,
    pub fail_open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OresOtelIntegration {
    pub enabled: bool,
    pub service_name: String,
    pub exporter_endpoint: Option<String>,
    pub propagators: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareIntegrations {
    pub shared_auth: SharedAuthIntegration,
    pub opto_sync: OptoSyncIntegration,
    pub ores_otel: OresOtelIntegration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareConfig {
    pub contract_version: String,
    pub environment: RuntimeEnvironment,
    pub required_capabilities: Vec<String>,
    pub settings: MiddlewareSettings,
    pub integrations: MiddlewareIntegrations,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

impl ValidationIssue {
    pub(crate) fn new(
        path: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// A single validation rule: a pure function from the config to at most one issue.
///
/// Rules are composed by [`validate_config`]; none of them touches shared state, so
/// each rule can be read, tested and reordered on its own.
type Rule = fn(&MiddlewareConfig) -> Option<ValidationIssue>;

/// Build an issue when `failed` holds. The message is built lazily so rules pay for
/// formatting only on the failure path.
fn issue_if(
    failed: bool,
    path: impl Into<String>,
    code: &'static str,
    message: impl FnOnce() -> String,
) -> Option<ValidationIssue> {
    failed.then(|| ValidationIssue::new(path, code, message()))
}

const CONFIG_RULES: &[Rule] = &[
    |config| {
        issue_if(
            config.contract_version != CONTRACT_VERSION,
            "/contractVersion",
            "unsupported_version",
            || format!("expected {CONTRACT_VERSION}"),
        )
    },
    |config| {
        issue_if(
            config.settings.timeout_ms == 0,
            "/settings/timeoutMs",
            "range",
            || "timeout must be positive".into(),
        )
    },
    |config| {
        issue_if(
            config.settings.max_body_bytes == 0,
            "/settings/maxBodyBytes",
            "range",
            || "body limit must be positive".into(),
        )
    },
];

const FAULT_AND_AUTH_RULES: &[Rule] = &[
    |config| {
        let fault = &config.settings.fault_injection;
        issue_if(
            !(0.0..=1.0).contains(&fault.error_rate) || !(0.0..=1.0).contains(&fault.drop_rate),
            "/settings/faultInjection",
            "range",
            || "fault rates must be within 0..=1".into(),
        )
    },
    |config| {
        issue_if(
            config.is_production() && config.settings.fault_injection.enabled,
            "/settings/faultInjection/enabled",
            "production_forbidden",
            || "fault injection is forbidden in production".into(),
        )
    },
    |config| {
        issue_if(
            config.is_production() && config.settings.test_auth_bypass.enabled,
            "/settings/testAuthBypass/enabled",
            "production_forbidden",
            || "test auth bypass is forbidden in production".into(),
        )
    },
    |config| {
        issue_if(
            config.integrations.shared_auth.fail_open,
            "/integrations/sharedAuth/failOpen",
            "auth_fail_open",
            || "shared-auth must fail closed".into(),
        )
    },
];

fn tls_mode_issue(config: &MiddlewareConfig) -> Option<ValidationIssue> {
    let tls = &config.settings.tls;
    match tls.mode.as_str() {
        "disabled" => issue_if(
            tls.require_https,
            "/settings/tls/requireHttps",
            "disabled_tls_requires_false",
            || "TLS mode disabled cannot enforce HTTPS".into(),
        ),
        "in-process" => None,
        "trusted-proxy" => issue_if(
            tls.trusted_proxy_cidrs.is_empty(),
            "/settings/tls/trustedProxyCidrs",
            "trusted_proxy_required",
            || "trusted-proxy mode requires an explicit CIDR allowlist".into(),
        ),
        _ => Some(ValidationIssue::new(
            "/settings/tls/mode",
            "unknown_tls_mode",
            "TLS mode must be disabled, in-process, or trusted-proxy",
        )),
    }
}

fn invalid_cidr_issues(config: &MiddlewareConfig) -> impl Iterator<Item = ValidationIssue> + '_ {
    config
        .settings
        .tls
        .trusted_proxy_cidrs
        .iter()
        .enumerate()
        .filter(|(_, cidr)| !valid_cidr(cidr))
        .map(|(index, cidr)| {
            ValidationIssue::new(
                format!("/settings/tls/trustedProxyCidrs/{index}"),
                "invalid_cidr",
                cidr.clone(),
            )
        })
}

fn unknown_capability_issues(
    config: &MiddlewareConfig,
) -> impl Iterator<Item = ValidationIssue> + '_ {
    config
        .required_capabilities
        .iter()
        .filter(|capability| !CAPABILITIES.contains(&capability.as_str()))
        .map(|capability| {
            ValidationIssue::new(
                "/requiredCapabilities",
                "unknown_capability",
                capability.clone(),
            )
        })
}

impl MiddlewareConfig {
    #[must_use]
    pub fn is_production(&self) -> bool {
        matches!(self.environment, RuntimeEnvironment::Production)
    }
}

/// Validate a config by composing independent pure rules.
///
/// The output order is the composition order, which the tests rely on; every rule
/// is a value-in/value-out function so no accumulator is threaded through the code.
pub fn validate_config(config: &MiddlewareConfig) -> Vec<ValidationIssue> {
    CONFIG_RULES
        .iter()
        .filter_map(|rule| rule(config))
        .chain(validate_rate_limit_policy(config))
        .chain(FAULT_AND_AUTH_RULES.iter().filter_map(|rule| rule(config)))
        .chain(tls_mode_issue(config))
        .chain(invalid_cidr_issues(config))
        .chain(unknown_capability_issues(config))
        .collect()
}

const RATE_LIMIT_RULES: &[Rule] = &[
    |config| {
        let policy = &config.settings.rate_limit;
        issue_if(
            policy.capacity == 0
                || !policy.refill_per_second.is_finite()
                || policy.refill_per_second <= 0.0,
            "/settings/rateLimit",
            "invalid_rate_limit",
            || "enabled rate limits require positive finite capacity and refill".into(),
        )
    },
    |config| {
        issue_if(
            config.settings.rate_limit.policy_id.trim().is_empty(),
            "/settings/rateLimit/policyId",
            "required",
            || "rate-limit policy IDs must not be empty".into(),
        )
    },
    |config| {
        issue_if(
            config.settings.rate_limit.key_by.is_empty(),
            "/settings/rateLimit/keyBy",
            "required",
            || "at least one rate-limit signal is required".into(),
        )
    },
    |config| {
        issue_if(
            !config
                .settings
                .rate_limit
                .key_by
                .iter()
                .copied()
                .any(RateLimitSignal::is_principal_signal),
            "/settings/rateLimit/keyBy",
            "principal_required",
            || "route and method alone cannot identify a rate-limit principal".into(),
        )
    },
    |config| {
        let policy = &config.settings.rate_limit;
        issue_if(
            matches!(policy.layer, RateLimitLayer::CloudflareEdge)
                && policy
                    .key_by
                    .iter()
                    .copied()
                    .any(|signal| !signal.is_edge_safe()),
            "/settings/rateLimit/keyBy",
            "edge_identity_forbidden",
            || "Cloudflare edge policies may use only IP, IP prefix, route, and method signals"
                .into(),
        )
    },
    |config| {
        let policy = &config.settings.rate_limit;
        issue_if(
            matches!(policy.layer, RateLimitLayer::Authorization)
                && matches!(policy.failure_mode, RateLimitFailureMode::FailOpen),
            "/settings/rateLimit/failureMode",
            "authorization_fail_open_forbidden",
            || "authorization-layer rate limiting must not fail open".into(),
        )
    },
    |config| {
        issue_if(
            config.settings.rate_limit.window_ms == 0,
            "/settings/rateLimit/windowMs",
            "range",
            || "rate-limit windows must be positive".into(),
        )
    },
    |config| {
        let entries = config.settings.rate_limit.local_cache_max_entries;
        issue_if(
            entries == 0 || entries > MAX_LOCAL_RATE_LIMIT_ENTRIES,
            "/settings/rateLimit/localCacheMaxEntries",
            "range",
            || {
                format!(
                    "local rate-limit caches must contain between 1 and {MAX_LOCAL_RATE_LIMIT_ENTRIES} entries"
                )
            },
        )
    },
    |config| {
        let policy = &config.settings.rate_limit;
        issue_if(
            policy.local_cache_ttl_ms < policy.window_ms,
            "/settings/rateLimit/localCacheTtlMs",
            "ttl_shorter_than_window",
            || "local cache TTL must be at least one policy window".into(),
        )
    },
    |config| {
        let policy = &config.settings.rate_limit;
        issue_if(
            policy.key_namespace.trim().is_empty() || policy.key_version.trim().is_empty(),
            "/settings/rateLimit",
            "key_domain_required",
            || "rate-limit key namespace and version must not be empty".into(),
        )
    },
    |config| {
        issue_if(
            config.is_production()
                && matches!(
                    config.settings.rate_limit.key_derivation,
                    RateLimitKeyDerivationMode::EphemeralHmacSha256
                ),
            "/settings/rateLimit/keyDerivation",
            "production_requires_external_hmac",
            || "production rate limiting requires a stable external HMAC key".into(),
        )
    },
];

fn validate_rate_limit_policy(config: &MiddlewareConfig) -> Vec<ValidationIssue> {
    if !config.settings.rate_limit.enabled {
        return Vec::new();
    }
    RATE_LIMIT_RULES
        .iter()
        .filter_map(|rule| rule(config))
        .collect()
}

pub fn default_config(service_name: impl Into<String>) -> MiddlewareConfig {
    let service_name = service_name.into();
    MiddlewareConfig {
        contract_version: CONTRACT_VERSION.to_owned(),
        environment: RuntimeEnvironment::Development,
        required_capabilities: CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        settings: MiddlewareSettings {
            request_id_header: "x-request-id".into(),
            trace_header: "traceparent".into(),
            timeout_ms: 5_000,
            max_body_bytes: 2 * 1024 * 1024,
            context_registry_max_entries: 10_000,
            context_registry_ttl_ms: 30_000,
            rate_limit: RateLimitPolicy {
                enabled: true,
                capacity: 100,
                refill_per_second: 20.0,
                key_by: vec![
                    RateLimitSignal::Tenant,
                    RateLimitSignal::User,
                    RateLimitSignal::Ip,
                    RateLimitSignal::Route,
                ],
                policy_id: "application-default".into(),
                algorithm: RateLimitAlgorithm::TokenBucket,
                layer: RateLimitLayer::Application,
                failure_mode: RateLimitFailureMode::LocalOnly,
                window_ms: 1_000,
                local_cache_max_entries: MAX_LOCAL_RATE_LIMIT_ENTRIES,
                local_cache_ttl_ms: 30_000,
                key_namespace: service_name.clone(),
                key_version: "v1".into(),
                key_derivation: RateLimitKeyDerivationMode::EphemeralHmacSha256,
            },
            compression: CompressionPolicy {
                enabled: true,
                minimum_bytes: 1_024,
                algorithms: vec!["br".into(), "gzip".into()],
            },
            tls: TlsPolicy {
                mode: "trusted-proxy".into(),
                require_https: true,
                strict_forwarded_headers: true,
                trusted_proxy_cidrs: vec!["127.0.0.1/32".into(), "::1/128".into()],
            },
            security_headers: SecurityHeaderPolicy {
                enabled: true,
                hsts_max_age_seconds: 31_536_000,
                content_security_policy: Some("default-src 'self'; frame-ancestors 'none'".into()),
                frame_options: "DENY".into(),
            },
            idempotency: IdempotencyPolicy {
                enabled: true,
                header_name: "idempotency-key".into(),
                ttl_seconds: 86_400,
                required_methods: vec!["POST".into(), "PUT".into(), "PATCH".into()],
            },
            fault_injection: FaultInjectionPolicy {
                enabled: false,
                latency_ms: 0,
                error_rate: 0.0,
                drop_rate: 0.0,
            },
            test_auth_bypass: TestAuthBypassPolicy {
                enabled: false,
                header_name: "x-test-auth-bypass".into(),
                allowed_cidrs: vec!["127.0.0.1/32".into(), "::1/128".into()],
            },
            content_representations: vec![
                "application/json".into(),
                "application/problem+json".into(),
            ],
        },
        integrations: MiddlewareIntegrations {
            shared_auth: SharedAuthIntegration {
                mode: IntegrationMode::Disabled,
                issuer: None,
                audience: None,
                jwks_uri: None,
                introspection_url: None,
                fail_open: false,
            },
            opto_sync: OptoSyncIntegration {
                mode: IntegrationMode::Disabled,
                endpoint: None,
                outbox_topic: None,
                fail_open: true,
            },
            ores_otel: OresOtelIntegration {
                enabled: true,
                service_name,
                exporter_endpoint: None,
                propagators: vec!["tracecontext".into(), "baggage".into()],
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_requires_stable_hmac_key_derivation() {
        let mut config = default_config("test-service");
        config.environment = RuntimeEnvironment::Production;
        assert!(
            validate_config(&config)
                .iter()
                .any(|issue| issue.code == "production_requires_external_hmac")
        );

        config.settings.rate_limit.key_derivation = RateLimitKeyDerivationMode::ExternalHmacSha256;
        assert!(
            !validate_config(&config)
                .iter()
                .any(|issue| issue.code == "production_requires_external_hmac")
        );
    }

    #[test]
    fn edge_policy_rejects_authenticated_identity_signals() {
        let mut config = default_config("test-service");
        config.settings.rate_limit.layer = RateLimitLayer::CloudflareEdge;
        config.settings.rate_limit.key_by = vec![
            RateLimitSignal::Ip,
            RateLimitSignal::User,
            RateLimitSignal::Route,
        ];
        assert!(
            validate_config(&config)
                .iter()
                .any(|issue| issue.code == "edge_identity_forbidden")
        );
    }

    #[test]
    fn local_cache_is_bounded_to_ten_thousand_entries() {
        let mut config = default_config("test-service");
        config.settings.rate_limit.local_cache_max_entries = 10_001;
        assert!(
            validate_config(&config)
                .iter()
                .any(|issue| issue.path.ends_with("localCacheMaxEntries"))
        );
    }

    #[test]
    fn authorization_layer_cannot_fail_open() {
        let mut config = default_config("test-service");
        config.settings.rate_limit.layer = RateLimitLayer::Authorization;
        config.settings.rate_limit.failure_mode = RateLimitFailureMode::FailOpen;
        assert!(
            validate_config(&config)
                .iter()
                .any(|issue| issue.code == "authorization_fail_open_forbidden")
        );
    }

    #[test]
    fn rate_limit_validation_returns_repeatable_owned_issues() {
        let mut config = default_config("test-service");
        config.settings.rate_limit.capacity = 0;
        config.settings.rate_limit.policy_id = "   ".into();
        config.settings.rate_limit.key_by = vec![RateLimitSignal::Route];
        config.settings.rate_limit.window_ms = 0;

        let first = validate_rate_limit_policy(&config);
        let second = validate_rate_limit_policy(&config);

        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|issue| issue.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "invalid_rate_limit",
                "required",
                "principal_required",
                "range"
            ]
        );
    }
}
