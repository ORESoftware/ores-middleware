use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Instant,
};

use tokio::sync::Mutex;

use crate::context::RequestContext;

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
}

pub trait RateLimiter: Send + Sync {
    fn allow<'a>(
        &'a self,
        key: &'a str,
        capacity: u32,
        refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
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
}

#[derive(Default)]
pub struct InMemoryTokenBucket {
    buckets: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter for InMemoryTokenBucket {
    fn allow<'a>(
        &'a self,
        key: &'a str,
        capacity: u32,
        refill_per_second: f64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let mut buckets = self.buckets.lock().await;
            let now = Instant::now();
            let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
                tokens: f64::from(capacity),
                last_refill: now,
            });
            let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.tokens =
                (bucket.tokens + elapsed * refill_per_second).min(f64::from(capacity));
            bucket.last_refill = now;
            if bucket.tokens >= 1.0 {
                bucket.tokens -= 1.0;
                true
            } else {
                false
            }
        })
    }
}

pub type DynAuthVerifier = Arc<dyn AuthVerifier>;
pub type DynSyncObserver = Arc<dyn SyncObserver>;
pub type DynTelemetrySink = Arc<dyn TelemetrySink>;
pub type DynRateLimiter = Arc<dyn RateLimiter>;
