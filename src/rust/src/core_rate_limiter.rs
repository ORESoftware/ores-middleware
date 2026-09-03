use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use ores_rl_lib_core::{
    transition, Algorithm, Decision, DenyReason, EnforcementMode, LimitPolicy, LimitState,
    OpaqueKey, TransitionError,
};
use tokio::sync::Mutex;

use crate::{
    IntegrationError, RateLimitAlgorithm, RateLimitDecision, RateLimitDecisionKind,
    RateLimitDecisionSource, RateLimitRequest, RateLimiter,
};

pub const DEFAULT_CORE_STATE_CAPACITY: usize = 10_000;
pub const DEFAULT_CORE_STATE_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct StateKey {
    policy_id: String,
    principal: OpaqueKey,
}

#[derive(Clone, Copy, Debug)]
struct StateEntry {
    state: LimitState,
    last_seen_ms: u64,
    generation: u64,
}

#[derive(Debug, Default)]
struct StateStore {
    entries: HashMap<StateKey, StateEntry>,
    next_generation: u64,
}

/// Application-layer adapter backed by the deterministic state machines in
/// `ores-rate-limit/ores-rl-lib-core`.
///
/// The adapter keeps only a bounded, process-local hot set. It is appropriate as
/// a fast local limiter or fallback, not as a strict cross-node quota ledger.
/// Distributed denial propagation belongs in `ores-redis-lru-cache`; durable
/// business quotas belong in a transactional data store.
pub struct CoreInMemoryRateLimiter {
    state: Mutex<StateStore>,
    max_entries: usize,
    ttl: Duration,
    started: Instant,
}

impl Default for CoreInMemoryRateLimiter {
    fn default() -> Self {
        Self::with_bounds(DEFAULT_CORE_STATE_CAPACITY, DEFAULT_CORE_STATE_TTL)
            .expect("default core limiter bounds are valid")
    }
}

impl CoreInMemoryRateLimiter {
    pub fn with_bounds(max_entries: usize, ttl: Duration) -> Result<Self, IntegrationError> {
        if max_entries == 0 || ttl.is_zero() {
            return Err(IntegrationError {
                code: "rate_limit_core_bounds_invalid",
                message: "core rate-limit state capacity and TTL must be positive".into(),
            });
        }
        Ok(Self {
            state: Mutex::new(StateStore::default()),
            max_entries,
            ttl,
            started: Instant::now(),
        })
    }

    pub async fn evaluate_at(
        &self,
        request: &RateLimitRequest,
        now_ms: u64,
    ) -> Result<RateLimitDecision, IntegrationError> {
        let principal = parse_principal(&request.principal.digest)?;
        let policy = core_policy(request)?;
        let key = StateKey {
            policy_id: request.policy_id.clone(),
            principal,
        };
        let ttl_ms = duration_ms(self.ttl);

        let mut store = self.state.lock().await;
        store.entries.retain(|_, entry| {
            now_ms >= entry.last_seen_ms && now_ms - entry.last_seen_ms < ttl_ms
        });

        let previous = store
            .entries
            .get(&key)
            .map_or(LimitState::Empty, |entry| entry.state);
        let (next, decision) = transition(policy, previous, now_ms, u64::from(request.cost))
            .map_err(transition_error)?;

        if !store.entries.contains_key(&key) && store.entries.len() >= self.max_entries {
            if let Some(oldest) = store
                .entries
                .iter()
                .min_by_key(|(_, entry)| (entry.last_seen_ms, entry.generation))
                .map(|(key, _)| key.clone())
            {
                store.entries.remove(&oldest);
            }
        }

        store.next_generation = store.next_generation.wrapping_add(1);
        if store.next_generation == 0 {
            store.next_generation = 1;
        }
        let generation = store.next_generation;
        store.entries.insert(
            key,
            StateEntry {
                state: next,
                last_seen_ms: now_ms,
                generation,
            },
        );
        drop(store);

        Ok(map_decision(request, decision))
    }

    #[cfg(test)]
    async fn state_len(&self) -> usize {
        self.state.lock().await.entries.len()
    }

    fn elapsed_ms(&self) -> u64 {
        self.started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX))
            .try_into()
            .expect("elapsed duration was clamped to u64")
    }
}

impl RateLimiter for CoreInMemoryRateLimiter {
    fn allow<'a>(
        &'a self,
        key: &'a str,
        capacity: u32,
        refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let request = RateLimitRequest {
                principal: crate::RateLimitPrincipal {
                    digest: key.to_owned(),
                    key_version: "legacy-v1".into(),
                },
                policy_id: "legacy-token-bucket".into(),
                algorithm: RateLimitAlgorithm::TokenBucket,
                layer: crate::RateLimitLayer::Application,
                capacity,
                refill_per_second,
                window_ms: 1_000,
                cost: 1,
            };
            self.evaluate_at(&request, self.elapsed_ms())
                .await
                .is_ok_and(|decision| decision.is_allowed())
        })
    }

    fn evaluate<'a>(
        &'a self,
        request: &'a RateLimitRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<RateLimitDecision, IntegrationError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { self.evaluate_at(request, self.elapsed_ms()).await })
    }
}

