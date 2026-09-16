//! Static-dispatch composition for paired Supabase + Neon Shared Auth.
//!
//! This module mirrors the fail-closed identity reconciliation policy used by
//! `SharedAuthReadyStack` without routing the provider pair through the legacy
//! `MiddlewareStack` / `Arc<dyn AuthVerifier>` boundary.

use std::collections::BTreeMap;

use crate::{
    AuthDecision, IntegrationError, RequestMetadata, StaticAuthVerifier,
    StaticSharedAuthProviderVerifier,
};
use crate::shared_auth::{
    SharedAuthDecisionMode, SharedAuthProvider, SharedAuthProviderContext,
    SharedAuthProviderFailure, SharedAuthProviderFailureKind, SharedAuthRuntimeTopology,
    SharedAuthVerifiedPrincipal,
};

/// Statically typed paired Shared Auth verifier.
///
/// `S` and `N` remain the concrete Supabase and Neon adapter types. Their
/// futures are also concrete through [`StaticSharedAuthProviderVerifier`]. Use
/// this verifier with `AuthStage` or the standalone Axum auth primitive when a
/// service wants paired Shared Auth without entering the legacy dynamic stack.
pub struct PairedSharedAuthVerifier<S, N> {
    topology: SharedAuthRuntimeTopology,
    supabase: S,
    neon: N,
}

impl<S, N> PairedSharedAuthVerifier<S, N> {
    #[must_use]
    pub const fn new(topology: SharedAuthRuntimeTopology, supabase: S, neon: N) -> Self {
        Self {
            topology,
            supabase,
            neon,
        }
    }

    #[must_use]
    pub const fn topology(&self) -> &SharedAuthRuntimeTopology {
        &self.topology
    }

    #[must_use]
    pub const fn supabase(&self) -> &S {
        &self.supabase
    }

    #[must_use]
    pub const fn neon(&self) -> &N {
        &self.neon
    }

    #[must_use]
    pub fn into_parts(self) -> (SharedAuthRuntimeTopology, S, N) {
        (self.topology, self.supabase, self.neon)
    }
}

