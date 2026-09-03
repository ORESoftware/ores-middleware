use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::{
    context::RequestContext,
    rate_limit::{
        RateLimitAlgorithm, RateLimitDecision, RateLimitDecisionKind, RateLimitDecisionSource,
        RateLimitRequest,
    },
};

/// Trusted transport metadata inserted by the listener or framework adapter.
///
/// Never derive this value from a public request header. In-process TLS listeners
/// should insert `TransportSecurity::secure()` into request extensions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransportSecurity {
    pub secure: bool,
}

impl TransportSecurity {
    pub const fn secure() -> Self {
        Self { secure: true }
    }

    pub const fn insecure() -> Self {
        Self { secure: false }
    }
}

#[derive(Debug, Clone)]
pub struct RequestMetadata {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub remote_ip: Option<String>,
    pub content_length: Option<u64>,
    pub transport_secure: bool,
}

#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub status: u16,
    pub duration_ms: u64,
    pub response_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct AuthDecision {
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub claims: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct IntegrationError {
    pub code: &'static str,
    pub message: String,
}

pub trait AuthVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>>;
}

pub trait SyncObserver: Send + Sync {
    fn observe<'a>(
        &'a self,
        context: &'a RequestContext,
        request: &'a RequestMetadata,
        response: &'a ResponseMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>>;
}

pub trait TelemetrySink: Send + Sync {
    fn request_started<'a>(
        &'a self,
        context: &'a RequestContext,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    fn request_finished<'a>(
        &'a self,
        context: &'a RequestContext,
        request: &'a RequestMetadata,
        response: &'a ResponseMetadata,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    fn rate_limit_decision<'a>(
        &'a self,
        _context: &'a RequestContext,
        _request: &'a RequestMetadata,
        _decision: &'a RateLimitDecision,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}

pub trait RateLimiter: Send + Sync {
    /// Compatibility method retained for existing adapters.
    fn allow<'a>(
        &'a self,
        key: &'a str,
        capacity: u32,
        refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

    /// Structured decision API used by the layered middleware contract.
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
        Box::pin(async move {
            let allowed = self
                .allow(
                    &request.principal.digest,
                    request.capacity,
                    request.refill_per_second,
                )
                .await;
            Ok(RateLimitDecision::legacy(
                request,
                allowed,
                RateLimitDecisionSource::Unknown,
            ))
        })
    }
}

#[derive(Default)]
pub struct AnonymousAuth;

impl AuthVerifier for AnonymousAuth {
    fn verify<'a>(
        &'a self,
        _request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(async { Ok(AuthDecision::default()) })
    }
}

#[derive(Default)]
pub struct NoopSyncObserver;

impl SyncObserver for NoopSyncObserver {
    fn observe<'a>(
        &'a self,
        _context: &'a RequestContext,
        _request: &'a RequestMetadata,
        _response: &'a ResponseMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<(), IntegrationError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
pub struct TracingTelemetry;

impl TelemetrySink for TracingTelemetry {
    fn request_started<'a>(
        &'a self,
        context: &'a RequestContext,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                request_id = %context.request_id,
                trace_id = %context.trace_id,
                method = %request.method,
                path = %request.path,
                "request started"
            );
        })
    }

    fn request_finished<'a>(
        &'a self,
        context: &'a RequestContext,
        _request: &'a RequestMetadata,
        response: &'a ResponseMetadata,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                request_id = %context.request_id,
                trace_id = %context.trace_id,
                status = response.status,
                duration_ms = response.duration_ms,
                "request finished"
            );
        })
    }

    fn rate_limit_decision<'a>(
        &'a self,
        context: &'a RequestContext,
        _request: &'a RequestMetadata,
        decision: &'a RateLimitDecision,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                request_id = %context.request_id,
                trace_id = %context.trace_id,
                rate_limit_policy_id = %decision.policy_id,
                rate_limit_layer = ?decision.layer,
                rate_limit_algorithm = ?decision.algorithm,
                rate_limit_outcome = ?decision.kind,
                rate_limit_source = ?decision.source,
                rate_limit_remaining = decision.remaining,
                rate_limit_retry_after_ms = ?decision.retry_after_ms,
                rate_limit_reason_code = ?decision.reason_code.as_deref(),
                "rate-limit decision"
            );
        })
    }
}

pub struct InMemoryTokenBucket {
    state: Mutex<BucketStore>,
    max_entries: usize,
    ttl: Duration,
}

