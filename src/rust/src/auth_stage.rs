use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    AuthDecision, AuthVerifier, IntegrationError,
    stage::{MiddlewareStageHandler, StageDecision, StageInput, StageRejection},
};

/// Consumer hook for copying a reviewed subset of authentication decision data
/// into generic stage attributes.
pub trait AuthDecisionEnricher: Send + Sync {
    fn enrich(&self, input: StageInput, decision: &AuthDecision) -> StageInput;
}

impl<F> AuthDecisionEnricher for F
where
    F: Fn(StageInput, &AuthDecision) -> StageInput + Send + Sync,
{
    fn enrich(&self, input: StageInput, decision: &AuthDecision) -> StageInput {
        self(input, decision)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopAuthDecisionEnricher;

impl AuthDecisionEnricher for NoopAuthDecisionEnricher {
    fn enrich(&self, input: StageInput, _decision: &AuthDecision) -> StageInput {
        input
    }
}

/// Framework-neutral authentication stage.
///
/// Authentication is an integration/plugin boundary, so the provider is stored
/// as `Arc<dyn AuthVerifier>`. That keeps the stage type stable when a service
/// selects or swaps providers at runtime. The decision enricher remains generic
/// because it is normally a small consumer-owned closure and does not need a
/// second boxed callback boundary.
pub struct AuthStage<E = NoopAuthDecisionEnricher> {
    name: &'static str,
    verifier: Arc<dyn AuthVerifier>,
    decision_enricher: E,
}

impl AuthStage<NoopAuthDecisionEnricher> {
    #[must_use]
    pub fn from_provider<P>(name: &'static str, provider: P) -> Self
    where
        P: AuthVerifier + 'static,
    {
        Self::from_shared(name, Arc::new(provider))
    }

    #[must_use]
    pub fn from_shared(name: &'static str, provider: Arc<dyn AuthVerifier>) -> Self {
        Self {
            name,
            verifier: provider,
            decision_enricher: NoopAuthDecisionEnricher,
        }
    }
}

impl<E> AuthStage<E>
where
    E: AuthDecisionEnricher,
{
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub fn provider(&self) -> &(dyn AuthVerifier + 'static) {
        self.verifier.as_ref()
    }

    /// Let the consumer map selected, reviewed auth decision data into
    /// [`StageInput::attributes`] without allocating a dynamic callback.
    ///
    /// By default arbitrary claims are *not* copied into generic attributes or
    /// logs. The stage establishes user/tenant context and copies only `otel.*`
    /// claims into request baggage, matching the bundled stack behavior.
    #[must_use]
    pub fn with_decision_enricher<F>(self, enricher: F) -> AuthStage<F>
    where
        F: AuthDecisionEnricher,
    {
        AuthStage {
            name: self.name,
            verifier: self.verifier,
            decision_enricher: enricher,
        }
    }

    /// Evaluate this stage directly without registering it in the heterogeneous
    /// [`crate::StagePipeline`].
    pub async fn evaluate(&self, input: StageInput) -> StageDecision {
        match self.verifier.verify(&input.request).await {
            Ok(decision) => StageDecision::Continue(Box::new(self.apply_decision(input, &decision))),
            Err(error) => reject_auth(self.name, error),
        }
    }

    fn apply_decision(&self, mut input: StageInput, decision: &AuthDecision) -> StageInput {
        input.context.user_id = decision.user_id.clone();
        input.context.tenant_id = decision.tenant_id.clone();
        input.context.baggage.extend(
            decision
                .claims
                .iter()
                .filter(|(key, _)| key.starts_with("otel."))
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        self.decision_enricher.enrich(input, decision)
    }
}

impl<E> MiddlewareStageHandler for AuthStage<E>
where
    E: AuthDecisionEnricher + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(self.evaluate(input))
    }
}

fn reject_auth(stage_name: &'static str, error: IntegrationError) -> StageDecision {
    tracing::warn!(
        stage = stage_name,
        code = error.code,
        "authentication provider rejected request"
    );
    StageDecision::Reject(StageRejection::new(
        401,
        "authentication_failed",
        "authentication failed",
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::{
        RequestContext, RequestMetadata, StagePipeline, StageResponse, auth_provider_fn,
    };

    fn input(token: &str) -> StageInput {
        StageInput::new(
            RequestMetadata {
                method: "GET".into(),
                path: "/account".into(),
                headers: BTreeMap::from([("authorization".into(), token.into())]),
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

    #[tokio::test]
    async fn direct_evaluate_uses_the_same_provider_port_as_the_stack() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Ok(AuthDecision {
                user_id: Some("direct-user".into()),
                tenant_id: Some("direct-tenant".into()),
                claims: BTreeMap::new(),
            })
        });
        let auth = AuthStage::from_provider("direct-auth", provider);

        match auth.evaluate(input("token")).await {
            StageDecision::Continue(input) => {
                assert_eq!(input.context.user_id.as_deref(), Some("direct-user"));
                assert_eq!(input.context.tenant_id.as_deref(), Some("direct-tenant"));
            }
            StageDecision::Reject(rejection) => panic!("unexpected rejection: {}", rejection.code),
            StageDecision::Respond(response) => {
                panic!("unexpected direct response: {}", response.status)
            }
        }
    }

    #[tokio::test]
    async fn shared_dynamic_provider_handle_can_be_used_directly() {
        let provider: Arc<dyn AuthVerifier> = Arc::new(auth_provider_fn(
            |_request: RequestMetadata| async {
                Ok(AuthDecision {
                    user_id: Some("shared-user".into()),
                    tenant_id: None,
                    claims: BTreeMap::new(),
                })
            },
        ));
        let auth = AuthStage::from_shared("shared-auth", provider);

        match auth.evaluate(input("token")).await {
            StageDecision::Continue(input) => {
                assert_eq!(input.context.user_id.as_deref(), Some("shared-user"));
            }
            StageDecision::Reject(rejection) => panic!("unexpected rejection: {}", rejection.code),
            StageDecision::Respond(response) => {
                panic!("unexpected direct response: {}", response.status)
            }
        }
    }

    #[tokio::test]
    async fn injected_provider_establishes_principal_at_consumer_selected_stage() {
        #[derive(Clone)]
        struct PinnedSdkV9;

        impl PinnedSdkV9 {
            async fn verify(&self, token: &str) -> Result<String, &'static str> {
                token
                    .strip_prefix("v9:")
                    .map(ToOwned::to_owned)
                    .ok_or("bad token")
            }
        }

        let sdk = PinnedSdkV9;
        let provider = auth_provider_fn(move |request: RequestMetadata| {
            let sdk = sdk.clone();
            async move {
                let token = request
                    .headers
                    .get("authorization")
                    .ok_or_else(|| IntegrationError {
                        code: "missing_auth",
                        message: "missing token".into(),
                    })?;
                let subject = sdk.verify(token).await.map_err(|message| IntegrationError {
                    code: "provider_rejected",
                    message: message.into(),
                })?;
                Ok(AuthDecision {
                    user_id: Some(subject),
                    tenant_id: Some("tenant-v9".into()),
                    claims: BTreeMap::from([
                        ("otel.auth_method".into(), "sdk-v9".into()),
                        ("secret.internal_claim".into(), "must-not-auto-copy".into()),
                    ]),
                })
            }
        });

        let auth = AuthStage::from_provider("company-auth-v9", provider);
        let pipeline = StagePipeline::new().with_stage(Arc::new(auth));

        assert_eq!(pipeline.stage_names(), vec!["company-auth-v9"]);
        let response = pipeline
            .execute(input("v9:alice"), |input| async move {
                assert_eq!(input.context.user_id.as_deref(), Some("alice"));
                assert_eq!(input.context.tenant_id.as_deref(), Some("tenant-v9"));
                assert_eq!(
                    input.context.baggage.get("otel.auth_method").map(String::as_str),
                    Some("sdk-v9")
                );
                assert!(!input.attributes.contains_key("secret.internal_claim"));
                StageResponse::empty(204)
            })
            .await;

        assert_eq!(response.status, 204);
    }

    #[tokio::test]
    async fn consumer_can_use_concrete_enricher_without_dynamic_callback() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Ok(AuthDecision {
                user_id: Some("alice".into()),
                tenant_id: None,
                claims: BTreeMap::from([("role".into(), "admin".into())]),
            })
        });
        let auth = AuthStage::from_provider("auth", provider).with_decision_enricher(
            |input: StageInput, decision: &AuthDecision| {
                decision.claims.get("role").map_or(input.clone(), |role| {
                    input.with_attribute("authorization.role", json!(role))
                })
            },
        );
        let pipeline = StagePipeline::new().with_stage(Arc::new(auth));

        let response = pipeline
            .execute(input("token"), |input| async move {
                assert_eq!(
                    input.attributes.get("authorization.role"),
                    Some(&json!("admin"))
                );
                StageResponse::empty(200)
            })
            .await;
        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn provider_failure_is_generic_and_does_not_leak_sdk_diagnostic() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Err(IntegrationError {
                code: "provider_secret_code",
                message: "internal key id 12345".into(),
            })
        });
        let pipeline = StagePipeline::new().with_stage(Arc::new(AuthStage::from_provider(
            "auth",
            provider,
        )));

        let response = pipeline
            .execute(input("bad"), |_| async { StageResponse::empty(200) })
            .await;
        let body = String::from_utf8(response.body).expect("utf-8 problem body");
        assert_eq!(response.status, 401);
        assert!(body.contains("authentication_failed"));
        assert!(!body.contains("provider_secret_code"));
        assert!(!body.contains("12345"));
    }
}
