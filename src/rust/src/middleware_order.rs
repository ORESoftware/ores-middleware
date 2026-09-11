use serde::{Deserialize, Serialize};

use crate::RateLimitFailureMode;

/// Observable request/response execution order.
///
/// This is an execution trace, not the lexical nesting order of framework
/// layers. Response stages occur after the handler in the order listed here.
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiddlewareStage {
    PanicBoundary,
    Deadline,
    RequestId,
    TraceContext,
    TrustedProxy,
    TransportSecurity,
    PayloadLimit,
    AnonymousFloodGuard,
    Authentication,
    PrincipalRateLimit,
    Authorization,
    Idempotency,
    Handler,
    ResponseCompression,
    SecurityHeaders,
    TelemetryFinalize,
}

impl MiddlewareStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PanicBoundary => "panic-boundary",
            Self::Deadline => "deadline",
            Self::RequestId => "request-id",
            Self::TraceContext => "trace-context",
            Self::TrustedProxy => "trusted-proxy",
            Self::TransportSecurity => "transport-security",
            Self::PayloadLimit => "payload-limit",
            Self::AnonymousFloodGuard => "anonymous-flood-guard",
            Self::Authentication => "authentication",
            Self::PrincipalRateLimit => "principal-rate-limit",
            Self::Authorization => "authorization",
            Self::Idempotency => "idempotency",
            Self::Handler => "handler",
            Self::ResponseCompression => "response-compression",
            Self::SecurityHeaders => "security-headers",
            Self::TelemetryFinalize => "telemetry-finalize",
        }
    }
}