struct BucketStore {
    buckets: HashMap<String, Bucket>,
    recency: VecDeque<(String, u64)>,
    next_generation: u64,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
    last_seen: Instant,
    generation: u64,
}

impl Default for InMemoryTokenBucket {
    fn default() -> Self {
        Self::with_bounds(10_000, Duration::from_secs(30))
    }
}

impl InMemoryTokenBucket {
    pub fn with_bounds(max_entries: usize, ttl: Duration) -> Self {
        Self {
            state: Mutex::new(BucketStore {
                buckets: HashMap::new(),
                recency: VecDeque::new(),
                next_generation: 0,
            }),
            max_entries,
            ttl,
        }
    }

    async fn consume(
        &self,
        key: &str,
        capacity: u32,
        refill_per_second: f64,
        cost: u32,
    ) -> Result<BucketDecision, IntegrationError> {
        if self.max_entries == 0 || self.ttl.is_zero() {
            return Err(IntegrationError {
                code: "rate_limit_local_cache_invalid",
                message: "local rate-limit cache bounds must be positive".into(),
            });
        }
        if capacity == 0
            || cost == 0
            || cost > capacity
            || !refill_per_second.is_finite()
            || refill_per_second <= 0.0
        {
            return Err(IntegrationError {
                code: "rate_limit_request_invalid",
                message: "token-bucket capacity, cost, and refill must be positive".into(),
            });
        }

        let mut state = self.state.lock().await;
        let now = Instant::now();
        purge_expired(&mut state, now, self.ttl);

        if !state.buckets.contains_key(key) {
            while state.buckets.len() >= self.max_entries {
                if !evict_one(&mut state) {
                    return Err(IntegrationError {
                        code: "rate_limit_local_cache_saturated",
                        message: "local rate-limit cache could not evict an entry".into(),
                    });
                }
            }
        }

        state.next_generation = state.next_generation.wrapping_add(1);
        let generation = state.next_generation;
        let decision = {
            let bucket = state.buckets.entry(key.to_owned()).or_insert(Bucket {
                tokens: f64::from(capacity),
                last_refill: now,
                last_seen: now,
                generation,
            });
            let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.tokens =
                (bucket.tokens + elapsed * refill_per_second).min(f64::from(capacity));
            bucket.last_refill = now;
            bucket.last_seen = now;
            bucket.generation = generation;

            let cost = f64::from(cost);
            let allowed = bucket.tokens >= cost;
            if allowed {
                bucket.tokens -= cost;
            }
            let remaining = bucket
                .tokens
                .floor()
                .max(0.0)
                .min(f64::from(u32::MAX)) as u32;
            let retry_after_ms = if allowed {
                None
            } else {
                Some(milliseconds_until(cost - bucket.tokens, refill_per_second))
            };
            let reset_after_ms = Some(milliseconds_until(
                f64::from(capacity) - bucket.tokens,
                refill_per_second,
            ));
            BucketDecision {
                allowed,
                remaining,
                retry_after_ms,
                reset_after_ms,
            }
        };
        state.recency.push_back((key.to_owned(), generation));
        Ok(decision)
    }

    #[cfg(test)]
    async fn contains_key(&self, key: &str) -> bool {
        self.state.lock().await.buckets.contains_key(key)
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.state.lock().await.buckets.len()
    }
}

impl RateLimiter for InMemoryTokenBucket {
    fn allow<'a>(
        &'a self,
        key: &'a str,
        capacity: u32,
        refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            self.consume(key, capacity, refill_per_second, 1)
                .await
                .is_ok_and(|decision| decision.allowed)
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
        Box::pin(async move {
            match request.algorithm {
                RateLimitAlgorithm::TokenBucket => {}
                RateLimitAlgorithm::SlidingWindowCounter
                | RateLimitAlgorithm::FixedWindow
                | RateLimitAlgorithm::Concurrency => {
                    return Err(IntegrationError {
                        code: "rate_limit_algorithm_unsupported_by_local_cache",
                        message: "the local fallback supports token-bucket policies only".into(),
                    });
                }
            }

            let bucket = self
                .consume(
                    &request.principal.digest,
                    request.capacity,
                    request.refill_per_second,
                    request.cost,
                )
                .await?;
            Ok(RateLimitDecision {
                kind: if bucket.allowed {
                    RateLimitDecisionKind::Allowed
                } else {
                    RateLimitDecisionKind::Denied
                },
                source: RateLimitDecisionSource::LocalMemory,
                policy_id: request.policy_id.clone(),
                layer: request.layer,
                algorithm: request.algorithm,
                limit: request.capacity,
                remaining: bucket.remaining,
                retry_after_ms: bucket.retry_after_ms,
                reset_after_ms: bucket.reset_after_ms,
                reason_code: None,
            })
        })
    }
}