impl<S, N> StaticAuthVerifier for PairedSharedAuthVerifier<S, N>
where
    S: StaticSharedAuthProviderVerifier + 'static,
    N: StaticSharedAuthProviderVerifier + 'static,
{
    fn verify_owned(
        &self,
        request: RequestMetadata,
    ) -> impl std::future::Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static {
        let topology = self.topology.clone();
        let supabase_context = topology.provider_context(SharedAuthProvider::Supabase);
        let neon_context = topology.provider_context(SharedAuthProvider::Neon);
        let supabase_future = self
            .supabase
            .verify_owned(request.clone(), supabase_context.clone());
        let neon_future = self
            .neon
            .verify_owned(request, neon_context.clone());

        async move {
            let supabase = checked_provider_outcome(
                SharedAuthProvider::Supabase,
                &supabase_context,
                supabase_future.await,
            )?;
            let neon = checked_provider_outcome(
                SharedAuthProvider::Neon,
                &neon_context,
                neon_future.await,
            )?;
            decide_auth(&topology, supabase, neon)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckedProviderOutcome {
    Verified(SharedAuthVerifiedPrincipal),
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
            Ok(CheckedProviderOutcome::Verified(principal))
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
            ) => reconcile_verified_pair(topology, supabase, neon),
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
            ) => reconcile_verified_pair(topology, supabase, neon),
            (CheckedProviderOutcome::Verified(principal), CheckedProviderOutcome::Unavailable) => {
                single_provider_decision(topology, principal, SharedAuthProvider::Neon)
            }
            (CheckedProviderOutcome::Unavailable, CheckedProviderOutcome::Verified(principal)) => {
                single_provider_decision(topology, principal, SharedAuthProvider::Supabase)
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
    let claims: BTreeMap<String, String> = [
        (
            "shared_auth.data_plane",
            topology.data_plane().as_str().to_owned(),
        ),
        ("shared_auth.supabase", supabase_status.to_owned()),
        ("shared_auth.neon", neon_status.to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();

    AuthDecision {
        user_id: Some(principal.subject),
        tenant_id: Some(principal.tenant_id),
        claims,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        AuthStage, SharedAuthDataPlane, SharedAuthServerRole, shared_auth_provider_fn,
    };

    fn request(subject: &str) -> RequestMetadata {
        RequestMetadata {
            method: "GET".into(),
            path: "/protected".into(),
            headers: BTreeMap::from([("authorization".into(), subject.into())]),
            remote_ip: Some("127.0.0.1".into()),
            content_length: None,
            transport_secure: true,
        }
    }

    fn topology(mode: SharedAuthDecisionMode) -> SharedAuthRuntimeTopology {
        SharedAuthRuntimeTopology::dedicated(
            "example-org",
            "https://supabase.example.test",
            "https://neon.example.test",
            SharedAuthServerRole::WebServer,
            "example-api",
            mode,
        )
        .expect("valid topology")
    }

    fn provider() -> impl StaticSharedAuthProviderVerifier {
        shared_auth_provider_fn(
            |request: RequestMetadata, context: SharedAuthProviderContext| async move {
                let subject = request
                    .headers
                    .get("authorization")
                    .cloned()
                    .expect("test subject");
                Ok(SharedAuthVerifiedPrincipal::new(
                    context.provider,
                    subject,
                    "tenant-1",
                    "session-1",
                    context.issuer,
                    context.audience,
                    context.organization,
                    SharedAuthDataPlane::CustomerAuth,
                ))
            },
        )
    }

    #[tokio::test]
    async fn strict_pair_verifies_without_dynamic_auth_boundary() {
        let verifier = PairedSharedAuthVerifier::new(
            topology(SharedAuthDecisionMode::StrictPaired),
            provider(),
            provider(),
        );

        let decision = verifier.verify_owned(request("alice")).await.unwrap();
        assert_eq!(decision.user_id.as_deref(), Some("alice"));
        assert_eq!(decision.tenant_id.as_deref(), Some("tenant-1"));
        assert_eq!(
            decision.claims.get("shared_auth.supabase").map(String::as_str),
            Some("verified")
        );
        assert_eq!(
            decision.claims.get("shared_auth.neon").map(String::as_str),
            Some("verified")
        );
    }

    #[tokio::test]
    async fn strict_pair_rejects_identity_disagreement() {
        let supabase = provider();
        let neon = shared_auth_provider_fn(
            |_request: RequestMetadata, context: SharedAuthProviderContext| async move {
                Ok(SharedAuthVerifiedPrincipal::new(
                    context.provider,
                    "mallory",
                    "tenant-1",
                    "session-1",
                    context.issuer,
                    context.audience,
                    context.organization,
                    context.data_plane,
                ))
            },
        );
        let verifier = PairedSharedAuthVerifier::new(
            topology(SharedAuthDecisionMode::StrictPaired),
            supabase,
            neon,
        );

        let error = verifier.verify_owned(request("alice")).await.unwrap_err();
        assert_eq!(error.code, "shared_auth_provider_identity_disagreement");
    }

    #[tokio::test]
    async fn availability_first_allows_one_verified_provider_for_customer_plane() {
        let supabase = provider();
        let neon = shared_auth_provider_fn(
            |_request: RequestMetadata, _context: SharedAuthProviderContext| async move {
                Err(SharedAuthProviderFailure::unavailable("test outage"))
            },
        );
        let verifier = PairedSharedAuthVerifier::new(
            topology(SharedAuthDecisionMode::AvailabilityFirst),
            supabase,
            neon,
        );

        let decision = verifier.verify_owned(request("alice")).await.unwrap();
        assert_eq!(decision.user_id.as_deref(), Some("alice"));
        assert_eq!(
            decision.claims.get("shared_auth.neon").map(String::as_str),
            Some("unavailable")
        );
    }

    #[test]
    fn static_pair_can_feed_auth_stage_without_type_erasure() {
        let pair = PairedSharedAuthVerifier::new(
            topology(SharedAuthDecisionMode::StrictPaired),
            provider(),
            provider(),
        );

        let stage = AuthStage::from_provider("shared-auth", pair);
        assert_eq!(stage.name(), "shared-auth");
    }
}
