//! Protected middleware boundary for services that consume a unified Shared Auth token.
//!
//! `SharedAuthReadyStack` remains the authority for deployments that verify
//! Supabase and Neon provider proofs directly. Resource servers that receive a
//! token already minted by `shared-auth-server` must not pretend one unified
//! introspection call is two independent provider proofs. This stack preserves
//! the protected-service construction boundary while accepting one verifier for
//! the Shared Auth service token/JWKS/introspection contract.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    ActiveRequest, AuthVerifier, IntegrationError, MiddlewareConfig, MiddlewareError,
    MiddlewareStack, RequestMetadata, ValidationIssue,
    config::IntegrationMode,
    rate_limit::RateLimitKeyDerivationMode,
    shared_auth::{SharedAuthDataPlane, SharedAuthServerRole},
    validate_config,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthServiceTopology {
    pub github_org: String,
    pub server_role: SharedAuthServerRole,
    pub issuer: String,
    pub audience: String,
}

impl SharedAuthServiceTopology {
    pub fn new(
        github_org: impl Into<String>,
        server_role: SharedAuthServerRole,
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let topology = Self {
            github_org: github_org.into(),
            server_role,
            issuer: issuer.into(),
            audience: audience.into(),
        };
        let issues = topology.validation_issues();
        if issues.is_empty() {
            Ok(topology)
        } else {
            Err(issues)
        }
    }

    #[must_use]
    pub const fn data_plane(&self) -> SharedAuthDataPlane {
        self.server_role.data_plane()
    }

    #[must_use]
    pub fn validation_issues(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if !valid_org_slug(&self.github_org) {
            issues.push(ValidationIssue::new(
                "/sharedAuthServiceTopology/githubOrg",
                "invalid_org_slug",
                "GitHub organization must be a non-empty organization slug",
            ));
        }
        if !valid_https_issuer(&self.issuer) {
            issues.push(ValidationIssue::new(
                "/sharedAuthServiceTopology/issuer",
                "shared_auth_service_issuer_invalid",
                "Shared Auth issuer must be a non-empty HTTPS URL without whitespace",
            ));
        }
        if self.audience.trim().is_empty() {
            issues.push(ValidationIssue::new(
                "/sharedAuthServiceTopology/audience",
                "shared_auth_service_audience_required",
                "Shared Auth audience must not be empty",
            ));
        }
        issues
    }
}

#[derive(Debug)]
pub enum SharedAuthServiceStackError {
    Validation(Vec<ValidationIssue>),
    RateLimitKey(IntegrationError),
}

impl std::fmt::Display for SharedAuthServiceStackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(issues) => {
                write!(formatter, "Shared Auth service stack has {} validation issue(s)", issues.len())
            }
            Self::RateLimitKey(error) => write!(formatter, "rate-limit key rejected: {error}"),
        }
    }
}

impl std::error::Error for SharedAuthServiceStackError {}

pub struct SharedAuthServiceReadyStack {
    inner: MiddlewareStack,
    topology: SharedAuthServiceTopology,
}

impl SharedAuthServiceReadyStack {
    /// Construct a protected unified-Shared-Auth stack when the middleware's
    /// rate-limit key derivation is self-contained (for example ephemeral HMAC).
    /// External-HMAC configurations must use [`Self::new_with_rate_limit_hmac_key`].
    pub fn new<P>(
        config: MiddlewareConfig,
        topology: SharedAuthServiceTopology,
        verifier: P,
    ) -> Result<Self, SharedAuthServiceStackError>
    where
        P: AuthVerifier + 'static,
    {
        if config.settings.rate_limit.enabled
            && matches!(
                config.settings.rate_limit.key_derivation,
                RateLimitKeyDerivationMode::ExternalHmacSha256
            )
        {
            return Err(SharedAuthServiceStackError::Validation(vec![
                ValidationIssue::new(
                    "/settings/rateLimit/keyDerivation",
                    "external_rate_limit_key_required",
                    "protected Shared Auth service stack requires an explicit external HMAC key",
                ),
            ]));
        }
        Self::build(config, topology, verifier, None)
    }

