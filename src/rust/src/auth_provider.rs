use std::{future::Future, pin::Pin, sync::Arc};

use crate::{AuthDecision, AuthVerifier, IntegrationError, RequestMetadata};
use crate::shared_auth::{
    SharedAuthProviderContext, SharedAuthProviderFailure, SharedAuthProviderVerifier,
    SharedAuthVerifiedPrincipal,
};

/// Closure-backed authentication provider adapter.
///
/// The consuming application owns and pins the concrete authentication SDK,
/// captures that client in the closure, and maps SDK-specific values into the
/// stable ORES `AuthDecision` / `IntegrationError` boundary. The adapter itself
/// implements the existing object-safe [`AuthVerifier`] port so there is one
/// provider interface across standalone composition and `MiddlewareStack`.
pub struct FnAuthProvider<F> {
    f: F,
}

impl<F> FnAuthProvider<F> {
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self { f }
    }
}

#[must_use]
pub fn auth_provider_fn<F, Fut>(f: F) -> FnAuthProvider<F>
where
    F: Fn(RequestMetadata) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static,
{
    FnAuthProvider::new(f)
}

impl<F, Fut> AuthVerifier for FnAuthProvider<F>
where
    F: Fn(RequestMetadata) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static,
{
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin((self.f)(request.clone()))
    }
}

/// Compatibility wrapper used when a provider is handed to APIs that expose
/// provider errors too directly (notably the bundled `MiddlewareStack`).
struct SanitizingAuthProvider<P> {
    inner: P,
}

impl<P> SanitizingAuthProvider<P> {
    const fn new(inner: P) -> Self {
        Self { inner }
    }
}

impl<P> AuthVerifier for SanitizingAuthProvider<P>
where
    P: AuthVerifier,
{
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(async move {
            match self.inner.verify(request).await {
                Ok(decision) => Ok(decision),
                Err(error) => {
                    tracing::warn!(
                        code = error.code,
                        method = %request.method,
                        path = %request.path,
                        "authentication provider rejected request"
                    );
                    Err(IntegrationError {
                        code: "authentication_failed",
                        message: "authentication failed".into(),
                    })
                }
            }
        })
    }
}

/// Convert any concrete auth provider into the sanitizing object-safe provider
/// handle used by the bundled middleware stack.
///
/// `dyn` is intentional here: auth implementations are runtime-pluggable and the
/// stack should not become generic over every integration it owns.
#[must_use]
pub fn dyn_auth_provider<P>(provider: P) -> Arc<dyn AuthVerifier>
where
    P: AuthVerifier + 'static,
{
    Arc::new(SanitizingAuthProvider::new(provider))
}

/// Closure-backed adapter for the stricter Shared Auth provider boundary.
pub struct FnSharedAuthProvider<F> {
    f: F,
}

impl<F> FnSharedAuthProvider<F> {
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self { f }
    }
}