fn parse_principal(digest: &str) -> Result<OpaqueKey, IntegrationError> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(IntegrationError {
            code: "rate_limit_core_principal_invalid",
            message: "rate-limit principal must be exactly 64 lowercase hex characters".into(),
        });
    }
    digest.parse::<OpaqueKey>().map_err(|_| IntegrationError {
        code: "rate_limit_core_principal_invalid",
        message: "rate-limit principal could not be decoded".into(),
    })
}

fn core_policy(request: &RateLimitRequest) -> Result<LimitPolicy, IntegrationError> {
    let capacity = u64::from(request.capacity);
    let mut policy = match request.algorithm {
        RateLimitAlgorithm::TokenBucket => {
            let (tokens, interval_ms) = rational_refill(request.refill_per_second)?;
            LimitPolicy::token_bucket(capacity, tokens, interval_ms)
        }
        RateLimitAlgorithm::SlidingWindowCounter => {
            LimitPolicy::sliding_window(capacity, request.window_ms)
        }
        RateLimitAlgorithm::FixedWindow => LimitPolicy::fixed_window(capacity, request.window_ms),
        RateLimitAlgorithm::Concurrency => {
            return Err(IntegrationError {
                code: "rate_limit_core_concurrency_unsupported",
                message: "concurrency limits require a cancellation-safe permit adapter".into(),
            });
        }
    };
    policy.mode = EnforcementMode::Enforce;
    policy.validate().map_err(|error| IntegrationError {
        code: "rate_limit_core_policy_invalid",
        message: error.to_string(),
    })
}

fn rational_refill(rate_per_second: f64) -> Result<(u64, u64), IntegrationError> {
    const PRECISION: u64 = 1_000_000;
    if !rate_per_second.is_finite() || rate_per_second <= 0.0 {
        return Err(IntegrationError {
            code: "rate_limit_core_refill_invalid",
            message: "token-bucket refill rate must be finite and positive".into(),
        });
    }
    let scaled = rate_per_second * PRECISION as f64;
    if !scaled.is_finite() || scaled < 1.0 || scaled > u64::MAX as f64 {
        return Err(IntegrationError {
            code: "rate_limit_core_refill_out_of_range",
            message: "token-bucket refill rate is outside the supported range".into(),
        });
    }
    let numerator = scaled.round() as u64;
    let divisor = greatest_common_divisor(numerator, PRECISION);
    let tokens = numerator / divisor;
    let interval_ms = 1_000_u64
        .checked_mul(PRECISION / divisor)
        .ok_or_else(|| IntegrationError {
            code: "rate_limit_core_refill_out_of_range",
            message: "token-bucket refill interval overflowed".into(),
        })?;
    Ok((tokens, interval_ms))
}

const fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn map_decision(request: &RateLimitRequest, decision: Decision) -> RateLimitDecision {
    match decision {
        Decision::Bypass { reason } => RateLimitDecision {
            kind: RateLimitDecisionKind::Allowed,
            source: RateLimitDecisionSource::LocalMemory,
            policy_id: request.policy_id.clone(),
            layer: request.layer,
            algorithm: request.algorithm,
            limit: request.capacity,
            remaining: request.capacity,
            retry_after_ms: None,
            reset_after_ms: None,
            reason_code: Some(reason.into()),
        },
        Decision::Allow {
            remaining,
            reset_after_ms,
            observed_only,
        } => RateLimitDecision {
            kind: if observed_only {
                RateLimitDecisionKind::DegradedAllowed
            } else {
                RateLimitDecisionKind::Allowed
            },
            source: RateLimitDecisionSource::LocalMemory,
            policy_id: request.policy_id.clone(),
            layer: request.layer,
            algorithm: request.algorithm,
            limit: request.capacity,
            remaining: u32::try_from(remaining).unwrap_or(u32::MAX),
            retry_after_ms: None,
            reset_after_ms: Some(reset_after_ms),
            reason_code: observed_only.then(|| "observe-only-overage".into()),
        },
        Decision::Deny {
            retry_after_ms,
            reason,
        } => RateLimitDecision {
            kind: RateLimitDecisionKind::Denied,
            source: RateLimitDecisionSource::LocalMemory,
            policy_id: request.policy_id.clone(),
            layer: request.layer,
            algorithm: request.algorithm,
            limit: request.capacity,
            remaining: 0,
            retry_after_ms: Some(retry_after_ms),
            reset_after_ms: Some(retry_after_ms),
            reason_code: Some(deny_reason(reason).into()),
        },
    }
}

