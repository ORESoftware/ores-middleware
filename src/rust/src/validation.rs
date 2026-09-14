use std::{future::Future, pin::Pin, sync::Arc};

use crate::stage::{MiddlewareStageHandler, StageDecision, StageInput, StageRejection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractViolation {
    pub path: String,
    pub code: String,
}

pub trait RequestContractValidator: Send + Sync {
    fn validate<'a>(
        &'a self,
        input: &'a StageInput,
    ) -> Pin<Box<dyn Future<Output = Result<(), Vec<ContractViolation>>> + Send + 'a>>;
}

pub struct ValidationStage {
    validator: Arc<dyn RequestContractValidator>,
}

impl ValidationStage {
    pub fn new(validator: Arc<dyn RequestContractValidator>) -> Self {
        Self { validator }
    }
}

impl MiddlewareStageHandler for ValidationStage {
    fn name(&self) -> &'static str {
        "request-contract-validation"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            match self.validator.validate(&input).await {
                Ok(()) => StageDecision::Continue(input),
                Err(violations) => StageDecision::Reject(
                    StageRejection::new(
                        422,
                        "request_contract_invalid",
                        "decoded request payload does not satisfy the selected contract",
                    )
                    .with_header("x-ores-validation-error-count", violations.len().to_string()),
                ),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use serde_json::json;

    use super::*;
    use crate::{
        context::RequestContext,
        integrations::RequestMetadata,
        stage::{StagePipeline, StageResponse},
    };

    fn input(payload: serde_json::Value) -> StageInput {
        StageInput::new(
            RequestMetadata {
                method: "POST".into(),
                path: "/widgets".into(),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                remote_ip: None,
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
        .with_decoded_payload(payload)
    }

    struct WidgetValidator;

    impl RequestContractValidator for WidgetValidator {
        fn validate<'a>(
            &'a self,
            input: &'a StageInput,
        ) -> Pin<Box<dyn Future<Output = Result<(), Vec<ContractViolation>>> + Send + 'a>> {
            Box::pin(async move {
                let valid = input
                    .decoded_payload
                    .as_ref()
                    .and_then(|value| value.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| !name.is_empty());
                if valid {
                    Ok(())
                } else {
                    Err(vec![ContractViolation {
                        path: "/name".into(),
                        code: "required_nonempty_string".into(),
                    }])
                }
            })
        }
    }

    #[tokio::test]
    async fn valid_decoded_payload_reaches_handler() {
        let pipeline = StagePipeline::new()
            .with_stage(Arc::new(ValidationStage::new(Arc::new(WidgetValidator))));
        let response = pipeline
            .execute(input(json!({"name":"alpha"})), |_| async {
                StageResponse::empty(201)
            })
            .await;
        assert_eq!(response.status, 201);
    }

    #[tokio::test]
    async fn invalid_payload_is_422_and_handler_never_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = StagePipeline::new()
            .with_stage(Arc::new(ValidationStage::new(Arc::new(WidgetValidator))));
        let handler_calls = Arc::clone(&calls);
        let response = pipeline
            .execute(input(json!({"name":""})), move |_| {
                handler_calls.fetch_add(1, Ordering::SeqCst);
                async { StageResponse::empty(200) }
            })
            .await;
        assert_eq!(response.status, 422);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            response
                .headers
                .get("x-ores-validation-error-count")
                .map(String::as_str),
            Some("1")
        );
        let body = String::from_utf8_lossy(&response.body);
        assert!(body.contains("request_contract_invalid"));
        assert!(!body.contains("/name"));
        assert!(!body.contains("required_nonempty_string"));
    }
}
