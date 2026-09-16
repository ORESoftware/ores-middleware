use std::collections::BTreeMap;

use ores_middleware::{
    AuthDecision, AuthStage, AuthVerifier, IntegrationError, RequestMetadata, SharedAuthDataPlane,
    SharedAuthProvider, SharedAuthProviderContext, SharedAuthProviderFailure,
    SharedAuthProviderFailureKind, SharedAuthProviderVerifier, SharedAuthVerifiedPrincipal,
    auth_provider_fn, dyn_auth_provider, shared_auth_provider_fn,
};

fn request(token: &str) -> RequestMetadata {
    RequestMetadata {
        method: "GET".into(),
        path: "/me".into(),
        headers: BTreeMap::from([("authorization".into(), token.into())]),
        remote_ip: Some("127.0.0.1".into()),
        content_length: None,
        transport_secure: true,
    }
}

#[tokio::test]
async fn public_auth_provider_adapter_is_sdk_agnostic() {
    #[derive(Clone)]
    struct ConsumerPinnedSdk {
        accepted_prefix: &'static str,
    }

    let sdk = ConsumerPinnedSdk {
        accepted_prefix: "sdk-v7:",
    };

    let provider = auth_provider_fn(move |request: RequestMetadata| {
        let sdk = sdk.clone();
        async move {
            let token = request
                .headers
                .get("authorization")
                .cloned()
                .ok_or_else(|| IntegrationError {
                    code: "missing_auth",
                    message: "authorization header is required".into(),
                })?;
            let subject = token
                .strip_prefix(sdk.accepted_prefix)
                .map(ToOwned::to_owned)
                .ok_or_else(|| IntegrationError {
                    code: "invalid_auth",
                    message: "provider rejected token".into(),
                })?;

            Ok(AuthDecision {
                user_id: Some(subject),
                tenant_id: None,
                claims: BTreeMap::new(),
            })
        }
    });

    let decision = provider.verify(&request("sdk-v7:alice")).await.unwrap();
    assert_eq!(decision.user_id.as_deref(), Some("alice"));
}

#[tokio::test]
async fn public_shared_auth_adapter_uses_existing_provider_port() {
    let provider = shared_auth_provider_fn(
        |request: RequestMetadata, context: SharedAuthProviderContext| async move {
            let subject = request
                .headers
                .get("authorization")
                .cloned()
                .ok_or_else(|| SharedAuthProviderFailure {
                    kind: SharedAuthProviderFailureKind::Rejected,
                    code: "missing_auth",
                    message: "authorization header is required".into(),
                })?;

            Ok(SharedAuthVerifiedPrincipal::new(
                context.provider,
                subject,
                "tenant-1",
                "session-1",
                context.issuer,
                context.audience,
                context.organization,
                context.data_plane,
            ))
        },
    );

    let context = SharedAuthProviderContext {
        provider: SharedAuthProvider::Neon,
        organization: "example-org".into(),
        issuer: "https://neon.example".into(),
        audience: "example-api".into(),
        data_plane: SharedAuthDataPlane::CustomerAuth,
    };

    let principal = provider
        .verify(&request("bob"), &context)
        .await
        .unwrap();

    assert_eq!(principal.provider, SharedAuthProvider::Neon);
    assert_eq!(principal.subject, "bob");
}

#[tokio::test]
async fn same_provider_can_feed_auth_stage_or_dynamic_stack_boundary() {
    let stage_provider = auth_provider_fn(|_request: RequestMetadata| async {
        Ok(AuthDecision {
            user_id: Some("stage-user".into()),
            tenant_id: None,
            claims: BTreeMap::new(),
        })
    });
    let stage = AuthStage::from_provider("auth", stage_provider);
    assert_eq!(stage.name(), "auth");

    let stack_provider = auth_provider_fn(|_request: RequestMetadata| async {
        Ok(AuthDecision {
            user_id: Some("legacy-user".into()),
            tenant_id: None,
            claims: BTreeMap::new(),
        })
    });
    let provider = dyn_auth_provider(stack_provider);
    let decision = provider.verify(&request("legacy")).await.unwrap();
    assert_eq!(decision.user_id.as_deref(), Some("legacy-user"));
}