pub const DEFAULT_MIDDLEWARE_ORDER: [MiddlewareStage; 16] = [
    MiddlewareStage::PanicBoundary,
    MiddlewareStage::Deadline,
    MiddlewareStage::RequestId,
    MiddlewareStage::TraceContext,
    MiddlewareStage::TrustedProxy,
    MiddlewareStage::TransportSecurity,
    MiddlewareStage::PayloadLimit,
    MiddlewareStage::AnonymousFloodGuard,
    MiddlewareStage::Authentication,
    MiddlewareStage::PrincipalRateLimit,
    MiddlewareStage::Authorization,
    MiddlewareStage::Idempotency,
    MiddlewareStage::Handler,
    MiddlewareStage::ResponseCompression,
    MiddlewareStage::SecurityHeaders,
    MiddlewareStage::TelemetryFinalize,
];

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitConsistency {
    /// One atomic coordinator owns every admission decision.
    Strict,
    /// Local decisions are permitted only with a documented overshoot bound.
    Bounded,
    /// Telemetry or coarse protection; never a globally authoritative quota.
    Advisory,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationClass {
    HealthRead,
    PublicRead,
    AuthAttempt,
    AuthRecovery,
    Mutation,
    PaymentOrLedgerWrite,
    JobAdmission,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RateLimitPosture {
    pub consistency: RateLimitConsistency,
    pub failure_mode: RateLimitFailureMode,
    pub edge_may_deny: bool,
    pub coordinator_required: bool,
}

pub const fn rate_limit_posture(class: OperationClass) -> RateLimitPosture {
    match class {
        OperationClass::HealthRead => RateLimitPosture {
            consistency: RateLimitConsistency::Advisory,
            failure_mode: RateLimitFailureMode::FailOpen,
            edge_may_deny: false,
            coordinator_required: false,
        },
        OperationClass::PublicRead => RateLimitPosture {
            consistency: RateLimitConsistency::Bounded,
            failure_mode: RateLimitFailureMode::LocalOnly,
            edge_may_deny: true,
            coordinator_required: false,
        },
        OperationClass::AuthAttempt => RateLimitPosture {
            consistency: RateLimitConsistency::Strict,
            failure_mode: RateLimitFailureMode::FailClosed,
            edge_may_deny: true,
            coordinator_required: true,
        },
        OperationClass::AuthRecovery
        | OperationClass::Mutation
        | OperationClass::PaymentOrLedgerWrite
        | OperationClass::JobAdmission => RateLimitPosture {
            consistency: RateLimitConsistency::Strict,
            failure_mode: RateLimitFailureMode::FailClosed,
            edge_may_deny: false,
            coordinator_required: true,
        },
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OrderViolation {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct OrderRule {
    first: MiddlewareStage,
    second: MiddlewareStage,
    code: &'static str,
}

const ORDER_RULES: [OrderRule; 11] = [
    OrderRule {
        first: MiddlewareStage::RequestId,
        second: MiddlewareStage::TraceContext,
        code: "request-id-before-trace",
    },
    OrderRule {
        first: MiddlewareStage::TraceContext,
        second: MiddlewareStage::TrustedProxy,
        code: "trace-before-proxy-policy",
    },
    OrderRule {
        first: MiddlewareStage::TrustedProxy,
        second: MiddlewareStage::TransportSecurity,
        code: "proxy-before-transport-policy",
    },
    OrderRule {
        first: MiddlewareStage::TrustedProxy,
        second: MiddlewareStage::AnonymousFloodGuard,
        code: "trusted-proxy-before-anonymous-identity",
    },
    OrderRule {
        first: MiddlewareStage::PayloadLimit,
        second: MiddlewareStage::Authentication,
        code: "payload-limit-before-authentication",
    },
    OrderRule {
        first: MiddlewareStage::AnonymousFloodGuard,
        second: MiddlewareStage::Authentication,
        code: "anonymous-guard-before-authentication",
    },
    OrderRule {
        first: MiddlewareStage::Authentication,
        second: MiddlewareStage::PrincipalRateLimit,
        code: "authentication-before-principal-rate-limit",
    },
    OrderRule {
        first: MiddlewareStage::PrincipalRateLimit,
        second: MiddlewareStage::Authorization,
        code: "principal-rate-limit-before-authorization",
    },
    OrderRule {
        first: MiddlewareStage::Authorization,
        second: MiddlewareStage::Handler,
        code: "authorization-before-handler",
    },
    OrderRule {
        first: MiddlewareStage::Handler,
        second: MiddlewareStage::ResponseCompression,
        code: "handler-before-response-compression",
    },
    OrderRule {
        first: MiddlewareStage::SecurityHeaders,
        second: MiddlewareStage::TelemetryFinalize,
        code: "security-headers-before-telemetry-finalize",
    },
];

/// Validates the ordering invariants shared by framework adapters and service
/// composition roots. Every declared stage must occur exactly once, and the
/// complete request/response execution sequence must match the reviewed order.
///
/// Rule helpers return new violation values instead of mutating a caller-owned
/// accumulator. That keeps the validation boundary referentially transparent:
/// callers provide stages and receive a newly constructed result vector.
pub fn validate_middleware_order(stages: &[MiddlewareStage]) -> Vec<OrderViolation> {
    let duplicates = stages.iter().enumerate().filter_map(|(index, stage)| {
        stages[..index].contains(stage).then_some(OrderViolation {
            code: "duplicate-stage",
            message: "middleware stages must occur exactly once",
        })
    });

    let missing = DEFAULT_MIDDLEWARE_ORDER
        .into_iter()
        .filter(|expected| !stages.contains(expected))
        .map(|_| OrderViolation {
            code: "missing-stage",
            message: "every reviewed middleware stage must be present exactly once",
        });

    let first = require_first(
        stages,
        MiddlewareStage::PanicBoundary,
        "panic-boundary-must-be-first",
    )
    .into_iter();

    let declared_rules = ORDER_RULES
        .into_iter()
        .filter_map(|rule| require_before(stages, rule.first, rule.second, rule.code));

    let reviewed_order = DEFAULT_MIDDLEWARE_ORDER.windows(2).filter_map(|pair| {
        require_before(
            stages,
            pair[0],
            pair[1],
            "reviewed-stage-order",
        )
    });

    duplicates
        .chain(missing)
        .chain(first)
        .chain(declared_rules)
        .chain(reviewed_order)
        .collect()
}

fn require_first(
    stages: &[MiddlewareStage],
    expected: MiddlewareStage,
    code: &'static str,
) -> Option<OrderViolation> {
    (stages.first().copied() != Some(expected)).then_some(OrderViolation {
        code,
        message: "the outer panic boundary must observe every downstream failure",
    })
}

fn require_before(
    stages: &[MiddlewareStage],
    first: MiddlewareStage,
    second: MiddlewareStage,
    code: &'static str,
) -> Option<OrderViolation> {
    let first_index = stages.iter().position(|stage| *stage == first);
    let second_index = stages.iter().position(|stage| *stage == second);
    (!matches!((first_index, second_index), (Some(left), Some(right)) if left < right)).then_some(
        OrderViolation {
            code,
            message: "required middleware stages are missing or out of order",
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_order_satisfies_every_invariant() {
        assert!(validate_middleware_order(&DEFAULT_MIDDLEWARE_ORDER).is_empty());
    }

    #[test]
    fn untrusted_identity_cannot_precede_proxy_validation() {
        let mut stages = DEFAULT_MIDDLEWARE_ORDER;
        stages.swap(4, 7);
        assert!(
            validate_middleware_order(&stages)
                .iter()
                .any(|issue| issue.code == "trusted-proxy-before-anonymous-identity")
        );
    }

    #[test]
    fn principal_limit_cannot_run_before_authentication() {
        let mut stages = DEFAULT_MIDDLEWARE_ORDER;
        stages.swap(8, 9);
        assert!(
            validate_middleware_order(&stages)
                .iter()
                .any(|issue| issue.code == "authentication-before-principal-rate-limit")
        );
    }

    #[test]
    fn duplicate_stage_is_rejected() {
        let mut stages = DEFAULT_MIDDLEWARE_ORDER;
        stages[15] = MiddlewareStage::Handler;
        assert!(
            validate_middleware_order(&stages)
                .iter()
                .any(|issue| issue.code == "duplicate-stage")
        );
    }

    #[test]
    fn omitted_stage_is_rejected_even_when_remaining_pairs_are_ordered() {
        let stages = DEFAULT_MIDDLEWARE_ORDER
            .into_iter()
            .filter(|stage| *stage != MiddlewareStage::Deadline)
            .collect::<Vec<_>>();
        assert!(
            validate_middleware_order(&stages)
                .iter()
                .any(|issue| issue.code == "missing-stage")
        );
    }

    #[test]
    fn every_adjacent_stage_boundary_is_enforced() {
        let mut stages = DEFAULT_MIDDLEWARE_ORDER;
        stages.swap(11, 12);
        assert!(
            validate_middleware_order(&stages)
                .iter()
                .any(|issue| issue.code == "reviewed-stage-order")
        );
    }

    #[test]
    fn security_sensitive_classes_are_strict_and_fail_closed() {
        for class in [
            OperationClass::AuthRecovery,
            OperationClass::Mutation,
            OperationClass::PaymentOrLedgerWrite,
            OperationClass::JobAdmission,
        ] {
            let posture = rate_limit_posture(class);
            assert_eq!(posture.consistency, RateLimitConsistency::Strict);
            assert_eq!(posture.failure_mode, RateLimitFailureMode::FailClosed);
            assert!(posture.coordinator_required);
            assert!(!posture.edge_may_deny);
        }
    }
}
