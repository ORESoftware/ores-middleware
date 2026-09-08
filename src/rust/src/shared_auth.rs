//! Fail-closed construction boundary for services that incorporate Shared Auth
//! through `ores-middleware`.
//!
//! `MiddlewareStack::new` remains available for public/anonymous middleware
//! deployments. Protected customer and admin services should construct this
//! wrapper instead: it cannot be created without an explicit verifier and a
//! validated dedicated Supabase + Neon topology.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    config::IntegrationMode, validate_config, ActiveRequest, AuthVerifier, MiddlewareConfig,
    MiddlewareError, MiddlewareStack, RequestMetadata, ValidationIssue,
};

pub const SUPABASE_AUTH_DATABASE_URL_ENV: &str = "SUPABASE_AUTH_DATABASE_URL";
pub const NEON_AUTH_DATABASE_URL_ENV: &str = "NEON_AUTH_DATABASE_URL";
pub const SUPABASE_ADMIN_DATABASE_URL_ENV: &str = "SUPABASE_ADMIN_DATABASE_URL";
pub const NEON_ADMIN_DATABASE_URL_ENV: &str = "NEON_ADMIN_DATABASE_URL";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthServerRole {
    WebServer,
    ApiServer,
    AdminWebServer,
    AdminApiServer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthDataPlane {
    CustomerAuth,
    AdminAuth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthDecisionMode {
    AvailabilityFirst,
    StrictPaired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedAuthDatabaseEnvKeys {
    pub supabase: &'static str,
    pub neon: &'static str,
}

impl SharedAuthServerRole {
    #[must_use]
    pub const fn data_plane(self) -> SharedAuthDataPlane {
        match self {
            Self::WebServer | Self::ApiServer => SharedAuthDataPlane::CustomerAuth,
            Self::AdminWebServer | Self::AdminApiServer => SharedAuthDataPlane::AdminAuth,
        }
    }

    #[must_use]
    pub const fn database_env_keys(self) -> SharedAuthDatabaseEnvKeys {
        match self.data_plane() {
            SharedAuthDataPlane::CustomerAuth => SharedAuthDatabaseEnvKeys {
                supabase: SUPABASE_AUTH_DATABASE_URL_ENV,
                neon: NEON_AUTH_DATABASE_URL_ENV,
            },
            SharedAuthDataPlane::AdminAuth => SharedAuthDatabaseEnvKeys {
                supabase: SUPABASE_ADMIN_DATABASE_URL_ENV,
                neon: NEON_ADMIN_DATABASE_URL_ENV,
            },
        }
    }

    #[must_use]
    pub const fn is_admin(self) -> bool {
        matches!(self, Self::AdminWebServer | Self::AdminApiServer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthRuntimeTopology {
    pub github_org: String,
    pub supabase_org: String,
    pub neon_org: String,
    pub server_role: SharedAuthServerRole,
    pub audience: String,
    pub decision_mode: SharedAuthDecisionMode,
}

impl SharedAuthRuntimeTopology {
    pub fn new(
        github_org: impl Into<String>,
        supabase_org: impl Into<String>,
        neon_org: impl Into<String>,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let topology = Self {
            github_org: github_org.into(),
            supabase_org: supabase_org.into(),
            neon_org: neon_org.into(),
            server_role,
            audience: audience.into(),
            decision_mode,
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
    pub const fn database_env_keys(&self) -> SharedAuthDatabaseEnvKeys {
        self.server_role.database_env_keys()
    }

    #[must_use]
    pub fn validation_issues(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if !valid_org_slug(&self.github_org) {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/githubOrg",
                "invalid_org_slug",
                "GitHub organization must be a non-empty organization slug",
            ));
        }
        if self.supabase_org != self.github_org {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/supabaseOrg",
                "shared_auth_provider_org_mismatch",
                "Supabase organization must exactly match the GitHub organization",
            ));
        }
        if self.neon_org != self.github_org {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/neonOrg",
                "shared_auth_provider_org_mismatch",
                "Neon organization must exactly match the GitHub organization",
            ));
        }
        if self.audience.trim().is_empty() {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/audience",
                "shared_auth_audience_required",
                "Shared Auth audience must not be empty",
            ));
        }
        if self.server_role.is_admin()
            && self.decision_mode != SharedAuthDecisionMode::StrictPaired
        {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/decisionMode",
                "shared_auth_admin_requires_strict_paired",
                "admin web and API servers require strict paired provider proof",
            ));
        }
        issues
    }
}

/// A middleware stack that has passed the Shared Auth readiness boundary.
///
/// The constructor requires a concrete verifier. The crate-private
/// `AnonymousAuth` default cannot be supplied by external services, and a
/// verifier that returns no canonical user remains rejected by the underlying
/// request pipeline.
pub struct SharedAuthReadyStack {
    inner: MiddlewareStack,
    topology: SharedAuthRuntimeTopology,
}

