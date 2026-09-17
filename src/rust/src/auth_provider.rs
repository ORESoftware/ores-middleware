use std::{future::Future, pin::Pin, sync::Arc};

use crate::shared_auth::{
    SharedAuthProviderContext, SharedAuthProviderFailure, SharedAuthProviderVerifier,
    SharedAuthVerifiedPrincipal,
};
use crate::{AuthDecision, AuthVerifier, IntegrationError, RequestMetadata};

/// Static-dispatch authentication provider boundary.
///
/// Consumers keep their concrete auth SDK and future types all the way through
/// normal middleware composition. Runtime type erasure is only needed when the
/// consumer explicitly chooses a trait object or reaches a legacy compatibility
/// boundary that requires `dyn AuthVerifier`.
pub trait StaticAuthVerifier: Send + Sync {
    type Future: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static;

    fn verify_owned(&self, request: RequestMetadata) -> Self::Future;
}

impl<P> StaticAuthVerifier for Arc<P>
where
    P: StaticAuthVerifier + ?Sized,
{
    type Future = P::Future;

    fn verify_owned(&self, request: RequestMetadata) -> Self::Future {
        self.as_ref().verify_owned(request)
    }
}

/// Allow an explicitly type-erased verifier to use every consumer-owned generic
/// composition surface. The allocation/dispatch cost remains explicit in the
/// provider type instead of forcing a separate middleware API.
impl StaticAuthVerifier for Arc<dyn AuthVerifier> {
    type Future = Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send>>;

    fn verify_owned(&self, request: RequestMetadata) -> Self::Future {
        let provider = Arc::clone(self);
        Box::pin(async move { provider.verify(&request).await })
    }
}

/// Static-dispatch Shared Auth provider boundary.
pub trait StaticSharedAuthProviderVerifier: Send + Sync {
    type Future: Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
        + Send
        + 'static;

    fn verify_owned(
        &self,
        request: RequestMetadata,
        context: SharedAuthProviderContext,
    ) -> Self::Future;
}

impl<P> StaticSharedAuthProviderVerifier for Arc<P>
where
    P: StaticSharedAuthProviderVerifier + ?Sized,
{
    type Future = P::Future;

    fn verify_owned(
        &self,
        request: RequestMetadata,
        context: SharedAuthProviderContext,
    ) -> Self::Future {
        self.as_ref().verify_owned(request, context)
    }
}

/// Dynamic Shared Auth providers can opt into the same generic composition
/// surface when runtime provider selection is useful to the application.
impl StaticSharedAuthProviderVerifier for Arc<dyn SharedAuthProviderVerifier> {
    type Future = Pin<
        Box<
            dyn Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
                + Send,
        >,
    >;

    fn verify_owned(
        &self,
        request: RequestMetadata,
        context: SharedAuthProviderContext,
    ) -> Self::Future {
        let provider = Arc::clone(self);
        Box::pin(async move { provider.verify(&request, &context).await })
    }
}

/// Closure-backed authentication provider adapter.
pub struct FnAuthProvider<F> {
    f: F,
}

impl<F> FnAuthProvider<F> {
    #[must_use]
    pub const fn new(f: F) -> Self {
        Self { f }
    }
}

/// Adapt an async closure into the static ORES auth-provider boundary.
#[must_use]
pub fn auth_provider_fn<F, Fut>(f: F) -> FnAuthProvider<F>
where
    F: Fn(RequestMetadata) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static,
{
    FnAuthProvider::new(f)
}

impl<F, Fut> StaticAuthVerifier for FnAuthProvider<F>
where
    F: Fn(RequestMetadata) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static,
{
    type Future = Fut;

    fn verify_owned(&self, request: RequestMetadata) -> Self::Future {
        (self.f)(request)
    }
}

/// Object-safe compatibility bridge for older APIs such as `MiddlewareStack`.
impl<F, Fut> AuthVerifier for FnAuthProvider<F>
where
    F: Fn(RequestMetadata) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'static,
{
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(self.verify_owned(request.clone()))
    }
}

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

