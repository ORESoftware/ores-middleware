//! Fail-closed construction boundary for protected services using Shared Auth.
//!
//! Public/anonymous middleware may still construct `MiddlewareStack` directly.
//! Protected product/admin services should use `SharedAuthReadyStack`, which
//! requires an explicit verifier and a validated provider/data-plane topology.

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SupabaseTopology {
    /// Long-term target: one Supabase organization dedicated to the GitHub org.
    DedicatedOrganization { organization: String },
    /// Explicit near-term exception: a shared provider organization with a
    /// dedicated PostgreSQL schema namespace supplied by RuntimeConfig.
    SharedSchema {
        organization: String,
        schema_namespace: String,
    },
}

impl SupabaseTopology {
    #[must_use]
    pub fn organization(&self) -> &str {
        match self {
            Self::DedicatedOrganization { organization }
            | Self::SharedSchema { organization, .. } => organization,
        }
    }

    #[must_use]
    pub fn schema_namespace(&self) -> Option<&str> {
        match self {
            Self::DedicatedOrganization { .. } => None,
            Self::SharedSchema {
                schema_namespace, ..
            } => Some(schema_namespace),
        }
    }
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
    pub supabase: SupabaseTopology,
    pub neon_org: String,
    pub server_role: SharedAuthServerRole,
    pub audience: String,
    pub decision_mode: SharedAuthDecisionMode,
}

impl SharedAuthRuntimeTopology {
    pub fn dedicated(
        github_org: impl Into<String>,
        neon_org: impl Into<String>,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let github_org = github_org.into();
        Self::new(
            github_org.clone(),
            SupabaseTopology::DedicatedOrganization {
                organization: github_org,
            },
            neon_org,
            server_role,
            audience,
            decision_mode,
        )
    }

    pub fn shared_supabase_schema(
        github_org: impl Into<String>,
        shared_supabase_org: impl Into<String>,
        schema_namespace: impl Into<String>,
        neon_org: impl Into<String>,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        Self::new(
            github_org,
            SupabaseTopology::SharedSchema {
                organization: shared_supabase_org.into(),
                schema_namespace: schema_namespace.into(),
            },
            neon_org,
            server_role,
            audience,
            decision_mode,
        )
    }

    pub fn new(
        github_org: impl Into<String>,
        supabase: SupabaseTopology,
        neon_org: impl Into<String>,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let topology = Self {
            github_org: github_org.into(),
            supabase,
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
        if !valid_org_slug(&self.neon_org) || self.neon_org != self.github_org {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/neonOrg",
                "shared_auth_neon_org_mismatch",
                "Neon organization must exactly match the GitHub organization",
            ));
        }
        match &self.supabase {
            SupabaseTopology::DedicatedOrganization { organization } => {
                if !valid_org_slug(organization) || organization != &self.github_org {
                    issues.push(ValidationIssue::new(
                        "/sharedAuthTopology/supabase/organization",
                        "shared_auth_supabase_org_mismatch",
                        "dedicated Supabase organization must exactly match the GitHub organization",
                    ));
                }
            }
            SupabaseTopology::SharedSchema {
                organization,
                schema_namespace,
            } => {
                if !valid_org_slug(organization) {
                    issues.push(ValidationIssue::new(
                        "/sharedAuthTopology/supabase/organization",
                        "invalid_org_slug",
                        "shared Supabase provider organization must be a valid organization slug",
                    ));
                }
                if !valid_schema_namespace(schema_namespace) {
                    issues.push(ValidationIssue::new(
                        "/sharedAuthTopology/supabase/schemaNamespace",
                        "shared_auth_schema_namespace_required",
                        "shared Supabase mode requires a dedicated non-public PostgreSQL schema namespace",
                    ));
                }
            }
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
            .is_none_or(|issuer| issuer.trim().is_empty())
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
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn valid_schema_namespace(value: &str) -> bool {
    if value.eq_ignore_ascii_case("public") {
        return false;
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
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
    fn dedicated_supabase_requires_matching_org() {
        let issues = SharedAuthRuntimeTopology::new(
            "messaging-intel",
            SupabaseTopology::DedicatedOrganization {
                organization: "oresoftware".into(),
            },
            "messaging-intel",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("mismatched dedicated Supabase org must fail");
        assert!(issues.iter().any(|issue| issue.code == "shared_auth_supabase_org_mismatch"));
    }

    #[test]
    fn shared_supabase_requires_explicit_non_public_schema() {
        let topology = SharedAuthRuntimeTopology::shared_supabase_schema(
            "messaging-intel",
            "oresoftware",
            "messaging_intel",
            "messaging-intel",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect("explicit shared provider plus per-org schema is valid");
        assert_eq!(topology.supabase.organization(), "oresoftware");
        assert_eq!(topology.supabase.schema_namespace(), Some("messaging_intel"));

        for schema in ["", "public", "bad-schema"] {
            let issues = SharedAuthRuntimeTopology::shared_supabase_schema(
                "messaging-intel",
                "oresoftware",
                schema,
                "messaging-intel",
                SharedAuthServerRole::ApiServer,
                "msgint",
                SharedAuthDecisionMode::AvailabilityFirst,
            )
            .expect_err("shared Supabase without dedicated schema must fail");
            assert!(issues.iter().any(|issue| issue.code == "shared_auth_schema_namespace_required"));
        }
    }

    #[test]
    fn neon_remains_dedicated_to_github_org() {
        let issues = SharedAuthRuntimeTopology::shared_supabase_schema(
            "messaging-intel",
            "oresoftware",
            "messaging_intel",
            "another-org",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("shared Neon org must fail closed");
        assert!(issues.iter().any(|issue| issue.code == "shared_auth_neon_org_mismatch"));
    }

    #[test]
    fn admin_requires_strict_paired_mode() {
        let issues = SharedAuthRuntimeTopology::shared_supabase_schema(
            "messaging-intel",
            "oresoftware",
            "messaging_intel",
            "messaging-intel",
            SharedAuthServerRole::AdminWebServer,
            "msgint-admin",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("admin availability-first must fail closed");
        assert!(issues.iter().any(|issue| issue.code == "shared_auth_admin_requires_strict_paired"));
    }

    #[test]
    fn construction_requires_enabled_shared_auth_and_matching_audience() {
        let topology = SharedAuthRuntimeTopology::shared_supabase_schema(
            "messaging-intel",
            "oresoftware",
            "messaging_intel",
            "messaging-intel",
            SharedAuthServerRole::WebServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .unwrap();
        let issues = SharedAuthReadyStack::new(default_config("test-service"), topology.clone(), CanonicalVerifier)
            .err()
            .expect("disabled Shared Auth must be rejected");
        assert!(issues.iter().any(|issue| issue.code == "shared_auth_verifier_required"));

        let issues = SharedAuthReadyStack::new(protected_config("wrong"), topology, CanonicalVerifier)
            .err()
            .expect("audience mismatch must be rejected");
        assert!(issues.iter().any(|issue| issue.code == "shared_auth_audience_mismatch"));
    }

    #[test]
    fn construction_accepts_explicit_verifier_and_shared_schema_topology() {
        let topology = SharedAuthRuntimeTopology::shared_supabase_schema(
            "messaging-intel",
            "oresoftware",
            "messaging_intel",
            "messaging-intel",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .unwrap();
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology,
            CanonicalVerifier,
        )
        .expect("explicit verifier plus scoped topology should satisfy readiness");
        assert_eq!(ready.topology().data_plane(), SharedAuthDataPlane::CustomerAuth);
        assert_eq!(ready.topology().supabase.schema_namespace(), Some("messaging_intel"));
    }
}
