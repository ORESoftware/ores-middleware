use std::str::FromStr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{CAPABILITIES, CONTRACT_VERSION};

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
    pub key_by: Vec<String>,
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
    fn new(path: impl Into<String>, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

pub fn validate_config(config: &MiddlewareConfig) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if config.contract_version != CONTRACT_VERSION {
        issues.push(ValidationIssue::new(
            "/contractVersion",
            "unsupported_version",
            format!("expected {CONTRACT_VERSION}"),
        ));
    }
    if config.settings.timeout_ms == 0 {
        issues.push(ValidationIssue::new(
            "/settings/timeoutMs",
            "range",
            "timeout must be positive",
        ));
    }
    if config.settings.max_body_bytes == 0 {
        issues.push(ValidationIssue::new(
            "/settings/maxBodyBytes",
            "range",
            "body limit must be positive",
        ));
    }
    if config.settings.rate_limit.enabled
        && (config.settings.rate_limit.capacity == 0
            || config.settings.rate_limit.refill_per_second <= 0.0)
    {
        issues.push(ValidationIssue::new(
            "/settings/rateLimit",
            "invalid_rate_limit",
            "enabled token buckets require positive capacity and refill",
        ));
    }
    if !(0.0..=1.0).contains(&config.settings.fault_injection.error_rate)
        || !(0.0..=1.0).contains(&config.settings.fault_injection.drop_rate)
    {
        issues.push(ValidationIssue::new(
            "/settings/faultInjection",
            "range",
            "fault rates must be within 0..=1",
        ));
    }
    if matches!(config.environment, RuntimeEnvironment::Production) {
        if config.settings.fault_injection.enabled {
            issues.push(ValidationIssue::new(
                "/settings/faultInjection/enabled",
                "production_forbidden",
                "fault injection is forbidden in production",
            ));
        }
        if config.settings.test_auth_bypass.enabled {
            issues.push(ValidationIssue::new(
                "/settings/testAuthBypass/enabled",
                "production_forbidden",
                "test auth bypass is forbidden in production",
            ));
        }
    }
    if config.integrations.shared_auth.fail_open {
        issues.push(ValidationIssue::new(
            "/integrations/sharedAuth/failOpen",
            "auth_fail_open",
            "shared-auth must fail closed",
        ));
    }

    match config.settings.tls.mode.as_str() {
        "disabled" => {
            if config.settings.tls.require_https {
                issues.push(ValidationIssue::new(
                    "/settings/tls/requireHttps",
                    "disabled_tls_requires_false",
                    "TLS mode disabled cannot enforce HTTPS",
                ));
            }
        }
        "in-process" => {}
        "trusted-proxy" => {
            if config.settings.tls.trusted_proxy_cidrs.is_empty() {
                issues.push(ValidationIssue::new(
                    "/settings/tls/trustedProxyCidrs",
                    "trusted_proxy_required",
                    "trusted-proxy mode requires an explicit CIDR allowlist",
                ));
            }
        }
        _ => issues.push(ValidationIssue::new(
            "/settings/tls/mode",
            "unknown_tls_mode",
            "TLS mode must be disabled, in-process, or trusted-proxy",
        )),
    }

    for (index, cidr) in config.settings.tls.trusted_proxy_cidrs.iter().enumerate() {
        if IpNet::from_str(cidr).is_err() {
            issues.push(ValidationIssue::new(
                format!("/settings/tls/trustedProxyCidrs/{index}"),
                "invalid_cidr",
                cidr.clone(),
            ));
        }
    }

    for capability in &config.required_capabilities {
        if !CAPABILITIES.contains(&capability.as_str()) {
            issues.push(ValidationIssue::new(
                "/requiredCapabilities",
                "unknown_capability",
                capability.clone(),
            ));
        }
    }
    issues
}

pub fn default_config(service_name: impl Into<String>) -> MiddlewareConfig {
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
                key_by: vec!["tenant".into(), "user".into(), "ip".into(), "route".into()],
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
                content_security_policy: Some(
                    "default-src 'self'; frame-ancestors 'none'".into(),
                ),
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
                service_name: service_name.into(),
                exporter_endpoint: None,
                propagators: vec!["tracecontext".into(), "baggage".into()],
            },
        },
    }
}
