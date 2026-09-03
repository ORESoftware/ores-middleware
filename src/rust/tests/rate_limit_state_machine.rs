use ores_middleware::{
    RateLimitDecisionKind, RateLimitFailureMode, RateLimitLayer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendOutcome {
    Allow,
    Deny,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalOutcome {
    Allow,
    Deny,
    Unavailable,
}

fn resolve(
    layer: RateLimitLayer,
    failure_mode: RateLimitFailureMode,
    primary: BackendOutcome,
    local: LocalOutcome,
) -> RateLimitDecisionKind {
    match primary {
        BackendOutcome::Allow => RateLimitDecisionKind::Allowed,
        BackendOutcome::Deny => RateLimitDecisionKind::Denied,
        BackendOutcome::Unavailable => match (layer, failure_mode) {
            // Authorization is a security boundary: an unavailable primary
            // never becomes an allow through fail-open or local fallback.
            (RateLimitLayer::Authorization, _) => RateLimitDecisionKind::DegradedDenied,
            (_, RateLimitFailureMode::FailOpen) => RateLimitDecisionKind::DegradedAllowed,
            (_, RateLimitFailureMode::FailClosed) => RateLimitDecisionKind::DegradedDenied,
            (_, RateLimitFailureMode::LocalOnly) => match local {
                LocalOutcome::Allow => RateLimitDecisionKind::DegradedAllowed,
                LocalOutcome::Deny | LocalOutcome::Unavailable => {
                    RateLimitDecisionKind::DegradedDenied
                }
            },
        },
    }
}

#[test]
fn exhaustive_failure_state_model_preserves_safety_invariants() {
    let layers = [
        RateLimitLayer::CloudflareEdge,
        RateLimitLayer::KubernetesIngress,
        RateLimitLayer::ServiceMesh,
        RateLimitLayer::Application,
        RateLimitLayer::Authorization,
    ];
    let modes = [
        RateLimitFailureMode::FailOpen,
        RateLimitFailureMode::FailClosed,
        RateLimitFailureMode::LocalOnly,
    ];
    let primary_outcomes = [
        BackendOutcome::Allow,
        BackendOutcome::Deny,
        BackendOutcome::Unavailable,
    ];
    let local_outcomes = [
        LocalOutcome::Allow,
        LocalOutcome::Deny,
        LocalOutcome::Unavailable,
    ];

    let mut explored = 0;
    for layer in layers {
        for mode in modes {
            for primary in primary_outcomes {
                for local in local_outcomes {
                    explored += 1;
                    let decision = resolve(layer, mode, primary, local);

                    if matches!(primary, BackendOutcome::Deny) {
                        assert_eq!(decision, RateLimitDecisionKind::Denied);
                    }
                    if matches!(primary, BackendOutcome::Allow) {
                        assert_eq!(decision, RateLimitDecisionKind::Allowed);
                    }
                    if matches!(layer, RateLimitLayer::Authorization)
                        && matches!(primary, BackendOutcome::Unavailable)
                    {
                        assert!(!decision.is_allowed());
                    }
                    if matches!(mode, RateLimitFailureMode::FailClosed)
                        && matches!(primary, BackendOutcome::Unavailable)
                    {
                        assert!(!decision.is_allowed());
                    }
                    if matches!(mode, RateLimitFailureMode::LocalOnly)
                        && matches!(primary, BackendOutcome::Unavailable)
                        && !matches!(local, LocalOutcome::Allow)
                    {
                        assert!(!decision.is_allowed());
                    }
                }
            }
        }
    }

    assert_eq!(explored, 5 * 3 * 3 * 3);
}

#[test]
fn every_enum_variant_is_exercised_by_the_model() {
    assert_eq!(RateLimitLayer::CloudflareEdge.as_str(), "cloudflare-edge");
    assert_eq!(
        RateLimitLayer::KubernetesIngress.as_str(),
        "kubernetes-ingress"
    );
    assert_eq!(RateLimitLayer::ServiceMesh.as_str(), "service-mesh");
    assert_eq!(RateLimitLayer::Application.as_str(), "application");
    assert_eq!(RateLimitLayer::Authorization.as_str(), "authorization");

    assert_eq!(RateLimitDecisionKind::Allowed.as_str(), "allowed");
    assert_eq!(RateLimitDecisionKind::Denied.as_str(), "denied");
    assert_eq!(
        RateLimitDecisionKind::DegradedAllowed.as_str(),
        "degraded-allowed"
    );
    assert_eq!(
        RateLimitDecisionKind::DegradedDenied.as_str(),
        "degraded-denied"
    );
}