struct BucketDecision {
    allowed: bool,
    remaining: u32,
    retry_after_ms: Option<u64>,
    reset_after_ms: Option<u64>,
}

fn milliseconds_until(tokens: f64, refill_per_second: f64) -> u64 {
    if tokens <= 0.0 {
        return 0;
    }
    ((tokens / refill_per_second) * 1_000.0)
        .ceil()
        .max(1.0)
        .min(u64::MAX as f64) as u64
}

fn purge_expired(state: &mut BucketStore, now: Instant, ttl: Duration) {
    loop {
        let Some((key, generation)) = state.recency.front().cloned() else {
            return;
        };
        let (remove_recency, remove_bucket) = match state.buckets.get(&key) {
            None => (true, false),
            Some(bucket) if bucket.generation != generation => (true, false),
            Some(bucket) if now.duration_since(bucket.last_seen) >= ttl => (true, true),
            Some(_) => (false, false),
        };
        if remove_bucket {
            state.buckets.remove(&key);
        }
        if remove_recency {
            state.recency.pop_front();
        } else {
            return;
        }
    }
}

fn evict_one(state: &mut BucketStore) -> bool {
    while let Some((key, generation)) = state.recency.pop_front() {
        let is_live = state
            .buckets
            .get(&key)
            .is_some_and(|bucket| bucket.generation == generation);
        if is_live {
            state.buckets.remove(&key);
            return true;
        }
    }
    false
}

pub type DynAuthVerifier = Arc<dyn AuthVerifier>;
pub type DynSyncObserver = Arc<dyn SyncObserver>;
pub type DynTelemetrySink = Arc<dyn TelemetrySink>;
pub type DynRateLimiter = Arc<dyn RateLimiter>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limit::{RateLimitLayer, RateLimitPrincipal};

    fn request(key: &str, capacity: u32) -> RateLimitRequest {
        RateLimitRequest {
            principal: RateLimitPrincipal {
                digest: key.into(),
                key_version: "v1".into(),
            },
            policy_id: "test".into(),
            algorithm: RateLimitAlgorithm::TokenBucket,
            layer: RateLimitLayer::Application,
            capacity,
            refill_per_second: 0.000_001,
            window_ms: 1_000,
            cost: 1,
        }
    }

    #[tokio::test]
    async fn local_cache_never_exceeds_configured_bound() {
        let limiter = InMemoryTokenBucket::with_bounds(2, Duration::from_secs(30));
        limiter.evaluate(&request("a", 10)).await.unwrap();
        limiter.evaluate(&request("b", 10)).await.unwrap();
        limiter.evaluate(&request("c", 10)).await.unwrap();

        assert_eq!(limiter.len().await, 2);
        assert!(!limiter.contains_key("a").await);
        assert!(limiter.contains_key("b").await);
        assert!(limiter.contains_key("c").await);
    }

    #[tokio::test]
    async fn concurrent_consumers_cannot_exceed_capacity() {
        let limiter = Arc::new(InMemoryTokenBucket::with_bounds(
            10,
            Duration::from_secs(30),
        ));
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let limiter = limiter.clone();
            tasks.push(tokio::spawn(async move {
                limiter.evaluate(&request("same-key", 10)).await.unwrap()
            }));
        }

        let mut allowed = 0;
        for task in tasks {
            if task.await.unwrap().is_allowed() {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 10);
    }

    #[tokio::test]
    async fn exhausted_bucket_returns_retry_metadata() {
        let limiter = InMemoryTokenBucket::with_bounds(10, Duration::from_secs(30));
        let first = limiter.evaluate(&request("same-key", 1)).await.unwrap();
        let second = limiter.evaluate(&request("same-key", 1)).await.unwrap();

        assert!(first.is_allowed());
        assert!(!second.is_allowed());
        assert!(second.retry_after_ms.is_some_and(|value| value > 0));
        assert_eq!(second.remaining, 0);
    }
}
