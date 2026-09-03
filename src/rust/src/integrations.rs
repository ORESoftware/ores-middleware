use std::{collections::{BTreeMap, HashMap}, sync::Arc, time::Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::context::RequestContext;

#[derive(Debug, Clone)]
pub struct RequestMetadata {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub remote_ip: Option<String>,
    pub content_length: Option<u64>,
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

#[async_trait]
pub trait AuthVerifier: Send + Sync {
    async fn verify(&self, request: &RequestMetadata) -> Result<AuthDecision, IntegrationError>;
}

#[async_trait]
pub trait SyncObserver: Send + Sync {
    async fn observe(&self, context: &RequestContext, request: &RequestMetadata, response: &ResponseMetadata) -> Result<(), IntegrationError>;
}

#[async_trait]
pub trait TelemetrySink: Send + Sync {
    async fn request_started(&self, context: &RequestContext, request: &RequestMetadata);
    async fn request_finished(&self, context: &RequestContext, request: &RequestMetadata, response: &ResponseMetadata);
}

#[async_trait]
pub trait RateLimiter: Send + Sync {
    async fn allow(&self, key: &str, capacity: u32, refill_per_second: f64) -> bool;
}

#[derive(Default)]
pub struct AnonymousAuth;

#[async_trait]
impl AuthVerifier for AnonymousAuth {
    async fn verify(&self, _request: &RequestMetadata) -> Result<AuthDecision, IntegrationError> {
        Ok(AuthDecision::default())
    }
}

#[derive(Default)]
pub struct NoopSyncObserver;

#[async_trait]
impl SyncObserver for NoopSyncObserver {
    async fn observe(&self, _context: &RequestContext, _request: &RequestMetadata, _response: &ResponseMetadata) -> Result<(), IntegrationError> { Ok(()) }
}

#[derive(Default)]
pub struct TracingTelemetry;

#[async_trait]
impl TelemetrySink for TracingTelemetry {
    async fn request_started(&self, context: &RequestContext, request: &RequestMetadata) {
        tracing::info!(request_id = %context.request_id, trace_id = %context.trace_id, method = %request.method, path = %request.path, "request started");
    }
    async fn request_finished(&self, context: &RequestContext, _request: &RequestMetadata, response: &ResponseMetadata) {
        tracing::info!(request_id = %context.request_id, trace_id = %context.trace_id, status = response.status, duration_ms = response.duration_ms, "request finished");
    }
}

#[derive(Default)]
pub struct InMemoryTokenBucket {
    buckets: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[async_trait]
impl RateLimiter for InMemoryTokenBucket {
    async fn allow(&self, key: &str, capacity: u32, refill_per_second: f64) -> bool {
        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();
        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket { tokens: capacity as f64, last_refill: now });
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_per_second).min(capacity as f64);
        bucket.last_refill = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub type DynAuthVerifier = Arc<dyn AuthVerifier>;
pub type DynSyncObserver = Arc<dyn SyncObserver>;
pub type DynTelemetrySink = Arc<dyn TelemetrySink>;
pub type DynRateLimiter = Arc<dyn RateLimiter>;