    /// Construct a protected unified-Shared-Auth stack and install the external
    /// HMAC key before the stack can serve a request.
    pub fn new_with_rate_limit_hmac_key<P>(
        config: MiddlewareConfig,
        topology: SharedAuthServiceTopology,
        verifier: P,
        hmac_key: impl AsRef<[u8]>,
    ) -> Result<Self, SharedAuthServiceStackError>
    where
        P: AuthVerifier + 'static,
    {
        Self::build(config, topology, verifier, Some(hmac_key.as_ref()))
    }

    fn build<P>(
        config: MiddlewareConfig,
        topology: SharedAuthServiceTopology,
        verifier: P,
        hmac_key: Option<&[u8]>,
    ) -> Result<Self, SharedAuthServiceStackError>
    where
        P: AuthVerifier + 'static,
    {
        let issues: Vec<ValidationIssue> = validate_config(&config)
            .into_iter()
            .chain(topology.validation_issues())
            .chain(service_integration_issues(&config, &topology))
            .collect();
        if !issues.is_empty() {
            return Err(SharedAuthServiceStackError::Validation(issues));
        }

        let mut inner = MiddlewareStack::new(config)
            .map_err(SharedAuthServiceStackError::Validation)?
            .with_auth_verifier(Arc::new(verifier));
        if let Some(key) = hmac_key {
            inner = inner
                .with_rate_limit_hmac_key(key)
                .map_err(SharedAuthServiceStackError::RateLimitKey)?;
        }
        Ok(Self { inner, topology })
    }

    #[must_use]
    pub fn config(&self) -> &MiddlewareConfig {
        self.inner.config()
    }

    #[must_use]
    pub const fn topology(&self) -> &SharedAuthServiceTopology {
        &self.topology
    }

    pub async fn begin(&self, request: RequestMetadata) -> Result<ActiveRequest, MiddlewareError> {
        self.inner.begin(request).await
    }

    pub async fn finish(
        &self,
        active: ActiveRequest,
        status: u16,
        response_bytes: Option<u64>,
    ) -> BTreeMap<String, String> {
        self.inner.finish(active, status, response_bytes).await
    }
}

fn service_integration_issues(
    config: &MiddlewareConfig,
    topology: &SharedAuthServiceTopology,
) -> impl Iterator<Item = ValidationIssue> {
    let shared_auth = &config.integrations.shared_auth;
    let mode = (!matches!(shared_auth.mode, IntegrationMode::Http)).then(|| {
        ValidationIssue::new(
            "/integrations/sharedAuth/mode",
            "shared_auth_service_http_required",
            "unified Shared Auth service verification requires HTTP/JWKS service mode",
        )
    });
    let fail_open = shared_auth.fail_open.then(|| {
        ValidationIssue::new(
            "/integrations/sharedAuth/failOpen",
            "shared_auth_service_fail_open_forbidden",
            "protected Shared Auth service verification must fail closed",
        )
    });
    let issuer = (shared_auth.issuer.as_deref() != Some(topology.issuer.as_str())).then(|| {
        ValidationIssue::new(
            "/integrations/sharedAuth/issuer",
            "shared_auth_service_issuer_mismatch",
            "middleware issuer must exactly match the Shared Auth service topology",
        )
    });
    let audience = (shared_auth.audience.as_deref() != Some(topology.audience.as_str())).then(|| {
        ValidationIssue::new(
            "/integrations/sharedAuth/audience",
            "shared_auth_service_audience_mismatch",
            "middleware audience must exactly match the Shared Auth service topology",
        )
    });
    let endpoint = (shared_auth.jwks_uri.as_deref().is_none_or(str::is_empty)
        && shared_auth
            .introspection_url
            .as_deref()
            .is_none_or(str::is_empty))
    .then(|| {
        ValidationIssue::new(
            "/integrations/sharedAuth",
            "shared_auth_service_verification_endpoint_required",
            "Shared Auth service verification requires JWKS or introspection configuration",
        )
    });

    mode.into_iter()
        .chain(fail_open)
        .chain(issuer)
        .chain(audience)
        .chain(endpoint)
}

