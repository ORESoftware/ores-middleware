use std::{future::Future, pin::Pin};

use crate::{
    AuthDecision, IntegrationError, StaticAuthVerifier,
    stage::{MiddlewareStageHandler, StageDecision, StageInput, StageRejection},
};

/// Consumer hook for copying a reviewed subset of auth decision data into
/// generic stage attributes or other explicitly selected request context.
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

/// Framework-neutral authentication stage backed by a concrete provider type.
///
/// `P` remains concrete. Type erasure occurs only if the consumer inserts this
/// stage into the heterogeneous `StagePipeline` registry.
pub struct AuthStage<P, E = NoopAuthDecisionEnricher> {
    name: &'static str,
    verifier: P,
    decision_enricher: E,
}

impl<P> AuthStage<P, NoopAuthDecisionEnricher>
where
    P: StaticAuthVerifier,
{
    #[must_use]
    pub fn from_provider(name: &'static str, provider: P) -> Self {
        Self {
            name,
            verifier: provider,
            decision_enricher: NoopAuthDecisionEnricher,
        }
    }
}

impl<P, E> AuthStage<P, E>
where
    P: StaticAuthVerifier,
    E: AuthDecisionEnricher,
{
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub fn provider(&self) -> &P {
        &self.verifier
    }

    #[must_use]
    pub fn with_decision_enricher<F>(self, enricher: F) -> AuthStage<P, F>
    where
        F: AuthDecisionEnricher,
    {
        AuthStage {
            name: self.name,
            verifier: self.verifier,
            decision_enricher: enricher,
        }
    }

    pub async fn evaluate(&self, input: StageInput) -> StageDecision {
        match self.verifier.verify_owned(input.request.clone()).await {
            Ok(decision) => StageDecision::Continue(Box::new(self.apply_decision(input, &decision))),
            Err(error) => reject_auth(self.name, error),
        }
    }

    fn apply_decision(&self, mut input: StageInput, decision: &AuthDecision) -> StageInput {
        input.context.user_id = decision.user_id.clone();
        input.context.tenant_id = decision.tenant_id.clone();
        self.decision_enricher.enrich(input, decision)
    }
}

impl<P, E> MiddlewareStageHandler for AuthStage<P, E>
where
    P: StaticAuthVerifier + 'static,
    E: AuthDecisionEnricher + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        // The boxed future belongs to the heterogeneous stage-registry boundary;
        // the auth provider itself remains statically dispatched.
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
    use std::{collections::BTreeMap, sync::Arc};

    use serde_json::json;

    use super::*;
    use crate::{RequestContext, RequestMetadata, StagePipeline, StageResponse, auth_provider_fn};

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
    async fn direct_evaluate_preserves_concrete_provider() {
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
            _ => panic!("auth should continue"),
        }
    }

    #[tokio::test]
    async fn provider_claims_require_explicit_consumer_enrichment() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Ok(AuthDecision {
                user_id: Some("alice".into()),
                tenant_id: Some("tenant-a".into()),
                claims: BTreeMap::from([
                    ("role".into(), "admin".into()),
                    ("otel.secret".into(), "must-not-propagate".into()),
                ]),
            })
        });
        let auth = AuthStage::from_provider("auth", provider);

        match auth.evaluate(input("token")).await {
            StageDecision::Continue(input) => {
                assert_eq!(input.context.user_id.as_deref(), Some("alice"));
                assert_eq!(input.context.tenant_id.as_deref(), Some("tenant-a"));
                assert!(input.context.baggage.is_empty());
                assert!(input.attributes.is_empty());
            }
            _ => panic!("auth should continue"),
        }
    }

    #[tokio::test]
    async fn concrete_enricher_is_consumer_owned() {
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
                assert_eq!(input.attributes.get("authorization.role"), Some(&json!("admin")));
                StageResponse::empty(200)
            })
            .await;
        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn provider_failure_does_not_leak_diagnostic() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Err(IntegrationError {
                code: "provider_secret_code",
                message: "internal key id 12345".into(),
            })
        });
        let pipeline = StagePipeline::new().with_stage(Arc::new(AuthStage::from_provider(
            "auth", provider,
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
