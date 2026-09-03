use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use next_loggers_request_context as canonical;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Middleware-facing request snapshot retained for source compatibility. The
/// native ambient carrier belongs to `ores-otel`; this type is converted into
/// `ores.request-context.v1` at the request boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    pub request_id: String,
    pub trace_id: String,
    pub span_id: Option<String>,
    pub tenant_id: Option<String>,
    /// Authenticated user identifier. It maps to canonical `loggedInUserId`.
    pub user_id: Option<String>,
    pub locale: Option<String>,
    pub started_at_unix_ms: u64,
    pub deadline_unix_ms: Option<u64>,
    pub baggage: BTreeMap<String, String>,
}

impl RequestContext {
    pub fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn logged_in_user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    pub(crate) fn to_canonical(&self) -> canonical::RequestContext {
        canonical::RequestContext {
            request_id: self.request_id.clone(),
            logged_in_user_id: self.user_id.clone(),
            tenant_id: self.tenant_id.clone(),
            trace_id: (!self.trace_id.is_empty()).then(|| self.trace_id.clone()),
            span_id: self.span_id.clone(),
            locale: self.locale.clone(),
            started_at_unix_ms: Some(self.started_at_unix_ms),
            deadline_unix_ms: self.deadline_unix_ms,
            // Middleware is the trust boundary: only explicit OTel baggage is
            // propagated. Request IDs and user IDs live in dedicated fields.
            baggage: self
                .baggage
                .iter()
                .filter(|(key, _)| key.starts_with("otel."))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn from_canonical(value: canonical::RequestContext) -> Self {
        Self {
            request_id: value.request_id,
            trace_id: value.trace_id.unwrap_or_default(),
            span_id: value.span_id,
            tenant_id: value.tenant_id,
            user_id: value.logged_in_user_id,
            locale: value.locale,
            started_at_unix_ms: value.started_at_unix_ms.unwrap_or_default(),
            deadline_unix_ms: value.deadline_unix_ms,
            baggage: value.baggage,
        }
    }
}

/// Scope a future through the single poll-safe context carrier owned by
/// `ores-otel`. No middleware-specific Tokio task-local is created.
pub async fn run_with_context<F>(context: RequestContext, future: F) -> F::Output
where
    F: Future,
{
    canonical::with_request_context(context.to_canonical(), future).await
}

pub fn current_context() -> Option<RequestContext> {
    canonical::current_request_context().map(RequestContext::from_canonical)
}

pub fn capture_request_context() -> Option<RequestContext> {
    canonical::capture_request_context().map(RequestContext::from_canonical)
}

pub async fn run_with_captured_context<F>(
    context: Option<RequestContext>,
    future: F,
) -> F::Output
where
    F: Future,
{
    canonical::with_captured_request_context(
        context.map(|context| context.to_canonical()),
        future,
    )
    .await
}

pub fn current_request_id() -> Option<String> {
    canonical::current_request_id()
}

pub fn current_logged_in_user_id() -> Option<String> {
    canonical::current_logged_in_user_id()
}

pub fn current_tenant_id() -> Option<String> {
    canonical::current_tenant_id()
}

pub fn current_session_id() -> Option<String> {
    canonical::current_session_id()
}

pub fn current_correlation_id() -> Option<String> {
    canonical::current_correlation_id()
}

pub fn spawn_with_current_context<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    canonical::spawn_with_current_request_context(future)
}

#[derive(Clone)]
pub struct ContextRegistry {
    inner: Arc<RwLock<HashMap<String, (Instant, RequestContext)>>>,
    max_entries: usize,
    ttl: Duration,
}

impl ContextRegistry {
    /// Optional bounded diagnostics index. It is never the propagation or
    /// business-logic lookup mechanism; the ores-otel carrier is authoritative.
    pub fn new(max_entries: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            max_entries,
            ttl,
        }
    }

    pub async fn insert(&self, context: RequestContext) {
        if self.max_entries == 0 {
            return;
        }
        let mut guard = self.inner.write().await;
        let now = Instant::now();
        guard.retain(|_, (created, _)| now.duration_since(*created) <= self.ttl);
        if guard.len() >= self.max_entries {
            if let Some(oldest) = guard
                .iter()
                .min_by_key(|(_, (created, _))| *created)
                .map(|(key, _)| key.clone())
            {
                guard.remove(&oldest);
            }
        }
        guard.insert(context.request_id.clone(), (now, context));
    }

    pub async fn get(&self, request_id: &str) -> Option<RequestContext> {
        let guard = self.inner.read().await;
        guard
            .get(request_id)
            .and_then(|(created, context)| (created.elapsed() <= self.ttl).then(|| context.clone()))
    }

    pub async fn remove(&self, request_id: &str) {
        self.inner.write().await.remove(request_id);
    }
}