#[must_use]
pub fn shared_auth_provider_fn<F, Fut>(f: F) -> FnSharedAuthProvider<F>
where
    F: Fn(RequestMetadata, SharedAuthProviderContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
        + Send
        + 'static,
{
    FnSharedAuthProvider::new(f)
}

impl<F, Fut> SharedAuthProviderVerifier for FnSharedAuthProvider<F>
where
    F: Fn(RequestMetadata, SharedAuthProviderContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
        + Send
        + 'static,
{
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
        context: &'a SharedAuthProviderContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin((self.f)(request.clone(), context.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::shared_auth::{
        SharedAuthDataPlane, SharedAuthProvider, SharedAuthProviderFailureKind,
    };

    #[derive(Clone)]
    struct FakeAuthV1 {
        prefix: &'static str,
    }

    impl FakeAuthV1 {
        async fn verify(&self, token: String) -> Result<String, &'static str> {
            token
                .strip_prefix(self.prefix)
                .map(ToOwned::to_owned)
                .ok_or("invalid-v1-token")
        }
    }

    #[derive(Clone)]
    struct FakeAuthV2 {
        prefix: &'static str,
    }

    impl FakeAuthV2 {
        async fn authenticate(&self, token: String) -> Result<(String, String), &'static str> {
            token
                .strip_prefix(self.prefix)
                .map(|subject| (subject.to_owned(), "tenant-v2".to_owned()))
                .ok_or("invalid-v2-token")
        }
    }

    fn request(token: &str) -> RequestMetadata {
        RequestMetadata {
            method: "GET".into(),
            path: "/account".into(),
            headers: BTreeMap::from([("authorization".into(), token.into())]),
            remote_ip: Some("127.0.0.1".into()),
            content_length: None,
            transport_secure: true,
        }
    }

    #[tokio::test]
    async fn consumer_can_adapt_different_auth_sdk_versions_through_one_port() {
        let v1 = FakeAuthV1 { prefix: "v1:" };
        let v2 = FakeAuthV2 { prefix: "v2:" };

        let provider_v1 = auth_provider_fn(move |request: RequestMetadata| {
            let client = v1.clone();
            async move {
                let token = request.headers.get("authorization").cloned().ok_or_else(|| {
                    IntegrationError {
                        code: "missing_auth",
                        message: "authorization header is required".into(),
                    }
                })?;
                let subject = client.verify(token).await.map_err(|message| IntegrationError {
                    code: "invalid_auth_v1",
                    message: message.into(),
                })?;
                Ok(AuthDecision {
                    user_id: Some(subject),
                    tenant_id: None,
                    claims: BTreeMap::new(),
                })
            }
        });

        let provider_v2 = auth_provider_fn(move |request: RequestMetadata| {
            let client = v2.clone();
            async move {
                let token = request.headers.get("authorization").cloned().ok_or_else(|| {
                    IntegrationError {
                        code: "missing_auth",
                        message: "authorization header is required".into(),
                    }
                })?;
                let (subject, tenant_id) =
                    client.authenticate(token).await.map_err(|message| IntegrationError {
                        code: "invalid_auth_v2",
                        message: message.into(),
                    })?;
                Ok(AuthDecision {
                    user_id: Some(subject),
                    tenant_id: Some(tenant_id),
                    claims: BTreeMap::new(),
                })
            }
        });

        let v1_decision = provider_v1.verify(&request("v1:alice")).await.unwrap();
        let v2_decision = provider_v2.verify(&request("v2:bob")).await.unwrap();

        assert_eq!(v1_decision.user_id.as_deref(), Some("alice"));
        assert_eq!(v1_decision.tenant_id, None);
        assert_eq!(v2_decision.user_id.as_deref(), Some("bob"));
        assert_eq!(v2_decision.tenant_id.as_deref(), Some("tenant-v2"));
    }

    #[tokio::test]
    async fn shared_auth_closure_uses_existing_provider_port() {
        let provider = shared_auth_provider_fn(
            |request: RequestMetadata, context: SharedAuthProviderContext| async move {
                let token = request.headers.get("authorization").cloned().ok_or_else(|| {
                    SharedAuthProviderFailure {
                        kind: SharedAuthProviderFailureKind::Rejected,
                        code: "missing_auth",
                        message: "authorization header is required".into(),
                    }
                })?;
                Ok(SharedAuthVerifiedPrincipal::new(
                    context.provider,
                    token,
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
            provider: SharedAuthProvider::Supabase,
            organization: "example-org".into(),
            issuer: "https://issuer.example".into(),
            audience: "example-api".into(),
            data_plane: SharedAuthDataPlane::CustomerAuth,
        };

        let principal = provider
            .verify(&request("subject-1"), &context)
            .await
            .unwrap();

        assert_eq!(principal.provider, SharedAuthProvider::Supabase);
        assert_eq!(principal.subject, "subject-1");
        assert_eq!(principal.organization, "example-org");
    }

    #[tokio::test]
    async fn type_erased_compatibility_provider_sanitizes_sdk_failures() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Err(IntegrationError {
                code: "provider_secret_code",
                message: "internal signing key id 12345".into(),
            })
        });
        let provider = dyn_auth_provider(provider);
        let error = provider.verify(&request("bad")).await.unwrap_err();

        assert_eq!(error.code, "authentication_failed");
        assert_eq!(error.message, "authentication failed");
        assert!(!error.message.contains("12345"));
    }

    #[test]
    fn dynamic_erasure_is_available_for_stack_boundaries() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async {
            Ok(AuthDecision::default())
        });
        let _provider: Arc<dyn AuthVerifier> = dyn_auth_provider(provider);
    }
}
