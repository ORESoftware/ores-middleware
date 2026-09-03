use std::{collections::{BTreeMap, HashMap}, future::Future, sync::Arc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }
}

tokio::task_local! {
    static REQUEST_CONTEXT: RequestContext;
}

pub async fn run_with_context<F>(context: RequestContext, future: F) -> F::Output
where
    F: Future,
{
    REQUEST_CONTEXT.scope(context, future).await
}

pub fn current_context() -> Option<RequestContext> {
    REQUEST_CONTEXT.try_with(Clone::clone).ok()
}

#[derive(Clone)]
pub struct ContextRegistry {
    inner: Arc<RwLock<HashMap<String, (Instant, RequestContext)>>>,
    max_entries: usize,
    ttl: Duration,
}

impl ContextRegistry {
    pub fn new(max_entries: usize, ttl: Duration) -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())), max_entries, ttl }
    }

    pub async fn insert(&self, context: RequestContext) {
        if self.max_entries == 0 { return; }
        let mut guard = self.inner.write().await;
        let now = Instant::now();
        guard.retain(|_, (created, _)| now.duration_since(*created) <= self.ttl);
        if guard.len() >= self.max_entries {
            if let Some(oldest) = guard.iter().min_by_key(|(_, (created, _))| *created).map(|(key, _)| key.clone()) {
                guard.remove(&oldest);
            }
        }
        guard.insert(context.request_id.clone(), (now, context));
    }

    pub async fn get(&self, request_id: &str) -> Option<RequestContext> {
        let guard = self.inner.read().await;
        guard.get(request_id).and_then(|(created, context)| (created.elapsed() <= self.ttl).then(|| context.clone()))
    }

    pub async fn remove(&self, request_id: &str) {
        self.inner.write().await.remove(request_id);
    }
}
