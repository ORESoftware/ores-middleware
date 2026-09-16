# Consumer-owned provider injection

`ores-middleware` does **not** select or pin a concrete authentication SDK for downstream services. The consuming application owns the provider library/version, client construction, SDK-to-ORES mapping, enabled middleware, and exact route/server composition.

`ores-middleware` owns narrow provider ports, closure adapters, framework adapters, validation helpers, sanitization, and reusable middleware primitives.

## One auth provider interface

Rust consumers use the existing object-safe `AuthVerifier` port for normal auth providers and `SharedAuthProviderVerifier` for the stricter Shared Auth provider evidence boundary. There is no parallel static-auth trait family.

This is intentional. Authentication is naturally a runtime integration boundary: services may select providers by configuration, use different providers on different route trees, migrate between SDK versions, or hand the same provider to the bundled `MiddlewareStack`. The small boxed-future/dynamic-dispatch cost is negligible next to typical authentication I/O and cryptographic work, while a single provider interface keeps the public API substantially simpler.

Generics are still used where they improve ergonomics without creating a second conceptual API—for example, `auth_provider_fn(...)` captures a concrete consumer closure, and `AuthStage` keeps its optional decision-enricher callback generic.

```text
consumer-owned auth SDK/version
          |
          v
  auth_provider_fn(...)
          |
          v
      AuthVerifier
       /      |       \
      v       v        v
 AuthStage   Axum   dyn_auth_provider(...)
                      |
                      v
               MiddlewareStack

Shared Auth SDKs
      |
      v
shared_auth_provider_fn(...)
      |
      v
SharedAuthProviderVerifier
      |
      v
SharedAuthReadyStack / centralized paired-provider policy
```

`StagePipeline` is also intentionally object-safe because it is a heterogeneous runtime registry. Do not duplicate middleware/security policy merely to avoid a `dyn` boundary.

## Generic closure adapter, stable provider port

`auth_provider_fn(...)` converts an async closure into a concrete `FnAuthProvider<F>` that implements `AuthVerifier`.

```rust
use std::collections::BTreeMap;
use ores_middleware::{
    AuthDecision, AuthStage, IntegrationError, RequestMetadata,
    auth_provider_fn,
};

let auth_client = my_auth_sdk::Client::new(auth_config);

let auth = auth_provider_fn(move |request: RequestMetadata| {
    let auth_client = auth_client.clone();

    async move {
        let token = request
            .headers
            .get("authorization")
            .cloned()
            .ok_or_else(|| IntegrationError {
                code: "missing_auth",
                message: "authorization header is required".into(),
            })?;

        let identity = auth_client
            .verify(token)
            .await
            .map_err(|error| IntegrationError {
                code: "invalid_auth",
                message: error.to_string(),
            })?;

        Ok(AuthDecision {
            user_id: Some(identity.user_id),
            tenant_id: identity.tenant_id,
            claims: BTreeMap::new(),
        })
    }
});

let auth_stage = AuthStage::from_provider("company-auth", auth);
```

`AuthStage` stores the provider behind `Arc<dyn AuthVerifier>`, sanitizes public failures, and establishes stable user/tenant context. Provider-specific diagnostics are not copied into public responses.

If an application already owns an `Arc<dyn AuthVerifier>`, use `AuthStage::from_shared(...)` instead of wrapping or cloning the concrete implementation again.

## Standalone Axum composition

The standalone Axum primitive uses a stable, non-generic `AuthLayerState` containing `Arc<dyn AuthVerifier>`. Provider changes therefore do not change the router state type.

Keep Axum's concrete `FromFnLayer` in the consumer expression rather than additionally boxing the Tower service itself:

```rust
use axum::{middleware, Router};
use ores_middleware::frameworks::axum_composable::{
    AuthLayerState, authenticate,
};

let auth_state = AuthLayerState::from_provider(auth);

let protected = Router::new()
    .route("/account", account_route)
    .layer(middleware::from_fn_with_state(
        auth_state,
        authenticate,
    ));
```

A runtime-selected provider handle is equally valid:

```rust
let auth_state = AuthLayerState::from_shared(selected_provider);
```

The resulting Axum layer may also sit inside a consumer-owned `tower::ServiceBuilder`; the service still controls exact middleware order.

## Sanitizing provider boundary for `MiddlewareStack`

`dyn_auth_provider(...)` converts a concrete provider into the `Arc<dyn AuthVerifier>` handle used by the bundled `MiddlewareStack`, while also sanitizing provider failures before the older stack can expose them.