const fn deny_reason(reason: DenyReason) -> &'static str {
    match reason {
        DenyReason::LimitExceeded => "limit-exceeded",
        DenyReason::BackendUnavailable => "backend-unavailable",
        DenyReason::LocallyBlocked => "locally-blocked",
    }
}

fn transition_error(error: TransitionError) -> IntegrationError {
    let code = match error {
        TransitionError::InvalidPolicy(_) => "rate_limit_core_policy_invalid",
        TransitionError::ZeroCost => "rate_limit_core_zero_cost",
        TransitionError::CostExceedsCapacity => "rate_limit_core_cost_exceeds_capacity",
        TransitionError::ClockMovedBackwards => "rate_limit_core_clock_moved_backwards",
        TransitionError::StateAlgorithmMismatch => "rate_limit_core_state_algorithm_mismatch",
    };
    IntegrationError {
        code,
        message: error.to_string(),
    }
}

fn duration_ms(value: Duration) -> u64 {
    value
        .as_millis()
        .min(u128::from(u64::MAX))
        .try_into()
        .expect("duration was clamped to u64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RateLimitLayer, RateLimitPrincipal};

    fn request(digest_byte: u8, policy_id: &str, algorithm: RateLimitAlgorithm) -> RateLimitRequest {
        RateLimitRequest {
            principal: RateLimitPrincipal {
                digest: format!("{digest_byte:02x}").repeat(32),
                key_version: "v1".into(),
            },
            policy_id: policy_id.into(),
            algorithm,
            layer: RateLimitLayer::Application,
            capacity: 2,
            refill_per_second: 1.0,
            window_ms: 1_000,
            cost: 1,
        }
    }

    #[tokio::test]
    async fn token_bucket_uses_deterministic_core_transitions() {
        let limiter = CoreInMemoryRateLimiter::default();
        let request = request(0x11, "login", RateLimitAlgorithm::TokenBucket);
        let first = limiter.evaluate_at(&request, 0).await.unwrap();
        let second = limiter.evaluate_at(&request, 0).await.unwrap();
        let third = limiter.evaluate_at(&request, 0).await.unwrap();
        assert_eq!(first.remaining, 1);
        assert_eq!(second.remaining, 0);
        assert_eq!(third.kind, RateLimitDecisionKind::Denied);
        assert_eq!(third.retry_after_ms, Some(1_000));
    }

    #[tokio::test]
    async fn fixed_window_resets_at_the_boundary() {
        let limiter = CoreInMemoryRateLimiter::default();
        let request = request(0x22, "write", RateLimitAlgorithm::FixedWindow);
        limiter.evaluate_at(&request, 0).await.unwrap();
        limiter.evaluate_at(&request, 1).await.unwrap();
        assert_eq!(
            limiter.evaluate_at(&request, 999).await.unwrap().kind,
            RateLimitDecisionKind::Denied
        );
        assert_eq!(
            limiter.evaluate_at(&request, 1_000).await.unwrap().kind,
            RateLimitDecisionKind::Allowed
        );
    }

    #[tokio::test]
    async fn reusing_policy_id_with_another_algorithm_is_rejected() {
        let limiter = CoreInMemoryRateLimiter::default();
        limiter
            .evaluate_at(
                &request(0x33, "stable-policy", RateLimitAlgorithm::FixedWindow),
                0,
            )
            .await
            .unwrap();
        let error = limiter
            .evaluate_at(
                &request(
                    0x33,
                    "stable-policy",
                    RateLimitAlgorithm::SlidingWindowCounter,
                ),
                1,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "rate_limit_core_state_algorithm_mismatch");
    }

    #[tokio::test]
    async fn malformed_principal_never_becomes_a_backend_key() {
        let limiter = CoreInMemoryRateLimiter::default();
        let mut request = request(0x44, "read", RateLimitAlgorithm::TokenBucket);
        request.principal.digest = "raw@example.com".into();
        let error = limiter.evaluate_at(&request, 0).await.unwrap_err();
        assert_eq!(error.code, "rate_limit_core_principal_invalid");
    }

    #[tokio::test]
    async fn local_state_is_bounded_under_principal_churn() {
        let limiter = CoreInMemoryRateLimiter::with_bounds(1, Duration::from_secs(30)).unwrap();
        limiter
            .evaluate_at(&request(0x55, "read", RateLimitAlgorithm::TokenBucket), 0)
            .await
            .unwrap();
        limiter
            .evaluate_at(&request(0x66, "read", RateLimitAlgorithm::TokenBucket), 1)
            .await
            .unwrap();
        assert_eq!(limiter.state_len().await, 1);
    }
}