impl SharedAuthReadyStack {
    pub fn new<V>(
        config: MiddlewareConfig,
        topology: SharedAuthRuntimeTopology,
        verifier: V,
    ) -> Result<Self, Vec<ValidationIssue>>
    where
        V: AuthVerifier + 'static,
    {
        let mut issues = validate_config(&config);
        issues.extend(topology.validation_issues());

        let shared_auth = &config.integrations.shared_auth;
        if matches!(shared_auth.mode, IntegrationMode::Disabled) {
            issues.push(ValidationIssue::new(
                "/integrations/sharedAuth/mode",
                "shared_auth_verifier_required",
                "protected services must enable Shared Auth before startup",
            ));
        }
        if shared_auth
            .issuer
            .as_deref()
            .map_or(true, |issuer| issuer.trim().is_empty())
        {
            issues.push(ValidationIssue::new(
                "/integrations/sharedAuth/issuer",
                "shared_auth_issuer_required",
                "Shared Auth issuer must be configured",
            ));
        }
        match shared_auth.audience.as_deref() {
            Some(audience) if audience == topology.audience => {}
            _ => issues.push(ValidationIssue::new(
                "/integrations/sharedAuth/audience",
                "shared_auth_audience_mismatch",
                "middleware audience must exactly match the topology audience",
            )),
        }
        if matches!(shared_auth.mode, IntegrationMode::Http)
            && shared_auth
                .jwks_uri
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .is_none()
            && shared_auth
                .introspection_url
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .is_none()
        {
            issues.push(ValidationIssue::new(
                "/integrations/sharedAuth",
                "shared_auth_verification_endpoint_required",
                "HTTP Shared Auth requires JWKS or introspection configuration",
            ));
        }

        if !issues.is_empty() {
            return Err(issues);
        }

        let inner = MiddlewareStack::new(config)?.with_auth_verifier(Arc::new(verifier));
        Ok(Self { inner, topology })
    }

    #[must_use]
    pub fn config(&self) -> &MiddlewareConfig {
        self.inner.config()
    }

    #[must_use]
    pub const fn topology(&self) -> &SharedAuthRuntimeTopology {
        &self.topology
    }

    pub async fn begin(
        &self,
        request: RequestMetadata,
    ) -> Result<ActiveRequest, MiddlewareError> {
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

fn valid_org_slug(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, future::Future, pin::Pin};

    use super::*;
    use crate::{default_config, AuthDecision, IntegrationError, RuntimeEnvironment};

    struct CanonicalVerifier;

    impl AuthVerifier for CanonicalVerifier {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
        ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
            Box::pin(async {
                Ok(AuthDecision {
                    user_id: Some("user-123".into()),
                    tenant_id: Some("tenant-456".into()),
                    claims: BTreeMap::new(),
                })
            })
        }
    }

    fn protected_config(audience: &str) -> MiddlewareConfig {
        let mut config = default_config("test-service");
        config.environment = RuntimeEnvironment::Test;
        config.integrations.shared_auth.mode = IntegrationMode::Http;
        config.integrations.shared_auth.issuer = Some("https://auth.example.invalid".into());
        config.integrations.shared_auth.audience = Some(audience.into());
        config.integrations.shared_auth.jwks_uri =
            Some("https://auth.example.invalid/.well-known/jwks.json".into());
        config
    }

    fn topology(role: SharedAuthServerRole) -> SharedAuthRuntimeTopology {
        SharedAuthRuntimeTopology::new(
            "messaging-intel",
            "messaging-intel",
            "messaging-intel",
            role,
            "msgint",
            if role.is_admin() {
                SharedAuthDecisionMode::StrictPaired
            } else {
                SharedAuthDecisionMode::AvailabilityFirst
            },
        )
        .expect("valid dedicated topology")
    }

    #[test]
    fn customer_and_admin_roles_use_disjoint_database_keys() {
        assert_eq!(
            SharedAuthServerRole::WebServer.database_env_keys(),
            SharedAuthDatabaseEnvKeys {
                supabase: SUPABASE_AUTH_DATABASE_URL_ENV,
                neon: NEON_AUTH_DATABASE_URL_ENV,
            }
        );
        assert_eq!(
            SharedAuthServerRole::AdminApiServer.database_env_keys(),
            SharedAuthDatabaseEnvKeys {
                supabase: SUPABASE_ADMIN_DATABASE_URL_ENV,
                neon: NEON_ADMIN_DATABASE_URL_ENV,
            }
        );
    }

    #[test]
    fn rejects_shared_provider_organizations() {
        let issues = SharedAuthRuntimeTopology::new(
            "messaging-intel",
            "oresoftware",
            "messaging-intel",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("shared Supabase organization must fail closed");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_provider_org_mismatch")
        );
    }

    #[test]
    fn rejects_availability_first_for_admin_servers() {
        let issues = SharedAuthRuntimeTopology::new(
            "messaging-intel",
            "messaging-intel",
            "messaging-intel",
            SharedAuthServerRole::AdminWebServer,
            "msgint-admin",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("admin availability-first must fail closed");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_admin_requires_strict_paired")
        );
    }

    #[test]
    fn construction_requires_enabled_shared_auth_and_matching_audience() {
        let disabled = default_config("test-service");
        let issues = SharedAuthReadyStack::new(
            disabled,
            topology(SharedAuthServerRole::WebServer),
            CanonicalVerifier,
        )
        .err()
        .expect("disabled Shared Auth must be rejected");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_verifier_required")
        );

        let issues = SharedAuthReadyStack::new(
            protected_config("wrong-audience"),
            topology(SharedAuthServerRole::WebServer),
            CanonicalVerifier,
        )
        .err()
        .expect("audience mismatch must be rejected");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_audience_mismatch")
        );
    }

    #[test]
    fn construction_accepts_explicit_verifier_and_dedicated_topology() {
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(SharedAuthServerRole::ApiServer),
            CanonicalVerifier,
        )
        .expect("explicit verifier should satisfy readiness");
        assert_eq!(ready.topology().github_org, "messaging-intel");
        assert_eq!(
            ready.topology().data_plane(),
            SharedAuthDataPlane::CustomerAuth
        );
    }
}