```rust
use ores_middleware::{
    MiddlewareStack, auth_provider_fn, dyn_auth_provider,
};

let provider = dyn_auth_provider(auth_provider_fn(move |request| {
    let client = client.clone();
    async move { authenticate_provider(client, request).await }
}));

let stack = MiddlewareStack::new(config)?
    .with_auth_verifier(provider);
```

Raw SDK error messages are not forwarded because they may contain tokens, key IDs, tenant data, or other sensitive/high-cardinality diagnostics. The compatibility wrapper logs only bounded metadata and returns `authentication_failed` / `authentication failed` to the stack.

`AuthStage` and the standalone Axum primitive already sanitize their public failure responses themselves, so they can accept the normal provider adapter directly.

## Shared Auth dual-provider composition

`shared_auth_provider_fn(...)` adapts a consumer-owned Supabase, Neon, or other reviewed provider implementation into `SharedAuthProviderVerifier`.

```rust
use ores_middleware::{
    RequestMetadata, SharedAuthProviderContext, SharedAuthProviderFailure,
    SharedAuthVerifiedPrincipal, shared_auth_provider_fn,
};

let supabase_client = selected_supabase_auth_version::Client::new(...);

let supabase = shared_auth_provider_fn(
    move |request: RequestMetadata, context: SharedAuthProviderContext| {
        let client = supabase_client.clone();

        async move {
            let verified = client
                .verify(request.headers.get("authorization"))
                .await
                .map_err(|error| SharedAuthProviderFailure::rejected(
                    "supabase_auth_rejected",
                    error.to_string(),
                ))?;

            Ok(SharedAuthVerifiedPrincipal::new(
                context.provider,
                verified.subject,
                verified.tenant_id,
                verified.session_id,
                context.issuer,
                context.audience,
                context.organization,
                context.data_plane,
            ))
        }
    },
);
```

Apply the same adapter pattern to Neon. The two providers are reconciled by the centralized `shared_auth.rs` policy through `SharedAuthReadyStack`. That policy remains in one place: provider/org/issuer/audience/data-plane evidence checks, strict-paired versus availability-first behavior, canonical identity agreement, and admin fail-closed behavior are not reimplemented merely to create another dispatch mode.

## Pinning provider versions belongs to the consumer

For crates.io:

```toml
[dependencies]
ores-middleware = "0.1"
my-auth-sdk = "=2.3.1"
```

For an immutable Git revision:

```toml
[dependencies]
my-auth-sdk = {
    git = "https://github.com/example/my-auth-sdk",
    rev = "0123456789abcdef0123456789abcdef01234567"
}
```

Cargo dependency renaming can keep incompatible SDK versions in one binary during migrations:

```toml
[dependencies]
auth_v1 = { package = "my-auth-sdk", version = "=1.9.4" }
auth_v2 = { package = "my-auth-sdk", version = "=2.3.1" }
```

Each version receives its own `auth_provider_fn(...)` adapter and can be attached to a different route/router without changing the ORES contract.

## Middleware order remains consumer-owned

Provider injection does not imply a universal middleware order. `StagePipeline`, direct Axum/Tower composition, `MiddlewareOrderPolicy`, and `MiddlewareCompositionPlan` let each service select and validate its own sequence.

```rust
use std::sync::Arc;
use ores_middleware::{AuthStage, StagePipeline};

let pipeline = StagePipeline::new()
    .with_stage(Arc::new(request_id_stage))
    .with_stage(Arc::new(AuthStage::from_provider("auth-v2", modern_auth)))
    .with_stage(Arc::new(tenant_context_stage))
    .with_stage(Arc::new(rate_limit_stage))
    .with_stage(Arc::new(telemetry_stage));
```

`DEFAULT_MIDDLEWARE_ORDER` remains an opt-in reviewed reference profile, not a mandatory global architecture.

## Design rules

1. Concrete provider SDKs belong to consuming applications, not `ores-middleware` core.
2. Use one stable object-safe auth provider port; do not introduce a second trait solely to avoid a boxed future.
3. Use `dyn` at runtime integration and heterogeneous registry boundaries when it makes composition simpler.
4. Keep paired Shared Auth security policy centralized rather than duplicating it for another dispatch mode.
5. Pin exact versions or immutable Git revisions when determinism is required.
6. Map provider-specific values into stable ORES types at the adapter boundary.
7. Sanitize raw provider errors before public responses or low-cardinality telemetry.
8. Do not leak provider SDK types into route/business interfaces.
9. Keep credentials and secrets outside descriptors and logs.
10. Consumers own middleware selection, ordering, and route/server scope.