/// Explicit type-erasure helper for consumers that want runtime heterogeneity or
/// compatibility with APIs that require `Arc<dyn AuthVerifier>`.
///
/// The returned provider sanitizes provider diagnostics and also implements
/// `StaticAuthVerifier`, so it can be passed to `AuthStage` and the standalone
/// Axum authentication layer without switching composition APIs.
#[must_use]
pub fn dyn_auth_provider<P>(provider: P) -> Arc<dyn AuthVerifier>
where
    P: AuthVerifier + 'static,
{
    Arc::new(SanitizingAuthProvider::new(provider))
}

/// Closure-backed adapter for the stricter paired Shared Auth boundary.
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

impl<F, Fut> StaticSharedAuthProviderVerifier for FnSharedAuthProvider<F>
where
    F: Fn(RequestMetadata, SharedAuthProviderContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<SharedAuthVerifiedPrincipal, SharedAuthProviderFailure>>
        + Send
        + 'static,
{
    type Future = Fut;

    fn verify_owned(
        &self,
        request: RequestMetadata,
        context: SharedAuthProviderContext,
    ) -> Self::Future {
        (self.f)(request, context)
    }
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
        Box::pin(self.verify_owned(request.clone(), context.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::shared_auth::{SharedAuthDataPlane, SharedAuthProvider};

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
    async fn closure_provider_uses_static_dispatch() {
        let provider = auth_provider_fn(|request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: request.headers.get("authorization").cloned(),
                tenant_id: Some("tenant-a".into()),
                claims: BTreeMap::new(),
            })
        });

        let decision = provider.verify_owned(request("alice")).await.unwrap();
        assert_eq!(decision.user_id.as_deref(), Some("alice"));
        assert_eq!(decision.tenant_id.as_deref(), Some("tenant-a"));
    }

    #[tokio::test]
    async fn shared_auth_closure_has_static_dispatch_path() {
        let provider = shared_auth_provider_fn(
            |request: RequestMetadata, context: SharedAuthProviderContext| async move {
                Ok(SharedAuthVerifiedPrincipal::new(
                    context.provider,
                    request
                        .headers
                        .get("authorization")
                        .cloned()
                        .unwrap_or_default(),
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
            .verify_owned(request("subject-1"), context)
            .await
            .unwrap();
        assert_eq!(principal.subject, "subject-1");
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
        let error = provider.verify_owned(request("bad")).await.unwrap_err();
        assert_eq!(error.code, "authentication_failed");
        assert_eq!(error.message, "authentication failed");
    }

    #[tokio::test]
    async fn explicit_dyn_provider_uses_the_static_composition_trait() {
        let provider = auth_provider_fn(|request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: request.headers.get("authorization").cloned(),
                tenant_id: Some("runtime-selected".into()),
                claims: BTreeMap::new(),
            })
        });
        let provider = dyn_auth_provider(provider);
        let decision = StaticAuthVerifier::verify_owned(&provider, request("bob"))
            .await
            .unwrap();
        assert_eq!(decision.user_id.as_deref(), Some("bob"));
        assert_eq!(decision.tenant_id.as_deref(), Some("runtime-selected"));
    }

    #[tokio::test]
    async fn dyn_shared_auth_provider_uses_the_static_composition_trait() {
        let provider = shared_auth_provider_fn(
            |request: RequestMetadata, context: SharedAuthProviderContext| async move {
                Ok(SharedAuthVerifiedPrincipal::new(
                    context.provider,
                    request
                        .headers
                        .get("authorization")
                        .cloned()
                        .unwrap_or_default(),
                    "tenant-dyn",
                    "session-dyn",
                    context.issuer,
                    context.audience,
                    context.organization,
                    context.data_plane,
                ))
            },
        );
        let provider: Arc<dyn SharedAuthProviderVerifier> = Arc::new(provider);
        let context = SharedAuthProviderContext {
            provider: SharedAuthProvider::Neon,
            organization: "example-org".into(),
            issuer: "https://issuer.example".into(),
            audience: "example-api".into(),
            data_plane: SharedAuthDataPlane::CustomerAuth,
        };
        let principal =
            StaticSharedAuthProviderVerifier::verify_owned(&provider, request("subject-dyn"), context)
                .await
                .unwrap();
        assert_eq!(principal.subject, "subject-dyn");
        assert_eq!(principal.tenant_id, "tenant-dyn");
    }
}