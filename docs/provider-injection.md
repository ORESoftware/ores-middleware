# Consumer-owned provider injection

`ores-middleware` does **not** select or pin a concrete authentication SDK for downstream services. The consuming application owns the provider library/version, client construction, SDK-to-ORES mapping, enabled middleware, and exact route/server composition.

`ores-middleware` owns narrow provider ports, closure adapters, framework adapters, validation helpers, sanitization, and reusable middleware primitives.

## Choose static or dynamic dispatch by boundary

There is no repository-wide rule that `dyn` is undesirable. Use the representation that best matches the boundary:

- keep a concrete type when the provider is naturally known at compile time and doing so keeps the API simple;
- use `dyn` for heterogeneous runtime registries, optional/plugin-like integrations, configuration-selected implementations, or stable object-safe compatibility ports;
- do not duplicate security policy merely to avoid type erasure;
- do not box Tower/Axum services when the framework's concrete layer composes cleanly without it.

The current Rust auth surfaces intentionally support both styles.

```text
consumer-owned SDK
      |
      v
 auth_provider_fn(...)
      |
      +--> StaticAuthVerifier --> AuthStage<P, E>
      |                         --> AuthLayerState<P>
      |
      +--> AuthVerifier --------> dyn_auth_provider(...)
                                  --> MiddlewareStack

heterogeneous StagePipeline
      -> object-safe MiddlewareStageHandler boundary by design

Shared Auth paired policy
      -> SharedAuthReadyStack / centralized shared_auth.rs policy
      -> dynamic provider boundary is acceptable here to avoid policy duplication
```

## Generic auth provider adapter

`auth_provider_fn(...)` converts an async closure into a concrete `FnAuthProvider<F>`. It implements both the static `StaticAuthVerifier` port and the object-safe `AuthVerifier` compatibility port.

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

`AuthStage` sanitizes public failures and establishes stable user/tenant context. Provider-specific diagnostics are not copied into public responses.

`AuthStage::evaluate(...)` is the direct typed path. When an `AuthStage` is inserted into `StagePipeline`, the stage is intentionally erased behind the object-safe heterogeneous stage interface.

## Standalone Axum composition

The standalone Axum primitive keeps the provider type concrete because that fits Axum's composition model naturally. Keep Axum's concrete `FromFnLayer` in the consumer expression rather than hiding it behind an ORES-owned boxed service.

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

The same concrete layer can sit inside a consumer-owned `tower::ServiceBuilder`; the consuming service still chooses ordering.

## Dynamic compatibility with `MiddlewareStack`

`dyn_auth_provider(...)` converts a provider into the object-safe `Arc<dyn AuthVerifier>` boundary used by the bundled `MiddlewareStack` lifecycle.

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

This is a valid architecture, not a fallback of last resort. The wrapper also sanitizes provider failures before handing them to the bundled stack: raw SDK messages are not exposed because they may contain tokens, key IDs, tenant data, or other sensitive/high-cardinality diagnostics.

## Shared Auth dual-provider composition

`shared_auth_provider_fn(...)` adapts a consumer-owned Supabase, Neon, or other reviewed provider implementation into the Shared Auth provider contract. It exposes a concrete static port for direct calls and the object-safe `SharedAuthProviderVerifier` port used by the paired Shared Auth stack.

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

Apply the same adapter pattern to Neon. The two providers are reconciled by the centralized `shared_auth.rs` policy through `SharedAuthReadyStack`. That boundary intentionally permits dynamic dispatch so strict-paired/availability-first evidence rules, identity agreement, admin fail-closed behavior, and provider-outage handling remain implemented in one place.

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

Each version receives its own adapter and can be attached to a different route/router without changing the `ores-middleware` contract.

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
2. Prefer concrete/generic composition when it is naturally simpler; prefer `dyn` when runtime heterogeneity or a stable object-safe boundary makes it simpler.
3. Keep paired Shared Auth security policy centralized rather than duplicating it to preserve static dispatch.
4. Pin exact versions or immutable Git revisions when determinism is required.
5. Map provider-specific values into stable ORES types at the adapter boundary.
6. Sanitize raw provider errors before public responses or low-cardinality telemetry.
7. Do not leak provider SDK types into route/business interfaces.
8. Keep credentials and secrets outside descriptors and logs.
9. Consumers own middleware selection, ordering, and route/server scope.
10. Use `StagePipeline` and `MiddlewareStack` as intentional dynamic boundaries where their runtime flexibility is useful.
