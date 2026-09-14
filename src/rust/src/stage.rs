use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use serde_json::{Value, json};

use crate::{context::RequestContext, integrations::RequestMetadata};

#[derive(Debug, Clone)]
pub struct StageInput {
    pub request: RequestMetadata,
    pub context: RequestContext,
    pub decoded_payload: Option<Value>,
    pub attributes: BTreeMap<String, Value>,
}

impl StageInput {
    pub fn new(request: RequestMetadata, context: RequestContext) -> Self {
        Self {
            request,
            context,
            decoded_payload: None,
            attributes: BTreeMap::new(),
        }
    }

    pub fn with_decoded_payload(mut self, payload: Value) -> Self {
        self.decoded_payload = Some(payload);
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl StageResponse {
    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    pub fn problem(rejection: &StageRejection) -> Self {
        let body = serde_json::to_vec(&json!({
            "type": "about:blank",
            "title": "Request rejected",
            "status": rejection.status,
            "code": rejection.code,
        }))
        .unwrap_or_else(|_| b"{\"title\":\"Request rejected\"}".to_vec());
        let headers = rejection
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .chain([
                ("content-type".to_owned(), "application/problem+json".to_owned()),
                ("cache-control".to_owned(), "no-store".to_owned()),
            ])
            .collect();
        Self {
            status: rejection.status,
            headers,
            body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRejection {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub headers: BTreeMap<String, String>,
}

impl StageRejection {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            headers: BTreeMap::new(),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

#[derive(Debug, Clone)]
pub enum StageDecision {
    Continue(StageInput),
    Respond(StageResponse),
    Reject(StageRejection),
}

pub trait MiddlewareStageHandler: Send + Sync {
    fn name(&self) -> &'static str;

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>>;

    fn response<'a>(
        &'a self,
        _input: &'a StageInput,
        response: StageResponse,
    ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
        Box::pin(async move { response })
    }
}

#[derive(Default)]
pub struct StagePipeline {
    stages: Vec<Arc<dyn MiddlewareStageHandler>>,
}

impl StagePipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_stage(mut self, stage: Arc<dyn MiddlewareStageHandler>) -> Self {
        self.stages.push(stage);
        self
    }

    pub fn push(&mut self, stage: Arc<dyn MiddlewareStageHandler>) {
        self.stages.push(stage);
    }

    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.iter().map(|stage| stage.name()).collect()
    }

    pub async fn execute<F, Fut>(&self, mut input: StageInput, handler: F) -> StageResponse
    where
        F: FnOnce(StageInput) -> Fut,
        Fut: Future<Output = StageResponse>,
    {
        let mut entered: Vec<(Arc<dyn MiddlewareStageHandler>, StageInput)> = Vec::new();

        for stage in &self.stages {
            let received = input.clone();
            match stage.request(input).await {
                StageDecision::Continue(next) => {
                    entered.push((Arc::clone(stage), next.clone()));
                    input = next;
                }
                StageDecision::Respond(response) => {
                    entered.push((Arc::clone(stage), received));
                    return unwind(entered, response).await;
                }
                StageDecision::Reject(rejection) => {
                    entered.push((Arc::clone(stage), received));
                    return unwind(entered, StageResponse::problem(&rejection)).await;
                }
            }
        }

        let response = handler(input).await;
        unwind(entered, response).await
    }
}

async fn unwind(
    entered: Vec<(Arc<dyn MiddlewareStageHandler>, StageInput)>,
    mut response: StageResponse,
) -> StageResponse {
    for (stage, input) in entered.into_iter().rev() {
        response = stage.response(&input, response).await;
    }
    response
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::sync::Mutex;

    use super::*;

    fn input() -> StageInput {
        StageInput::new(
            RequestMetadata {
                method: "POST".into(),
                path: "/widgets".into(),
                headers: BTreeMap::new(),
                remote_ip: Some("127.0.0.1".into()),
                content_length: None,
                transport_secure: true,
            },
            RequestContext {
                request_id: "req-1".into(),
                trace_id: "0123456789abcdef0123456789abcdef".into(),
                span_id: None,
                tenant_id: None,
                user_id: None,
                locale: None,
                started_at_unix_ms: 0,
                deadline_unix_ms: None,
                baggage: BTreeMap::new(),
            },
        )
    }

    struct RejectStage {
        calls: Arc<AtomicUsize>,
    }

    impl MiddlewareStageHandler for RejectStage {
        fn name(&self) -> &'static str {
            "reject"
        }

        fn request<'a>(
            &'a self,
            _input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                StageDecision::Reject(StageRejection::new(
                    403,
                    "rejected_for_test",
                    "request rejected",
                ))
            })
        }
    }

    struct CountStage {
        calls: Arc<AtomicUsize>,
    }

    impl MiddlewareStageHandler for CountStage {
        fn name(&self) -> &'static str {
            "count"
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { StageDecision::Continue(input) })
        }
    }

    #[tokio::test]
    async fn rejection_stops_later_stages_and_handler() {
        let reject_calls = Arc::new(AtomicUsize::new(0));
        let later_calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = StagePipeline::new()
            .with_stage(Arc::new(RejectStage {
                calls: Arc::clone(&reject_calls),
            }))
            .with_stage(Arc::new(CountStage {
                calls: Arc::clone(&later_calls),
            }));
        let handler_counter = Arc::clone(&handler_calls);

        let response = pipeline
            .execute(input(), move |_| {
                handler_counter.fetch_add(1, Ordering::SeqCst);
                async { StageResponse::empty(200) }
            })
            .await;

        assert_eq!(response.status, 403);
        assert_eq!(reject_calls.load(Ordering::SeqCst), 1);
        assert_eq!(later_calls.load(Ordering::SeqCst), 0);
        assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("application/problem+json")
        );
        assert!(!String::from_utf8_lossy(&response.body).contains("request rejected"));
    }

    struct FinalizerStage {
        name: &'static str,
        order: Arc<Mutex<Vec<&'static str>>>,
    }

    impl MiddlewareStageHandler for FinalizerStage {
        fn name(&self) -> &'static str {
            self.name
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            Box::pin(async move { StageDecision::Continue(input) })
        }

        fn response<'a>(
            &'a self,
            _input: &'a StageInput,
            response: StageResponse,
        ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
            Box::pin(async move {
                self.order.lock().await.push(self.name);
                response
            })
        }
    }

    #[tokio::test]
    async fn response_finalizers_unwind_in_reverse_entry_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let pipeline = StagePipeline::new()
            .with_stage(Arc::new(FinalizerStage {
                name: "first",
                order: Arc::clone(&order),
            }))
            .with_stage(Arc::new(FinalizerStage {
                name: "second",
                order: Arc::clone(&order),
            }))
            .with_stage(Arc::new(FinalizerStage {
                name: "third",
                order: Arc::clone(&order),
            }));

        let response = pipeline
            .execute(input(), |_| async { StageResponse::empty(204) })
            .await;

        assert_eq!(response.status, 204);
        assert_eq!(*order.lock().await, vec!["third", "second", "first"]);
    }

    struct AttributeStage;

    impl MiddlewareStageHandler for AttributeStage {
        fn name(&self) -> &'static str {
            "attribute"
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            Box::pin(async move {
                StageDecision::Continue(input.with_attribute("auth.level", json!("strong")))
            })
        }
    }

    #[tokio::test]
    async fn continued_stage_can_return_updated_typed_context() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(AttributeStage));
        let response = pipeline
            .execute(input(), |input| async move {
                assert_eq!(input.attributes.get("auth.level"), Some(&json!("strong")));
                StageResponse::empty(200)
            })
            .await;
        assert_eq!(response.status, 200);
    }
}