fn valid_org_slug(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        && value
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn valid_https_issuer(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() > "https://".len()
        && !value.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthDecision, RuntimeEnvironment, auth_provider_fn, default_config,
        rate_limit::RateLimitKeyDerivationMode,
    };

    fn topology(role: SharedAuthServerRole) -> SharedAuthServiceTopology {
        SharedAuthServiceTopology::new(
            "okla-platform",
            role,
            if role.is_admin() {
                "https://admin-auth.okla.invalid"
            } else {
                "https://auth.okla.invalid"
            },
            if role.is_admin() { "okla-admin" } else { "okla" },
        )
        .unwrap()
    }

    fn config(role: SharedAuthServerRole) -> MiddlewareConfig {
        let topology = topology(role);
        let mut config = default_config("okla-test");
        config.environment = RuntimeEnvironment::Test;
        config.integrations.shared_auth.mode = IntegrationMode::Http;
        config.integrations.shared_auth.fail_open = false;
        config.integrations.shared_auth.issuer = Some(topology.issuer.clone());
        config.integrations.shared_auth.audience = Some(topology.audience.clone());
        config.integrations.shared_auth.introspection_url =
            Some(format!("{}/auth/introspect", topology.issuer));
        config.settings.rate_limit.key_derivation = RateLimitKeyDerivationMode::EphemeralHmacSha256;
        config
    }

    fn verifier() -> impl AuthVerifier {
        auth_provider_fn(|_request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: Some("principal-1".into()),
                tenant_id: Some("tenant-1".into()),
                claims: BTreeMap::new(),
            })
        })
    }

    #[test]
    fn admin_topology_selects_admin_data_plane() {
        assert_eq!(
            topology(SharedAuthServerRole::AdminApiServer).data_plane(),
            SharedAuthDataPlane::AdminAuth
        );
    }

    #[test]
    fn protected_service_rejects_disabled_or_fail_open_shared_auth() {
        let topology = topology(SharedAuthServerRole::ApiServer);
        let mut disabled = config(SharedAuthServerRole::ApiServer);
        disabled.integrations.shared_auth.mode = IntegrationMode::Disabled;
        assert!(matches!(
            SharedAuthServiceReadyStack::new(disabled, topology.clone(), verifier()),
            Err(SharedAuthServiceStackError::Validation(_))
        ));

        let mut fail_open = config(SharedAuthServerRole::ApiServer);
        fail_open.integrations.shared_auth.fail_open = true;
        assert!(matches!(
            SharedAuthServiceReadyStack::new(fail_open, topology, verifier()),
            Err(SharedAuthServiceStackError::Validation(_))
        ));
    }

    #[test]
    fn external_hmac_config_is_not_ready_without_explicit_key() {
        let topology = topology(SharedAuthServerRole::ApiServer);
        let mut config = config(SharedAuthServerRole::ApiServer);
        config.settings.rate_limit.key_derivation = RateLimitKeyDerivationMode::ExternalHmacSha256;
        assert!(matches!(
            SharedAuthServiceReadyStack::new(config, topology, verifier()),
            Err(SharedAuthServiceStackError::Validation(_))
        ));
    }

    #[tokio::test]
    async fn unified_verifier_establishes_request_context() {
        let topology = topology(SharedAuthServerRole::ApiServer);
        let stack = SharedAuthServiceReadyStack::new(
            config(SharedAuthServerRole::ApiServer),
            topology,
            verifier(),
        )
        .unwrap();
        let active = stack
            .begin(RequestMetadata {
                method: "GET".into(),
                path: "/v1/context".into(),
                headers: BTreeMap::new(),
                remote_ip: Some("127.0.0.1".into()),
                content_length: None,
                transport_secure: true,
            })
            .await
            .unwrap();
        assert_eq!(active.context.user_id.as_deref(), Some("principal-1"));
        assert_eq!(active.context.tenant_id.as_deref(), Some("tenant-1"));
    }

    #[tokio::test]
    async fn external_hmac_key_is_installed_before_first_request() {
        let topology = topology(SharedAuthServerRole::ApiServer);
        let mut config = config(SharedAuthServerRole::ApiServer);
        config.settings.rate_limit.key_derivation = RateLimitKeyDerivationMode::ExternalHmacSha256;
        let stack = SharedAuthServiceReadyStack::new_with_rate_limit_hmac_key(
            config,
            topology,
            verifier(),
            b"0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        let active = stack
            .begin(RequestMetadata {
                method: "GET".into(),
                path: "/v1/context".into(),
                headers: BTreeMap::new(),
                remote_ip: Some("127.0.0.1".into()),
                content_length: None,
                transport_secure: true,
            })
            .await
            .unwrap();
        assert_eq!(active.context.user_id.as_deref(), Some("principal-1"));
    }
}
