use std::{
    fmt::Write as _,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    context::RequestContext,
    integrations::{AuthDecision, IntegrationError, RequestMetadata},
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitAlgorithm {
    TokenBucket,
    SlidingWindowCounter,
    FixedWindow,
    Concurrency,
}

impl RateLimitAlgorithm {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TokenBucket => "token-bucket",
            Self::SlidingWindowCounter => "sliding-window-counter",
            Self::FixedWindow => "fixed-window",
            Self::Concurrency => "concurrency",
        }
    }
}

impl Default for RateLimitAlgorithm {
    fn default() -> Self {
        Self::TokenBucket
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitLayer {
    CloudflareEdge,
    KubernetesIngress,
    ServiceMesh,
    Application,
    Authorization,
}

impl RateLimitLayer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CloudflareEdge => "cloudflare-edge",
            Self::KubernetesIngress => "kubernetes-ingress",
            Self::ServiceMesh => "service-mesh",
            Self::Application => "application",
            Self::Authorization => "authorization",
        }
    }
}

impl Default for RateLimitLayer {
    fn default() -> Self {
        Self::Application
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitFailureMode {
    FailOpen,
    FailClosed,
    LocalOnly,
}

impl Default for RateLimitFailureMode {
    fn default() -> Self {
        Self::LocalOnly
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitKeyDerivationMode {
    EphemeralHmacSha256,
    ExternalHmacSha256,
}

impl Default for RateLimitKeyDerivationMode {
    fn default() -> Self {
        Self::EphemeralHmacSha256
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitSignal {
    Ip,
    IpPrefix,
    User,
    Subject,
    Email,
    Tenant,
    Organization,
    Session,
    Device,
    ApiKey,
    Route,
    Method,
}

impl RateLimitSignal {
    pub const fn is_edge_safe(self) -> bool {
        match self {
            Self::Ip | Self::IpPrefix | Self::Route | Self::Method => true,
            Self::User
            | Self::Subject
            | Self::Email
            | Self::Tenant
            | Self::Organization
            | Self::Session
            | Self::Device
            | Self::ApiKey => false,
        }
    }

    pub const fn is_principal_signal(self) -> bool {
        match self {
            Self::Route | Self::Method => false,
            Self::Ip
            | Self::IpPrefix
            | Self::User
            | Self::Subject
            | Self::Email
            | Self::Tenant
            | Self::Organization
            | Self::Session
            | Self::Device
            | Self::ApiKey => true,
        }
    }

    const fn tag(self) -> &'static str {
        match self {
            Self::Ip => "ip",
            Self::IpPrefix => "ip-prefix",
            Self::User => "user",
            Self::Subject => "subject",
            Self::Email => "email",
            Self::Tenant => "tenant",
            Self::Organization => "organization",
            Self::Session => "session",
            Self::Device => "device",
            Self::ApiKey => "api-key",
            Self::Route => "route",
            Self::Method => "method",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitDecisionKind {
    Allowed,
    Denied,
    DegradedAllowed,
    DegradedDenied,
}

impl RateLimitDecisionKind {
    pub const fn is_allowed(self) -> bool {
        match self {
            Self::Allowed | Self::DegradedAllowed => true,
            Self::Denied | Self::DegradedDenied => false,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::DegradedAllowed => "degraded-allowed",
            Self::DegradedDenied => "degraded-denied",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitDecisionSource {
    LocalMemory,
    Redis,
    CloudflareCache,
    KubernetesIngress,
    ServiceMesh,
    Authorization,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPrincipal {
    pub digest: String,
    pub key_version: String,
}

#[derive(Debug, Clone)]
pub struct RateLimitRequest {
    pub principal: RateLimitPrincipal,
    pub policy_id: String,
    pub algorithm: RateLimitAlgorithm,
    pub layer: RateLimitLayer,
    pub capacity: u32,
    pub refill_per_second: f64,
    pub window_ms: u64,
    pub cost: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitDecision {
    pub kind: RateLimitDecisionKind,
    pub source: RateLimitDecisionSource,
    pub policy_id: String,
    pub layer: RateLimitLayer,
    pub algorithm: RateLimitAlgorithm,
    pub limit: u32,
    pub remaining: u32,
    pub retry_after_ms: Option<u64>,
    pub reset_after_ms: Option<u64>,
    pub reason_code: Option<String>,
}

impl RateLimitDecision {
    pub const fn is_allowed(&self) -> bool {
        self.kind.is_allowed()
    }

    pub fn legacy(
        request: &RateLimitRequest,
        allowed: bool,
        source: RateLimitDecisionSource,
    ) -> Self {
        Self {
            kind: if allowed {
                RateLimitDecisionKind::Allowed
            } else {
                RateLimitDecisionKind::Denied
            },
            source,
            policy_id: request.policy_id.clone(),
            layer: request.layer,
            algorithm: request.algorithm,
            limit: request.capacity,
            remaining: if allowed {
                request.capacity.saturating_sub(request.cost)
            } else {
                0
            },
            retry_after_ms: (!allowed).then_some(1_000),
            reset_after_ms: None,
            reason_code: None,
        }
    }

    pub fn degraded(
        request: &RateLimitRequest,
        allowed: bool,
        source: RateLimitDecisionSource,
        reason_code: impl Into<String>,
    ) -> Self {
        Self {
            kind: if allowed {
                RateLimitDecisionKind::DegradedAllowed
            } else {
                RateLimitDecisionKind::DegradedDenied
            },
            source,
            policy_id: request.policy_id.clone(),
            layer: request.layer,
            algorithm: request.algorithm,
            limit: request.capacity,
            remaining: 0,
            retry_after_ms: (!allowed).then_some(1_000),
            reset_after_ms: None,
            reason_code: Some(reason_code.into()),
        }
    }
}

pub trait RateLimitKeyDeriver: Send + Sync {
    fn derive(
        &self,
        namespace: &str,
        key_version: &str,
        canonical_material: &[u8],
    ) -> Result<RateLimitPrincipal, IntegrationError>;
}

pub type DynRateLimitKeyDeriver = Arc<dyn RateLimitKeyDeriver>;

pub struct HmacSha256KeyDeriver {
    secret: Vec<u8>,
}

impl HmacSha256KeyDeriver {
    pub fn new(secret: impl AsRef<[u8]>) -> Result<Self, IntegrationError> {
        let secret = secret.as_ref();
        if secret.len() < 32 {
            return Err(IntegrationError {
                code: "rate_limit_hmac_key_too_short",
                message: "rate-limit HMAC keys must contain at least 32 bytes".into(),
            });
        }
        Ok(Self {
            secret: secret.to_vec(),
        })
    }

    pub fn from_key(secret: [u8; 32]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }
}

impl Drop for HmacSha256KeyDeriver {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

impl RateLimitKeyDeriver for HmacSha256KeyDeriver {
    fn derive(
        &self,
        namespace: &str,
        key_version: &str,
        canonical_material: &[u8],
    ) -> Result<RateLimitPrincipal, IntegrationError> {
        let mut mac = HmacSha256::new_from_slice(&self.secret).map_err(|_| IntegrationError {
            code: "rate_limit_hmac_initialization_failed",
            message: "rate-limit key derivation could not initialize".into(),
        })?;
        update_length_prefixed(&mut mac, namespace.as_bytes());
        update_length_prefixed(&mut mac, key_version.as_bytes());
        update_length_prefixed(&mut mac, canonical_material);
        let bytes = mac.finalize().into_bytes();
        let mut digest = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(&mut digest, "{byte:02x}");
        }
        Ok(RateLimitPrincipal {
            digest,
            key_version: key_version.to_owned(),
        })
    }
}

#[derive(Default)]
pub struct UnavailableRateLimitKeyDeriver;

impl RateLimitKeyDeriver for UnavailableRateLimitKeyDeriver {
    fn derive(
        &self,
        _namespace: &str,
        _key_version: &str,
        _canonical_material: &[u8],
    ) -> Result<RateLimitPrincipal, IntegrationError> {
        Err(IntegrationError {
            code: "rate_limit_hmac_key_unavailable",
            message: "a stable external rate-limit HMAC key was not installed".into(),
        })
    }
}

pub fn derive_rate_limit_principal(
    deriver: &dyn RateLimitKeyDeriver,
    namespace: &str,
    key_version: &str,
    signals: &[RateLimitSignal],
    context: &RequestContext,
    request: &RequestMetadata,
    auth: &AuthDecision,
    effective_client_ip: Option<&str>,
) -> Result<RateLimitPrincipal, IntegrationError> {
    let mut canonical = Vec::with_capacity(signals.len() * 48);
    let mut has_principal_material = false;

    for signal in signals {
        let value = signal_value(*signal, context, request, auth, effective_client_ip);
        if signal.is_principal_signal() && value.is_some() {
            has_principal_material = true;
        }
        update_vec_length_prefixed(&mut canonical, signal.tag().as_bytes());
        match value {
            Some(value) => update_vec_length_prefixed(&mut canonical, value.as_bytes()),
            None => update_vec_length_prefixed(&mut canonical, b"<missing>"),
        }
    }

    if !has_principal_material {
        canonical.fill(0);
        return Err(IntegrationError {
            code: "rate_limit_principal_unavailable",
            message: "no configured principal signal was available for rate limiting".into(),
        });
    }

    let result = deriver.derive(namespace, key_version, &canonical);
    canonical.fill(0);
    result
}

fn signal_value(
    signal: RateLimitSignal,
    context: &RequestContext,
    request: &RequestMetadata,
    auth: &AuthDecision,
    effective_client_ip: Option<&str>,
) -> Option<String> {
    match signal {
        RateLimitSignal::Ip => effective_client_ip.map(str::to_owned),
        RateLimitSignal::IpPrefix => effective_client_ip.and_then(ip_prefix),
        RateLimitSignal::User | RateLimitSignal::Subject => context.user_id.clone(),
        RateLimitSignal::Email => claim(auth, &["email", "email_address"])
            .map(|value| value.trim().to_ascii_lowercase()),
        RateLimitSignal::Tenant => context.tenant_id.clone(),
        RateLimitSignal::Organization => {
            claim(auth, &["organization_id", "org_id"]).map(str::to_owned)
        }
        RateLimitSignal::Session => claim(auth, &["session_id", "sid"]).map(str::to_owned),
        RateLimitSignal::Device => claim(auth, &["device_id"]).map(str::to_owned),
        RateLimitSignal::ApiKey => claim(auth, &["api_key_id", "client_id"]).map(str::to_owned),
        RateLimitSignal::Route => Some(request.path.clone()),
        RateLimitSignal::Method => Some(request.method.to_ascii_uppercase()),
    }
}

fn claim<'a>(auth: &'a AuthDecision, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| auth.claims.get(*name).map(String::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn ip_prefix(value: &str) -> Option<String> {
    match value.parse::<IpAddr>().ok()? {
        IpAddr::V4(ip) => {
            let mut octets = ip.octets();
            octets[3] = 0;
            Some(format!("{}/24", Ipv4Addr::from(octets)))
        }
        IpAddr::V6(ip) => {
            let mut octets = ip.octets();
            for byte in &mut octets[7..] {
                *byte = 0;
            }
            Some(format!("{}/56", Ipv6Addr::from(octets)))
        }
    }
}

fn update_length_prefixed(mac: &mut HmacSha256, value: &[u8]) {
    mac.update(&(value.len() as u64).to_be_bytes());
    mac.update(value);
}

fn update_vec_length_prefixed(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u64).to_be_bytes());
    target.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn context() -> RequestContext {
        RequestContext {
            request_id: "request-1".into(),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: None,
            tenant_id: Some("tenant-raw".into()),
            user_id: Some("user-raw".into()),
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
        }
    }

    fn request() -> RequestMetadata {
        RequestMetadata {
            method: "post".into(),
            path: "/v1/login".into(),
            headers: BTreeMap::new(),
            remote_ip: Some("203.0.113.99".into()),
            content_length: None,
            transport_secure: true,
        }
    }

    #[test]
    fn hmac_key_never_contains_raw_identity_material() {
        let deriver = HmacSha256KeyDeriver::new([7_u8; 32]).unwrap();
        let auth = AuthDecision {
            user_id: Some("user-raw".into()),
            tenant_id: Some("tenant-raw".into()),
            claims: BTreeMap::from([("email".into(), "Person@Example.com".into())]),
        };
        let principal = derive_rate_limit_principal(
            &deriver,
            "shared-auth",
            "v1",
            &[
                RateLimitSignal::User,
                RateLimitSignal::Email,
                RateLimitSignal::Ip,
                RateLimitSignal::Route,
            ],
            &context(),
            &request(),
            &auth,
            Some("203.0.113.99"),
        )
        .unwrap();

        assert_eq!(principal.digest.len(), 64);
        assert!(!principal.digest.contains("user-raw"));
        assert!(!principal.digest.contains("example.com"));
        assert!(!principal.digest.contains("203.0.113.99"));
        assert!(!principal.digest.contains("/v1/login"));
    }

    #[test]
    fn key_version_and_namespace_domain_separate_principals() {
        let deriver = HmacSha256KeyDeriver::new([9_u8; 32]).unwrap();
        let auth = AuthDecision::default();
        let a = derive_rate_limit_principal(
            &deriver,
            "service-a",
            "v1",
            &[RateLimitSignal::Ip],
            &context(),
            &request(),
            &auth,
            Some("203.0.113.99"),
        )
        .unwrap();
        let b = derive_rate_limit_principal(
            &deriver,
            "service-b",
            "v2",
            &[RateLimitSignal::Ip],
            &context(),
            &request(),
            &auth,
            Some("203.0.113.99"),
        )
        .unwrap();
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn ipv4_and_ipv6_prefixes_are_privacy_reduced() {
        assert_eq!(ip_prefix("203.0.113.99").as_deref(), Some("203.0.113.0/24"));
        assert_eq!(
            ip_prefix("2001:db8:abcd:1234:5678::1").as_deref(),
            Some("2001:db8:abcd:1200::/56")
        );
    }

    #[test]
    fn edge_safety_is_exhaustive() {
        let signals = [
            RateLimitSignal::Ip,
            RateLimitSignal::IpPrefix,
            RateLimitSignal::User,
            RateLimitSignal::Subject,
            RateLimitSignal::Email,
            RateLimitSignal::Tenant,
            RateLimitSignal::Organization,
            RateLimitSignal::Session,
            RateLimitSignal::Device,
            RateLimitSignal::ApiKey,
            RateLimitSignal::Route,
            RateLimitSignal::Method,
        ];
        assert_eq!(signals.iter().filter(|signal| signal.is_edge_safe()).count(), 4);
    }
}
