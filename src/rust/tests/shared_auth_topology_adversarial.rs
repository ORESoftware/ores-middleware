use ores_middleware::{
    SharedAuthDataPlane, SharedAuthDecisionMode, SharedAuthProvider, SharedAuthProviderTopology,
    SharedAuthRuntimeTopology, SharedAuthServerRole,
};

#[test]
fn provider_topology_reports_cross_org_and_insecure_issuer_failures_in_stable_order() {
    let issues = SharedAuthRuntimeTopology::new(
        "messaging-intel",
        SharedAuthProviderTopology::new("other-supabase", "http://supabase.example.invalid"),
        SharedAuthProviderTopology::new("other-neon", "ftp://neon.example.invalid"),
        SharedAuthServerRole::ApiServer,
        "msgint",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect_err("cross-org and insecure provider topology must fail closed");

    let diagnostics = issues
        .iter()
        .map(|issue| (issue.path.as_str(), issue.code.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        diagnostics,
        vec![
            (
                "/sharedAuthTopology/supabase/organization",
                "shared_auth_provider_org_mismatch",
            ),
            (
                "/sharedAuthTopology/supabase/issuer",
                "shared_auth_provider_issuer_invalid",
            ),
            (
                "/sharedAuthTopology/neon/organization",
                "shared_auth_provider_org_mismatch",
            ),
            (
                "/sharedAuthTopology/neon/issuer",
                "shared_auth_provider_issuer_invalid",
            ),
        ]
    );
}

#[test]
fn dedicated_topology_binds_both_provider_owners_to_the_github_org() {
    let topology = SharedAuthRuntimeTopology::dedicated(
        "messaging-intel",
        "https://supabase.example.invalid/auth/v1",
        "https://neon.example.invalid/auth",
        SharedAuthServerRole::ApiServer,
        "msgint",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect("dedicated topology should be valid");

    assert_eq!(topology.github_org, "messaging-intel");
    assert_eq!(topology.supabase.organization, topology.github_org);
    assert_eq!(topology.neon.organization, topology.github_org);
    assert_ne!(topology.supabase.issuer, topology.neon.issuer);
}

#[test]
fn invalid_github_org_does_not_mask_provider_specific_diagnostics() {
    let issues = SharedAuthRuntimeTopology::new(
        "messaging_intel",
        SharedAuthProviderTopology::new("different-supabase", "http://supabase.invalid"),
        SharedAuthProviderTopology::new("different-neon", "http://neon.invalid"),
        SharedAuthServerRole::ApiServer,
        "msgint",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect_err("invalid owner and provider evidence must all be reported");

    assert_eq!(
        issues.first().map(|issue| issue.code.as_str()),
        Some("invalid_org_slug")
    );
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.code == "shared_auth_provider_org_mismatch")
            .count(),
        2
    );
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.code == "shared_auth_provider_issuer_invalid")
            .count(),
        2
    );
}

#[test]
fn duplicate_provider_issuer_is_a_distinct_topology_violation() {
    let issues = SharedAuthRuntimeTopology::dedicated(
        "messaging-intel",
        "https://auth.example.invalid",
        "https://auth.example.invalid",
        SharedAuthServerRole::ApiServer,
        "msgint",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect_err("provider issuers must be independently identified");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/sharedAuthTopology/providers");
    assert_eq!(
        issues[0].code,
        "shared_auth_provider_issuers_must_differ"
    );
}

#[test]
fn admin_availability_first_mode_fails_without_spurious_provider_errors() {
    let issues = SharedAuthRuntimeTopology::dedicated(
        "messaging-intel",
        "https://supabase.example.invalid/auth/v1",
        "https://neon.example.invalid/auth",
        SharedAuthServerRole::AdminApiServer,
        "msgint-admin",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect_err("admin services must require strict paired authentication");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "/sharedAuthTopology/decisionMode");
    assert_eq!(
        issues[0].code,
        "shared_auth_admin_requires_strict_paired"
    );
}

#[test]
fn provider_context_keeps_provider_identity_and_customer_realm_isolated() {
    let topology = SharedAuthRuntimeTopology::dedicated(
        "messaging-intel",
        "https://supabase.example.invalid/auth/v1",
        "https://neon.example.invalid/auth",
        SharedAuthServerRole::WebServer,
        "msgint",
        SharedAuthDecisionMode::AvailabilityFirst,
    )
    .expect("valid customer topology");

    let supabase = topology.provider_context(SharedAuthProvider::Supabase);
    let neon = topology.provider_context(SharedAuthProvider::Neon);

    assert_eq!(supabase.provider, SharedAuthProvider::Supabase);
    assert_eq!(neon.provider, SharedAuthProvider::Neon);
    assert_eq!(supabase.organization, "messaging-intel");
    assert_eq!(neon.organization, "messaging-intel");
    assert_eq!(supabase.audience, "msgint");
    assert_eq!(neon.audience, "msgint");
    assert_eq!(supabase.data_plane, SharedAuthDataPlane::CustomerAuth);
    assert_eq!(neon.data_plane, SharedAuthDataPlane::CustomerAuth);
    assert_ne!(supabase.issuer, neon.issuer);
}
