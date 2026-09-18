use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use next_loggers_request_context as canonical;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Middleware-facing request snapshot retained for source compatibility.
///
/// Ambient propagation is owned by ores-otel's canonical poll-safe request
/// context carrier. `baggage` crosses that telemetry boundary, so only `otel.*`
/// entries are admitted when the snapshot is installed.
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

/// Scope a future through the single poll-safe request carrier shared with
/// ores-otel. This intentionally does not create a middleware-specific Tokio
/// task-local.
pub async fn run_with_context<F>(context: RequestContext, future: F) -> F::Output
where
    F: Future,
{
    canonical::with_request_context(context.to_canonical(), future).await
}

pub fn current_context() -> Option<RequestContext> {
    canonical::current_request_context().map(RequestContext::from_canonical)
}

/// Capture a detached defensive request snapshot for queues or explicitly
/// propagated child work.
pub fn capture_request_context() -> Option<RequestContext> {
    canonical::capture_request_context().map(RequestContext::from_canonical)
}

/// Re-enter a captured request snapshot. `None` deliberately installs an empty
/// canonical logger frame for the duration of the future so unrelated caller
/// context cannot leak into work that was captured outside a request.
pub async fn run_with_captured_context<F>(
    context: Option<RequestContext>,
    future: F,
) -> F::Output
where
    F: Future,
{
    match context {
        Some(context) => run_with_context(context, future).await,
        None => next_loggers::with_log_context_async(next_loggers::LogContext::default(), future).await,
    }
}

pub fn current_request_id() -> Option<String> {
    canonical::current_request_id()
}

pub fn current_trace_id() -> Option<String> {
    current_context().map(|context| context.trace_id).filter(|value| !value.is_empty())
}

pub fn current_user_id() -> Option<String> {
    canonical::current_logged_in_user_id()
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

/// Tokio tasks do not implicitly inherit a logical request. This helper
/// captures the canonical logger/request frame and re-enters it in the child.
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
    /// Optional bounded diagnostics index. It is never the ambient propagation
    /// or business-logic lookup mechanism; the canonical carrier is authoritative.
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

    fn test_context(id: &str) -> RequestContext {
        RequestContext {
            request_id: format!("request-{id}"),
            trace_id: format!("trace-{id}"),
            span_id: None,
            tenant_id: Some(format!("tenant-{id}")),
            user_id: Some(format!("user-{id}")),
            locale: None,
            started_at_unix_ms: 1,
            deadline_unix_ms: Some(2),
            baggage: BTreeMap::from([
                ("otel.safe".into(), id.into()),
                ("authorization".into(), "must-not-propagate".into()),
            ]),
        }
    }

    #[tokio::test]
    async fn typed_accessors_use_the_canonical_request_carrier() {
        assert_eq!(current_request_id(), None);
        assert_eq!(current_logged_in_user_id(), None);

        run_with_context(test_context("42"), async {
            assert_eq!(current_request_id().as_deref(), Some("request-42"));
            assert_eq!(current_trace_id().as_deref(), Some("trace-42"));
            assert_eq!(current_user_id().as_deref(), Some("user-42"));
            assert_eq!(current_logged_in_user_id().as_deref(), Some("user-42"));
            assert_eq!(current_tenant_id().as_deref(), Some("tenant-42"));
            let current = current_context().expect("request context");
            assert_eq!(current.baggage.get("otel.safe").map(String::as_str), Some("42"));
            assert!(!current.baggage.contains_key("authorization"));
            assert_eq!(
                next_loggers::current_log_context().fields.get("request.id"),
                Some(&next_loggers::Value::String("request-42".into()))
            );
        })
        .await;

        assert_eq!(current_request_id(), None);
        assert_eq!(current_user_id(), None);
    }

    #[tokio::test]
    async fn nested_and_captured_scopes_restore_exact_request_identity() {
        run_with_context(test_context("outer"), async {
            let captured = capture_request_context();
            run_with_context(test_context("inner"), async {
                assert_eq!(current_request_id().as_deref(), Some("request-inner"));
            })
            .await;
            assert_eq!(current_request_id().as_deref(), Some("request-outer"));

            run_with_captured_context(captured, async {
                assert_eq!(current_request_id().as_deref(), Some("request-outer"));
            })
            .await;
        })
        .await;
        assert_eq!(current_request_id(), None);
    }

    #[tokio::test]
    async fn explicitly_absent_capture_clears_an_unrelated_request_scope() {
        let absent = capture_request_context();
        assert!(absent.is_none());

        run_with_context(test_context("caller"), async {
            assert_eq!(current_request_id().as_deref(), Some("request-caller"));
            run_with_captured_context(absent, async {
                assert_eq!(current_request_id(), None);
                assert_eq!(next_loggers::current_log_context(), next_loggers::LogContext::default());
            })
            .await;
            assert_eq!(current_request_id().as_deref(), Some("request-caller"));
        })
        .await;
    }

    #[tokio::test]
    async fn spawned_child_requires_explicit_context_handoff() {
        run_with_context(test_context("parent"), async {
            let handle = spawn_with_current_context(async { current_request_id() });
            assert_eq!(
                handle.await.expect("child task").as_deref(),
                Some("request-parent")
            );
        })
        .await;
        assert_eq!(current_request_id(), None);
    }
}
