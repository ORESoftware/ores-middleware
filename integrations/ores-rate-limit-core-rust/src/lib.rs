#![forbid(unsafe_code)]

// The salvaged adapter intentionally remains outside the framework-neutral
// middleware crate. Consumers opt into this package only after Zed has
// installed and lock-verified the private deterministic core.
pub use ores_middleware::{
    IntegrationError, RateLimitAlgorithm, RateLimitDecision, RateLimitDecisionKind,
    RateLimitDecisionSource, RateLimitLayer, RateLimitPrincipal, RateLimitRequest, RateLimiter,
};

#[allow(unused_imports)]
mod core_rate_limiter;

pub use core_rate_limiter::{
    CoreInMemoryRateLimiter, DEFAULT_CORE_STATE_CAPACITY, DEFAULT_CORE_STATE_TTL,
};
