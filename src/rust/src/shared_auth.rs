//! Fail-closed construction boundary for protected services using Shared Auth.
//!
//! Public/anonymous middleware may still construct `MiddlewareStack` directly.
//! Protected customer and admin services must construct `SharedAuthReadyStack`.
//! The protected boundary requires two explicit provider verifiers and applies
//! the configured paired-provider policy before producing an `AuthDecision`.

use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

use crate::{
    ActiveRequest, AuthDecision, AuthVerifier, IntegrationError, MiddlewareConfig, MiddlewareError,
    MiddlewareStack, RequestMetadata, ValidationIssue, config::IntegrationMode, validate_config,
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

impl SharedAuthDataPlane {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomerAuth => "customer-auth",
            Self::AdminAuth => "admin-auth",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthDecisionMode {
    AvailabilityFirst,
    StrictPaired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthProvider {
    Supabase,
    Neon,
}

impl SharedAuthProvider {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supabase => "supabase",
            Self::Neon => "neon",
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
pub struct SharedAuthProviderTopology {
    pub organization: String,
    pub issuer: String,
}

impl SharedAuthProviderTopology {
    #[must_use]
    pub fn new(organization: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            organization: organization.into(),
            issuer: issuer.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthRuntimeTopology {
    pub github_org: String,
    pub supabase: SharedAuthProviderTopology,
    pub neon: SharedAuthProviderTopology,
    pub server_role: SharedAuthServerRole,
    pub audience: String,
    pub decision_mode: SharedAuthDecisionMode,
}

impl SharedAuthRuntimeTopology {
    pub fn new(
        github_org: impl Into<String>,
        supabase: SharedAuthProviderTopology,
        neon: SharedAuthProviderTopology,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let topology = Self {
            github_org: github_org.into(),
            supabase,
            neon,
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

    pub fn dedicated(
        github_org: impl Into<String>,
        supabase_issuer: impl Into<String>,
        neon_issuer: impl Into<String>,
        server_role: SharedAuthServerRole,
        audience: impl Into<String>,
        decision_mode: SharedAuthDecisionMode,
    ) -> Result<Self, Vec<ValidationIssue>> {
        let github_org = github_org.into();
        Self::new(
            github_org.clone(),
            SharedAuthProviderTopology::new(github_org.clone(), supabase_issuer),
            SharedAuthProviderTopology::new(github_org, neon_issuer),
            server_role,
            audience,
            decision_mode,
        )
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
    pub fn provider_context(&self, provider: SharedAuthProvider) -> SharedAuthProviderContext {
        let topology = match provider {
            SharedAuthProvider::Supabase => &self.supabase,
            SharedAuthProvider::Neon => &self.neon,
        };
        SharedAuthProviderContext {
            provider,
            organization: topology.organization.clone(),
            issuer: topology.issuer.clone(),
            audience: self.audience.clone(),
            data_plane: self.data_plane(),
        }
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
        validate_provider_topology(
            &mut issues,
            SharedAuthProvider::Supabase,
            &self.github_org,
            &self.supabase,
        );
        validate_provider_topology(
            &mut issues,
            SharedAuthProvider::Neon,
            &self.github_org,
            &self.neon,
        );
        if self.supabase.issuer == self.neon.issuer {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/providers",
                "shared_auth_provider_issuers_must_differ",
                "Supabase Auth and Neon Auth must use independently identified issuers",
            ));
        }
        if self.audience.trim().is_empty() {
            issues.push(ValidationIssue::new(
                "/sharedAuthTopology/audience",
                "shared_auth_audience_required",
                "Shared Auth audience must not be empty",
            ));
        }
        if self.server_role.is_admin() && self.decision_mode != SharedAuthDecisionMode::StrictPaired
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

fn validate_provider_topology(
    issues: &mut Vec<ValidationIssue>,
    provider: SharedAuthProvider,
    github_org: &str,
    topology: &SharedAuthProviderTopology,
) {
    if !valid_org_slug(&topology.organization) || topology.organization != github_org {
        issues.push(ValidationIssue::new(
            match provider {
                SharedAuthProvider::Supabase => "/sharedAuthTopology/supabase/organization",
                SharedAuthProvider::Neon => "/sharedAuthTopology/neon/organization",
            },
            "shared_auth_provider_org_mismatch",
            "provider organization must exactly match the GitHub organization",
        ));
    }
    if !valid_https_issuer(&topology.issuer) {
        issues.push(ValidationIssue::new(
            match provider {
                SharedAuthProvider::Supabase => "/sharedAuthTopology/supabase/issuer",
                SharedAuthProvider::Neon => "/sharedAuthTopology/neon/issuer",
            },
            "shared_auth_provider_issuer_invalid",
            "provider issuer must be a non-empty HTTPS URL without whitespace",
        ));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthProviderContext {
    pub provider: SharedAuthProvider,
    pub organization: String,
    pub issuer: String,
    pub audience: String,
    pub data_plane: SharedAuthDataPlane,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthVerifiedPrincipal {
    pub provider: SharedAuthProvider,
    pub subject: String,
    pub tenant_id: String,
    pub session_id: String,
    pub issuer: String,
    pub audience: String,
    pub organization: String,
    pub data_plane: SharedAuthDataPlane,
}

impl SharedAuthVerifiedPrincipal {
    #[must_use]
    pub fn new(
        provider: SharedAuthProvider,
        subject: impl Into<String>,
        tenant_id: impl Into<String>,
        session_id: impl Into<String>,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        organization: impl Into<String>,
        data_plane: SharedAuthDataPlane,
    ) -> Self {
        Self {
            provider,
            subject: subject.into(),
            tenant_id: tenant_id.into(),
            session_id: session_id.into(),
            issuer: issuer.into(),
            audience: audience.into(),
            organization: organization.into(),
            data_plane,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAuthProviderFailureKind {
    Unavailable,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedAuthProviderFailure {
    pub kind: SharedAuthProviderFailureKind,
    pub code: &'static str,
    pub message: String,
}

impl SharedAuthProviderFailure {
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: SharedAuthProviderFailureKind::Unavailable,
            code: "shared_auth_provider_unavailable",
            message: message.into(),
        }
    }

    #[must_use]
    pub fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: SharedAuthProviderFailureKind::Rejected,
            code,
            message: message.into(),
        }
    }
}

pub trait SharedAuthProviderVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
        context: &'a SharedAuthProviderContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
                + Send
                + 'a,
        >,
    >;
}

pub struct SharedAuthReadyStack {
    inner: MiddlewareStack,
    topology: SharedAuthRuntimeTopology,
}

impl SharedAuthReadyStack {
    pub fn new<S, N>(
        config: MiddlewareConfig,
        topology: SharedAuthRuntimeTopology,
        supabase_verifier: S,
        neon_verifier: N,
    ) -> Result<Self, Vec<ValidationIssue>>
    where
        S: SharedAuthProviderVerifier + 'static,
        N: SharedAuthProviderVerifier + 'static,
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

        let verifier = DualProviderAuthVerifier {
            topology: topology.clone(),
            supabase: supabase_verifier,
            neon: neon_verifier,
        };
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

struct DualProviderAuthVerifier<S, N> {
    topology: SharedAuthRuntimeTopology,
    supabase: S,
    neon: N,
}

impl<S, N> AuthVerifier for DualProviderAuthVerifier<S, N>
where
    S: SharedAuthProviderVerifier,
    N: SharedAuthProviderVerifier,
{
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(async move {
            let supabase_context = self.topology.provider_context(SharedAuthProvider::Supabase);
            let neon_context = self.topology.provider_context(SharedAuthProvider::Neon);

            let supabase = checked_provider_outcome(
                SharedAuthProvider::Supabase,
                &supabase_context,
                self.supabase.verify(request, &supabase_context).await,
            )?;
            let neon = checked_provider_outcome(
                SharedAuthProvider::Neon,
                &neon_context,
                self.neon.verify(request, &neon_context).await,
            )?;

            decide_auth(&self.topology, supabase, neon)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckedProviderOutcome {
    Verified(Box<SharedAuthVerifiedPrincipal>),
    Unavailable,
}

fn checked_provider_outcome(
    provider: SharedAuthProvider,
    context: &SharedAuthProviderContext,
    result: Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>,
) -> Result<CheckedProviderOutcome, IntegrationError> {
    match result {
        Ok(principal) => {
            validate_verified_principal(provider, context, &principal)?;
            Ok(CheckedProviderOutcome::Verified(Box::new(principal)))
        }
        Err(failure) if failure.kind == SharedAuthProviderFailureKind::Unavailable => {
            Ok(CheckedProviderOutcome::Unavailable)
        }
        Err(failure) => Err(IntegrationError {
            code: failure.code,
            message: format!("{} authentication proof was rejected", provider.as_str()),
        }),
    }
}

fn validate_verified_principal(
    provider: SharedAuthProvider,
    context: &SharedAuthProviderContext,
    principal: &SharedAuthVerifiedPrincipal,
) -> Result<(), IntegrationError> {
    let complete = [
        principal.subject.as_str(),
        principal.tenant_id.as_str(),
        principal.session_id.as_str(),
        principal.issuer.as_str(),
        principal.audience.as_str(),
        principal.organization.as_str(),
    ]
    .iter()
    .all(|value| !value.trim().is_empty());

    if !complete
        || principal.provider != provider
        || principal.provider != context.provider
        || principal.organization != context.organization
        || principal.issuer != context.issuer
        || principal.audience != context.audience
        || principal.data_plane != context.data_plane
    {
        return Err(IntegrationError {
            code: "shared_auth_provider_evidence_mismatch",
            message: format!(
                "{} authentication proof did not match the configured organization, issuer, audience, or realm",
                provider.as_str()
            ),
        });
    }
    Ok(())
}

fn decide_auth(
    topology: &SharedAuthRuntimeTopology,
    supabase: CheckedProviderOutcome,
    neon: CheckedProviderOutcome,
) -> Result<AuthDecision, IntegrationError> {
    match topology.decision_mode {
        SharedAuthDecisionMode::StrictPaired => match (supabase, neon) {
            (
                CheckedProviderOutcome::Verified(supabase),
                CheckedProviderOutcome::Verified(neon),
            ) => reconcile_verified_pair(topology, *supabase, *neon),
            _ => Err(IntegrationError {
                code: "shared_auth_strict_provider_unavailable",
                message: "strict paired authentication requires verified Supabase and Neon proofs"
                    .into(),
            }),
        },
        SharedAuthDecisionMode::AvailabilityFirst => match (supabase, neon) {
            (
                CheckedProviderOutcome::Verified(supabase),
                CheckedProviderOutcome::Verified(neon),
            ) => reconcile_verified_pair(topology, *supabase, *neon),
            (CheckedProviderOutcome::Verified(principal), CheckedProviderOutcome::Unavailable) => {
                single_provider_decision(topology, *principal, SharedAuthProvider::Neon)
            }
            (CheckedProviderOutcome::Unavailable, CheckedProviderOutcome::Verified(principal)) => {
                single_provider_decision(topology, *principal, SharedAuthProvider::Supabase)
            }
            (CheckedProviderOutcome::Unavailable, CheckedProviderOutcome::Unavailable) => {
                Err(IntegrationError {
                    code: "shared_auth_all_providers_unavailable",
                    message: "no Shared Auth provider could establish a verified principal".into(),
                })
            }
        },
    }
}

fn reconcile_verified_pair(
    topology: &SharedAuthRuntimeTopology,
    supabase: SharedAuthVerifiedPrincipal,
    neon: SharedAuthVerifiedPrincipal,
) -> Result<AuthDecision, IntegrationError> {
    if supabase.subject != neon.subject
        || supabase.tenant_id != neon.tenant_id
        || supabase.session_id != neon.session_id
    {
        return Err(IntegrationError {
            code: "shared_auth_provider_identity_disagreement",
            message: "Supabase and Neon proofs resolved to different canonical identities".into(),
        });
    }

    Ok(auth_decision(topology, supabase, "verified", "verified"))
}

fn single_provider_decision(
    topology: &SharedAuthRuntimeTopology,
    principal: SharedAuthVerifiedPrincipal,
    unavailable: SharedAuthProvider,
) -> Result<AuthDecision, IntegrationError> {
    if topology.server_role.is_admin() {
        return Err(IntegrationError {
            code: "shared_auth_admin_requires_strict_paired",
            message: "admin authentication cannot use a single-provider outage path".into(),
        });
    }

    let (supabase_status, neon_status) = match unavailable {
        SharedAuthProvider::Supabase => ("unavailable", "verified"),
        SharedAuthProvider::Neon => ("verified", "unavailable"),
    };
    Ok(auth_decision(
        topology,
        principal,
        supabase_status,
        neon_status,
    ))
}

fn auth_decision(
    topology: &SharedAuthRuntimeTopology,
    principal: SharedAuthVerifiedPrincipal,
    supabase_status: &str,
    neon_status: &str,
) -> AuthDecision {
    let mut claims = BTreeMap::new();
    claims.insert(
        "shared_auth.data_plane".into(),
        topology.data_plane().as_str().into(),
    );
    claims.insert("shared_auth.supabase".into(), supabase_status.to_owned());
    claims.insert("shared_auth.neon".into(), neon_status.to_owned());

    AuthDecision {
        user_id: Some(principal.subject),
        tenant_id: Some(principal.tenant_id),
        claims,
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

fn valid_https_issuer(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() > "https://".len()
        && !value.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeEnvironment, default_config};

    #[derive(Clone)]
    struct StaticProviderVerifier {
        result: Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>,
    }

    impl StaticProviderVerifier {
        fn verified(principal: SharedAuthVerifiedPrincipal) -> Self {
            Self {
                result: Ok(principal),
            }
        }

        fn unavailable() -> Self {
            Self {
                result: Err(SharedAuthProviderFailure::unavailable(
                    "simulated provider outage",
                )),
            }
        }

        fn rejected() -> Self {
            Self {
                result: Err(SharedAuthProviderFailure::rejected(
                    "shared_auth_provider_rejected",
                    "simulated provider rejection",
                )),
            }
        }
    }

    impl SharedAuthProviderVerifier for StaticProviderVerifier {
        fn verify<'a>(
            &'a self,
            _request: &'a RequestMetadata,
            _context: &'a SharedAuthProviderContext,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
                    + Send
                    + 'a,
            >,
        > {
            let result = self.result.clone();
            Box::pin(async move { result })
        }
    }

    fn topology(
        role: SharedAuthServerRole,
        decision_mode: SharedAuthDecisionMode,
    ) -> SharedAuthRuntimeTopology {
        SharedAuthRuntimeTopology::dedicated(
            "messaging-intel",
            "https://supabase.example.invalid/auth/v1",
            "https://neon.example.invalid/auth",
            role,
            if role.is_admin() {
                "msgint-admin"
            } else {
                "msgint"
            },
            decision_mode,
        )
        .expect("valid dedicated topology")
    }

    fn principal(
        provider: SharedAuthProvider,
        role: SharedAuthServerRole,
    ) -> SharedAuthVerifiedPrincipal {
        let context = topology(
            role,
            if role.is_admin() {
                SharedAuthDecisionMode::StrictPaired
            } else {
                SharedAuthDecisionMode::AvailabilityFirst
            },
        )
        .provider_context(provider);
        SharedAuthVerifiedPrincipal::new(
            provider,
            "user-123",
            "tenant-456",
            "session-789",
            context.issuer,
            context.audience,
            context.organization,
            context.data_plane,
        )
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

    fn request() -> RequestMetadata {
        RequestMetadata {
            method: "GET".into(),
            path: "/protected".into(),
            headers: BTreeMap::new(),
            remote_ip: None,
            content_length: None,
            transport_secure: true,
        }
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
    fn rejects_shared_or_cross_org_provider_ownership() {
        let issues = SharedAuthRuntimeTopology::new(
            "messaging-intel",
            SharedAuthProviderTopology::new(
                "oresoftware",
                "https://supabase.example.invalid/auth/v1",
            ),
            SharedAuthProviderTopology::new("messaging-intel", "https://neon.example.invalid/auth"),
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
    fn rejects_duplicate_or_insecure_provider_issuers() {
        let issues = SharedAuthRuntimeTopology::dedicated(
            "messaging-intel",
            "http://provider.example.invalid",
            "http://provider.example.invalid",
            SharedAuthServerRole::ApiServer,
            "msgint",
            SharedAuthDecisionMode::AvailabilityFirst,
        )
        .expect_err("insecure duplicate issuers must fail closed");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_provider_issuer_invalid")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_provider_issuers_must_differ")
        );
    }

    #[test]
    fn rejects_availability_first_for_admin_servers() {
        let issues = SharedAuthRuntimeTopology::dedicated(
            "messaging-intel",
            "https://supabase.example.invalid/auth/v1",
            "https://neon.example.invalid/auth",
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
            topology(
                SharedAuthServerRole::WebServer,
                SharedAuthDecisionMode::AvailabilityFirst,
            ),
            StaticProviderVerifier::verified(principal(
                SharedAuthProvider::Supabase,
                SharedAuthServerRole::WebServer,
            )),
            StaticProviderVerifier::verified(principal(
                SharedAuthProvider::Neon,
                SharedAuthServerRole::WebServer,
            )),
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
            topology(
                SharedAuthServerRole::WebServer,
                SharedAuthDecisionMode::AvailabilityFirst,
            ),
            StaticProviderVerifier::verified(principal(
                SharedAuthProvider::Supabase,
                SharedAuthServerRole::WebServer,
            )),
            StaticProviderVerifier::verified(principal(
                SharedAuthProvider::Neon,
                SharedAuthServerRole::WebServer,
            )),
        )
        .err()
        .expect("audience mismatch must be rejected");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "shared_auth_audience_mismatch")
        );
    }

    #[tokio::test]
    async fn strict_pair_accepts_matching_provider_proofs() {
        let role = SharedAuthServerRole::AdminApiServer;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint-admin"),
            topology(role, SharedAuthDecisionMode::StrictPaired),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Supabase, role)),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Neon, role)),
        )
        .expect("matching paired proofs should construct");

        let active = ready
            .begin(request())
            .await
            .expect("matching paired proofs should authenticate");
        assert_eq!(active.context.user_id.as_deref(), Some("user-123"));
        assert_eq!(active.context.tenant_id.as_deref(), Some("tenant-456"));
    }

    #[tokio::test]
    async fn strict_pair_rejects_single_provider_outage() {
        let role = SharedAuthServerRole::AdminApiServer;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint-admin"),
            topology(role, SharedAuthDecisionMode::StrictPaired),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Supabase, role)),
            StaticProviderVerifier::unavailable(),
        )
        .expect("valid strict topology should construct");

        let error = ready
            .begin(request())
            .await
            .err()
            .expect("strict paired proof must reject a provider outage");
        assert_eq!(error.code, "shared_auth_strict_provider_unavailable");
    }

    #[tokio::test]
    async fn availability_first_accepts_one_explicit_outage_for_customer_role() {
        let role = SharedAuthServerRole::ApiServer;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(role, SharedAuthDecisionMode::AvailabilityFirst),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Supabase, role)),
            StaticProviderVerifier::unavailable(),
        )
        .expect("valid customer topology should construct");

        let active = ready
            .begin(request())
            .await
            .expect("one verified provider plus explicit outage is admissible");
        assert_eq!(active.context.user_id.as_deref(), Some("user-123"));
    }

    #[tokio::test]
    async fn availability_first_rejects_provider_rejection() {
        let role = SharedAuthServerRole::ApiServer;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(role, SharedAuthDecisionMode::AvailabilityFirst),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Supabase, role)),
            StaticProviderVerifier::rejected(),
        )
        .expect("valid customer topology should construct");

        let error = ready
            .begin(request())
            .await
            .err()
            .expect("provider rejection is not an availability event");
        assert_eq!(error.code, "shared_auth_provider_rejected");
    }

    #[tokio::test]
    async fn rejects_provider_identity_disagreement() {
        let role = SharedAuthServerRole::ApiServer;
        let mut neon = principal(SharedAuthProvider::Neon, role);
        neon.subject = "different-user".into();
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(role, SharedAuthDecisionMode::StrictPaired),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Supabase, role)),
            StaticProviderVerifier::verified(neon),
        )
        .expect("valid paired topology should construct");

        let error = ready
            .begin(request())
            .await
            .err()
            .expect("provider identity disagreement must fail closed");
        assert_eq!(error.code, "shared_auth_provider_identity_disagreement");
    }

    #[tokio::test]
    async fn rejects_wrong_provider_realm_or_issuer() {
        let role = SharedAuthServerRole::ApiServer;
        let mut supabase = principal(SharedAuthProvider::Supabase, role);
        supabase.data_plane = SharedAuthDataPlane::AdminAuth;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(role, SharedAuthDecisionMode::StrictPaired),
            StaticProviderVerifier::verified(supabase),
            StaticProviderVerifier::verified(principal(SharedAuthProvider::Neon, role)),
        )
        .expect("valid paired topology should construct");

        let error = ready
            .begin(request())
            .await
            .err()
            .expect("wrong provider realm must fail closed");
        assert_eq!(error.code, "shared_auth_provider_evidence_mismatch");
    }

    #[tokio::test]
    async fn rejects_both_providers_unavailable() {
        let role = SharedAuthServerRole::WebServer;
        let ready = SharedAuthReadyStack::new(
            protected_config("msgint"),
            topology(role, SharedAuthDecisionMode::AvailabilityFirst),
            StaticProviderVerifier::unavailable(),
            StaticProviderVerifier::unavailable(),
        )
        .expect("valid customer topology should construct");

        let error = ready
            .begin(request())
            .await
            .err()
            .expect("two unavailable providers cannot authenticate");
        assert_eq!(error.code, "shared_auth_all_providers_unavailable");
    }
}
