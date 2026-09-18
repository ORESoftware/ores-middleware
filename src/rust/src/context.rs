use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use next_loggers_request_context as canonical;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Middleware-facing request snapshot retained for source and wire compatibility.
/// Ambient propagation is delegated to ores-otel's canonical poll-safe carrier;
/// ores-middleware does not create a second Tokio task-local.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    pub request_id: String,
    pub trace_id: String,
    pub span_id: Option<String>,
    pub tenant_id: Option<String>,
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

    fn to_canonical(&self) -> canonical::RequestContext {
        canonical::RequestContext {
            request_id: self.request_id.clone(),
            logged_in_user_id: self.user_id.clone(),
            tenant_id: self.tenant_id.clone(),
            trace_id: (!self.trace_id.is_empty()).then(|| self.trace_id.clone()),
            span_id: self.span_id.clone(),
            locale: self.locale.clone(),
            started_at_unix_ms: Some(self.started_at_unix_ms),
            deadline_unix_ms: self.deadline_unix_ms,
            // Only explicitly allow-listed OTel baggage crosses the middleware
            // boundary. Request/user/tenant identifiers have dedicated fields.
            baggage: self
                .baggage
                .iter()
                .filter(|(key, _)| key.starts_with("otel."))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            ..Default::default()
        }
    }

    fn from_canonical(value: canonical::RequestContext) -> Self {
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

/// Scope a future through the single canonical request/logger context carrier.
pub async fn run_with_context<F>(context: RequestContext, future: F) -> F::Output
where
    F: Future,
{
    canonical::with_request_context(context.to_canonical(), future).await
}

pub fn current_context() -> Option<RequestContext> {
    canonical::current_request_context().map(RequestContext::from_canonical)
}

/// Capture an immutable request snapshot for explicit queue/task propagation.
pub fn capture_request_context() -> Option<RequestContext> {
    canonical::capture_request_context().map(RequestContext::from_canonical)
}

/// Re-enter a previously captured snapshot. `None` deliberately runs without a
/// request context rather than inheriting one implicitly.
pub async fn run_with_captured_context<F>(context: Option<RequestContext>, future: F) -> F::Output
where
    F: Future,
{
    canonical::with_captured_request_context(
        context.map(|context| context.to_canonical()),
        future,
    )
    .await
}

/// Clones only the request ID from the canonical ambient carrier.
pub fn current_request_id() -> Option<String> {
    canonical::current_request_id()
}

/// Clones only the W3C trace ID from the canonical ambient carrier.
pub fn current_trace_id() -> Option<String> {
    canonical::current_request_context().and_then(|context| context.trace_id)
}

/// Returns the authenticated user ID from the canonical ambient carrier.
pub fn current_user_id() -> Option<String> {
    canonical::current_logged_in_user_id()
}

/// Explicit naming alias for call sites using "logged-in user" terminology.
pub fn current_logged_in_user_id() -> Option<String> {
    canonical::current_logged_in_user_id()
}

/// Returns the authenticated tenant ID from the canonical ambient carrier.
pub fn current_tenant_id() -> Option<String> {
    canonical::current_tenant_id()
}

pub fn current_session_id() -> Option<String> {
    canonical::current_session_id()
}

pub fn current_correlation_id() -> Option<String> {
    canonical::current_correlation_id()
}

/// Spawn a child task with an explicit snapshot of the current canonical logger
/// frame. Plain `tokio::spawn` intentionally does not inherit request context.
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
    /// Optional bounded diagnostics index. Ambient request propagation always
    /// uses the canonical ores-otel carrier instead of this registry.
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
        if guard.len() >= self.max_entries
            && let Some(oldest) = guard
                .iter()
                .min_by_key(|(_, (created, _))| *created)
                .map(|(key, _)| key.clone())
        {
            guard.remove(&oldest);
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

#[cfg(test)]
mod tests {
    use super::*;
    use next_loggers::current_log_context;

    fn test_context() -> RequestContext {
        RequestContext {
            request_id: "request-42".into(),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: None,
            tenant_id: Some("tenant-7".into()),
            user_id: Some("user-42".into()),
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::from([
                ("otel.vendor".into(), "allowed".into()),
                ("authorization".into(), "must-not-propagate".into()),
            ]),
        }
    }

    #[tokio::test]
    async fn typed_accessors_read_only_the_active_request_scope() {
        assert_eq!(current_request_id(), None);
        assert_eq!(current_logged_in_user_id(), None);

        run_with_context(test_context(), async {
            assert_eq!(current_request_id().as_deref(), Some("request-42"));
            assert_eq!(
                current_trace_id().as_deref(),
                Some("0123456789abcdef0123456789abcdef")
            );
            assert_eq!(current_user_id().as_deref(), Some("user-42"));
            assert_eq!(current_logged_in_user_id().as_deref(), Some("user-42"));
            assert_eq!(current_tenant_id().as_deref(), Some("tenant-7"));
        })
        .await;

        assert_eq!(current_request_id(), None);
        assert_eq!(current_user_id(), None);
    }

    #[tokio::test]
    async fn middleware_and_logger_share_one_poll_safe_carrier() {
        run_with_context(test_context(), async {
            let logger_context = current_log_context();
            assert_eq!(
                logger_context
                    .fields
                    .get("request.id")
                    .and_then(|value| value.as_str()),
                Some("request-42")
            );
            assert_eq!(logger_context.trace_id.as_deref(), Some("0123456789abcdef0123456789abcdef"));
            assert_eq!(logger_context.baggage.get("otel.vendor").map(String::as_str), Some("allowed"));
            assert!(!logger_context.baggage.contains_key("authorization"));
        })
        .await;
    }

    #[tokio::test]
    async fn explicit_capture_can_cross_a_spawn_boundary_without_implicit_inheritance() {
        run_with_context(test_context(), async {
            let plain = tokio::spawn(async { current_request_id() })
                .await
                .expect("plain child task joins");
            assert_eq!(plain, None);

            let propagated = spawn_with_current_context(async { current_request_id() })
                .await
                .expect("propagated child task joins");
            assert_eq!(propagated.as_deref(), Some("request-42"));
        })
        .await;
    }
}
