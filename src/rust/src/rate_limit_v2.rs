use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    middleware_order::{OperationClass, RateLimitConsistency, rate_limit_posture},
    rate_limit::{RateLimitFailureMode, RateLimitLayer},
};

const MAX_CAPACITY: u64 = 1_000_000_000;
const MAX_WINDOW_MS: u64 = 31 * 24 * 60 * 60 * 1_000;
const MAX_LOCAL_DENY_CACHE_ENTRIES: u32 = 10_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitAlgorithmV2 {
    TokenBucket,
    SlidingWindowCounter,
    FixedWindow,
    Gcra,
    Concurrency,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RateLimitEnforcementMode {
    Disabled,
    Audit,
    Enforce,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPolicyV2 {
    pub policy_id: String,
    pub operation_class: OperationClass,
    pub algorithm: RateLimitAlgorithmV2,
    pub consistency: RateLimitConsistency,
    pub enforcement_mode: RateLimitEnforcementMode,
    pub failure_mode: RateLimitFailureMode,
    pub layer: RateLimitLayer,
    pub capacity: u64,
    pub window_ms: Option<u64>,
    pub refill_tokens: Option<u64>,
    pub refill_interval_ms: Option<u64>,
    pub maximum_overshoot: u64,
    pub local_deny_cache_entries: u32,
    pub coordinator_required: bool,
    pub key_version: String,
}

impl RateLimitPolicyV2 {
    #[must_use]
    pub fn audit_for(operation_class: OperationClass) -> Self {
        let posture = rate_limit_posture(operation_class);
        let strict = posture.consistency == RateLimitConsistency::Strict;
        Self {
            policy_id: format!("{:?}-audit-v2", operation_class).to_ascii_lowercase(),
            operation_class,
            algorithm: if strict {
                RateLimitAlgorithmV2::Gcra
            } else {
                RateLimitAlgorithmV2::TokenBucket
            },
            consistency: posture.consistency,
            enforcement_mode: RateLimitEnforcementMode::Audit,
            failure_mode: posture.failure_mode,
            layer: RateLimitLayer::Application,
            capacity: if strict { 20 } else { 120 },
            window_ms: strict.then_some(60_000),
            refill_tokens: (!strict).then_some(120),
            refill_interval_ms: (!strict).then_some(60_000),
            maximum_overshoot: if strict { 0 } else { 8 },
            local_deny_cache_entries: MAX_LOCAL_DENY_CACHE_ENTRIES,
            coordinator_required: posture.coordinator_required,
            key_version: "v1".into(),
        }
    }

    pub fn validate(&self) -> Vec<RateLimitPolicyViolation> {
        let mut violations = Vec::new();

        if !valid_identifier(&self.policy_id) {
            violations.push(RateLimitPolicyViolation::new(
                "policy-id-invalid",
                "policyId",
                "policyId must be a non-empty ASCII token no longer than 128 bytes",
            ));
        }
        if !valid_identifier(&self.key_version) {
            violations.push(RateLimitPolicyViolation::new(
                "key-version-invalid",
                "keyVersion",
                "keyVersion must be a non-empty ASCII token no longer than 128 bytes",
            ));
        }
        if self.capacity == 0 || self.capacity > MAX_CAPACITY {
            violations.push(RateLimitPolicyViolation::new(
                "capacity-out-of-range",
                "capacity",
                "capacity must be between 1 and 1,000,000,000",
            ));
        }
        if self.local_deny_cache_entries > MAX_LOCAL_DENY_CACHE_ENTRIES {
            violations.push(RateLimitPolicyViolation::new(
                "local-cache-too-large",
                "localDenyCacheEntries",
                "local denial cache must not exceed 10,000 entries",
            ));
        }

        match self.algorithm {
            RateLimitAlgorithmV2::TokenBucket => {
                require_positive(
                    self.refill_tokens,
                    "refillTokens",
                    "token-bucket-refill-tokens-required",
                    &mut violations,
                );
                require_bounded_duration(
                    self.refill_interval_ms,
                    "refillIntervalMs",
                    "token-bucket-refill-interval-required",
                    &mut violations,
                );
                forbid(
                    self.window_ms,
                    "windowMs",
                    "token-bucket-window-forbidden",
                    &mut violations,
                );
            }
            RateLimitAlgorithmV2::SlidingWindowCounter
            | RateLimitAlgorithmV2::FixedWindow
            | RateLimitAlgorithmV2::Gcra => {
                require_bounded_duration(
                    self.window_ms,
                    "windowMs",
                    "window-required",
                    &mut violations,
                );
                forbid(
                    self.refill_tokens,
                    "refillTokens",
                    "window-refill-tokens-forbidden",
                    &mut violations,
                );
                forbid(
                    self.refill_interval_ms,
                    "refillIntervalMs",
                    "window-refill-interval-forbidden",
                    &mut violations,
                );
            }
            RateLimitAlgorithmV2::Concurrency => {
                forbid(
                    self.window_ms,
                    "windowMs",
                    "concurrency-window-forbidden",
                    &mut violations,
                );
                forbid(
                    self.refill_tokens,
                    "refillTokens",
                    "concurrency-refill-tokens-forbidden",
                    &mut violations,
                );
                forbid(
                    self.refill_interval_ms,
                    "refillIntervalMs",
                    "concurrency-refill-interval-forbidden",
                    &mut violations,
                );
            }
        }

        match self.consistency {
            RateLimitConsistency::Strict => {
                if self.failure_mode != RateLimitFailureMode::FailClosed {
                    violations.push(RateLimitPolicyViolation::new(
                        "strict-must-fail-closed",
                        "failureMode",
                        "strict consistency requires fail-closed backend behavior",
                    ));
                }
                if !self.coordinator_required {
                    violations.push(RateLimitPolicyViolation::new(
                        "strict-requires-coordinator",
                        "coordinatorRequired",
                        "strict consistency requires one atomic coordinator",
                    ));
                }
                if self.maximum_overshoot != 0 {
                    violations.push(RateLimitPolicyViolation::new(
                        "strict-overshoot-must-be-zero",
                        "maximumOvershoot",
                        "strict consistency cannot declare local overshoot",
                    ));
                }
            }
            RateLimitConsistency::Bounded => {
                if self.maximum_overshoot == 0 {
                    violations.push(RateLimitPolicyViolation::new(
                        "bounded-overshoot-required",
                        "maximumOvershoot",
                        "bounded consistency requires an explicit positive overshoot bound",
                    ));
                }
                if self.local_deny_cache_entries == 0 {
                    violations.push(RateLimitPolicyViolation::new(
                        "bounded-cache-required",
                        "localDenyCacheEntries",
                        "bounded consistency requires a finite local denial cache",
                    ));
                }
            }
            RateLimitConsistency::Advisory => {
                if self.coordinator_required {
                    violations.push(RateLimitPolicyViolation::new(
                        "advisory-coordinator-forbidden",
                        "coordinatorRequired",
                        "advisory policy cannot claim an authoritative coordinator",
                    ));
                }
                if self.failure_mode == RateLimitFailureMode::FailClosed {
                    violations.push(RateLimitPolicyViolation::new(
                        "advisory-fail-closed-misleading",
                        "failureMode",
                        "advisory policy must not present backend failure as global denial",
                    ));
                }
            }
        }

        let required = rate_limit_posture(self.operation_class);
        if required.consistency == RateLimitConsistency::Strict {
            if self.consistency != RateLimitConsistency::Strict {
                violations.push(RateLimitPolicyViolation::new(
                    "operation-requires-strict-consistency",
                    "consistency",
                    "security-sensitive operation class requires strict consistency",
                ));
            }
            if self.failure_mode != RateLimitFailureMode::FailClosed {
                violations.push(RateLimitPolicyViolation::new(
                    "operation-requires-fail-closed",
                    "failureMode",
                    "security-sensitive operation class requires fail-closed behavior",
                ));
            }
            if !self.coordinator_required {
                violations.push(RateLimitPolicyViolation::new(
                    "operation-requires-coordinator",
                    "coordinatorRequired",
                    "security-sensitive operation class requires an atomic coordinator",
                ));
            }
        }

        if self.layer == RateLimitLayer::Authorization
            && (self.consistency != RateLimitConsistency::Strict
                || self.failure_mode != RateLimitFailureMode::FailClosed
                || !self.coordinator_required)
        {
            violations.push(RateLimitPolicyViolation::new(
                "authorization-boundary-must-be-strict",
                "layer",
                "authorization-layer limiting must be strict, coordinated, and fail closed",
            ));
        }

        if self.layer == RateLimitLayer::CloudflareEdge
            && self.enforcement_mode == RateLimitEnforcementMode::Enforce
            && !required.edge_may_deny
        {
            violations.push(RateLimitPolicyViolation::new(
                "edge-denial-forbidden-for-operation",
                "enforcementMode",
                "edge may observe this operation class but cannot own its denial decision",
            ));
        }

        violations
    }

    pub fn from_json(input: &str) -> Result<Self, RateLimitPolicyDecodeError> {
        let policy: Self = serde_json::from_str(input).map_err(RateLimitPolicyDecodeError::Json)?;
        let violations = policy.validate();
        if violations.is_empty() {
            Ok(policy)
        } else {
            Err(RateLimitPolicyDecodeError::Validation(violations))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RateLimitPolicyViolation {
    pub code: &'static str,
    pub path: &'static str,
    pub message: &'static str,
}

impl RateLimitPolicyViolation {
    const fn new(code: &'static str, path: &'static str, message: &'static str) -> Self {
        Self {
            code,
            path,
            message,
        }
    }
}

#[derive(Debug)]
pub enum RateLimitPolicyDecodeError {
    Json(serde_json::Error),
    Validation(Vec<RateLimitPolicyViolation>),
}

impl fmt::Display for RateLimitPolicyDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid rate-limit policy JSON: {error}"),
            Self::Validation(violations) => write!(
                formatter,
                "rate-limit policy violated {} invariant(s)",
                violations.len()
            ),
        }
    }
}

impl std::error::Error for RateLimitPolicyDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn require_positive(
    value: Option<u64>,
    path: &'static str,
    code: &'static str,
    violations: &mut Vec<RateLimitPolicyViolation>,
) {
    if !value.is_some_and(|value| value > 0 && value <= MAX_CAPACITY) {
        violations.push(RateLimitPolicyViolation::new(
            code,
            path,
            "field is required and must be a positive bounded integer",
        ));
    }
}

fn require_bounded_duration(
    value: Option<u64>,
    path: &'static str,
    code: &'static str,
    violations: &mut Vec<RateLimitPolicyViolation>,
) {
    if !value.is_some_and(|value| value > 0 && value <= MAX_WINDOW_MS) {
        violations.push(RateLimitPolicyViolation::new(
            code,
            path,
            "duration is required and must be between 1 millisecond and 31 days",
        ));
    }
}

fn forbid(
    value: Option<u64>,
    path: &'static str,
    code: &'static str,
    violations: &mut Vec<RateLimitPolicyViolation>,
) {
    if value.is_some() {
        violations.push(RateLimitPolicyViolation::new(
            code,
            path,
            "field is not valid for the selected algorithm",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_gcra_mutation_policy_is_valid() {
        let policy = RateLimitPolicyV2::audit_for(OperationClass::Mutation);
        assert_eq!(policy.algorithm, RateLimitAlgorithmV2::Gcra);
        assert!(policy.validate().is_empty());
    }

    #[test]
    fn strict_policy_cannot_fail_open() {
        let mut policy = RateLimitPolicyV2::audit_for(OperationClass::AuthRecovery);
        policy.failure_mode = RateLimitFailureMode::FailOpen;
        assert!(
            policy
                .validate()
                .iter()
                .any(|violation| violation.code == "strict-must-fail-closed")
        );
    }

    #[test]
    fn gcra_requires_a_window_and_forbids_refill_fields() {
        let mut policy = RateLimitPolicyV2::audit_for(OperationClass::Mutation);
        policy.window_ms = None;
        policy.refill_tokens = Some(1);
        let violations = policy.validate();
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == "window-required")
        );
        assert!(violations.iter().any(|violation| {
            violation.code == "window-refill-tokens-forbidden"
        }));
    }

    #[test]
    fn bounded_policy_requires_an_explicit_overshoot_bound() {
        let mut policy = RateLimitPolicyV2::audit_for(OperationClass::PublicRead);
        policy.maximum_overshoot = 0;
        assert!(
            policy
                .validate()
                .iter()
                .any(|violation| violation.code == "bounded-overshoot-required")
        );
    }

    #[test]
    fn edge_cannot_enforce_a_ledger_write() {
        let mut policy = RateLimitPolicyV2::audit_for(OperationClass::PaymentOrLedgerWrite);
        policy.layer = RateLimitLayer::CloudflareEdge;
        policy.enforcement_mode = RateLimitEnforcementMode::Enforce;
        assert!(
            policy
                .validate()
                .iter()
                .any(|violation| violation.code == "edge-denial-forbidden-for-operation")
        );
    }

    #[test]
    fn runtime_json_validation_rejects_semantic_mismatch() {
        let input = serde_json::json!({
            "policyId": "mutation-v2",
            "operationClass": "mutation",
            "algorithm": "gcra",
            "consistency": "strict",
            "enforcementMode": "audit",
            "failureMode": "fail-open",
            "layer": "application",
            "capacity": 20,
            "windowMs": 60000,
            "maximumOvershoot": 0,
            "localDenyCacheEntries": 10000,
            "coordinatorRequired": true,
            "keyVersion": "v1"
        })
        .to_string();
        assert!(matches!(
            RateLimitPolicyV2::from_json(&input),
            Err(RateLimitPolicyDecodeError::Validation(_))
        ));
    }
}
