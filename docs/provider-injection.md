# Consumer-owned provider injection

`ores-middleware` does **not** select or pin a concrete authentication SDK for downstream services. The consuming application owns:

- which auth library/provider is used;
- the exact dependency version or immutable Git revision;
- construction and lifecycle of the concrete auth client;
- how provider-specific success/error values map into the stable ORES auth contract;
- which middleware is enabled and the exact middleware order.

`ores-middleware` owns the narrow provider ports, closure adapters, framework adapters, validation helpers, and middleware primitives.

## Static dispatch is the primary path

New Rust composition is deliberately static-first. `auth_provider_fn(...)` returns a concrete `FnAuthProvider<F>` implementing `StaticAuthVerifier`; its future type is an associated type rather than a boxed trait-object future. `AuthStage<P, E>` retains concrete provider and decision-enricher types, and `AuthLayerState<P>` retains the concrete provider behind `Arc<P>` only for cheap cloning.

`dyn_auth_provider(...)` exists solely as an explicit compatibility bridge for older APIs that already require `Arc<dyn AuthVerifier>`. Do not type-erase a provider before that boundary.

The framework-neutral `StagePipeline` is different: it is intentionally a heterogeneous runtime registry, so the registry stores different stage types behind one object-safe stage interface. Consumers that want an entirely statically typed Tower chain can compose the standalone Axum primitive directly instead of registering it in `StagePipeline`.

```text
consumer Cargo.toml
    |
    +-- auth-sdk v1.x --------+
    |                         |
    +-- auth-sdk v2.x -----+  |
                           |  |
                           v  v
                    concrete closures
                           |
                           v
               StaticAuthVerifier
                    /            \
                   v              v
          AuthStage<P, E>    AuthLayerState<P>
                   |
                   +-- optional heterogeneous StagePipeline boundary

legacy only:
concrete provider -> dyn_auth_provider -> Arc<dyn AuthVerifier> -> MiddlewareStack
```

The concrete SDK types never become dependencies of `ores-middleware` itself.

## Generic auth provider adapter

`auth_provider_fn(...)` converts an async closure into a concrete provider implementing `StaticAuthVerifier`. The closure receives owned `RequestMetadata`, so its returned future does not borrow the middleware stack and can remain its concrete type.

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

// P remains the concrete closure-adapter type here.
let auth_stage = AuthStage::from_provider("company-auth", auth);
```

`AuthStage` sanitizes public failures itself and establishes stable user/tenant context. Provider-specific error messages are not copied to the public response.

## Standalone Axum composition without an ORES `dyn Service`

Keep Axum's concrete `FromFnLayer` in the consumer expression. This gives `Router::layer(...)` the associated `Service<Request>` information it needs and avoids an ORES-owned boxed or dynamic Tower service.

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

The same concrete layer may be placed inside a consumer-owned `tower::ServiceBuilder`; the consumer still determines ordering.

## Compatibility with `MiddlewareStack`

`dyn_auth_provider(...)` is an explicit compatibility boundary when a caller needs an `Arc<dyn AuthVerifier>` for the older bundled `MiddlewareStack` API:

```rust
use ores_middleware::{
    MiddlewareStack, auth_provider_fn, dyn_auth_provider,
};

let provider = dyn_auth_provider(auth_provider_fn(move |request| {
    let client = client.clone();
    async move {
        authenticate_provider(client, request).await
    }
}));

let stack = MiddlewareStack::new(config)?
    .with_auth_verifier(provider);
```

The compatibility wrapper deliberately sanitizes provider failures before handing them to `MiddlewareStack`: it logs only the bounded provider error code and returns `authentication_failed` / `authentication failed`. Raw SDK error messages are not logged or exposed because they can contain tokens, signing-key IDs, tenant data, or other sensitive/high-cardinality diagnostics.

New consumer-owned compositions should prefer `AuthStage` or `frameworks::axum_composable::authenticate` rather than building new dependencies on the bundled stack lifecycle.

## Pinning a specific auth version belongs to the consumer

For a crates.io dependency:

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

The consumer's `Cargo.lock` then records the resolved dependency graph. `ores-middleware` does not depend on or re-export that SDK.

## Two auth versions in one binary

Cargo dependency renaming can keep two incompatible versions available simultaneously when a migration requires it:

```toml
[dependencies]
auth_v1 = { package = "my-auth-sdk", version = "=1.9.4" }
auth_v2 = { package = "my-auth-sdk", version = "=2.3.1" }
```

Each version gets its own concrete adapter:

```rust
let legacy_client = auth_v1::Client::new(legacy_config);
let modern_client = auth_v2::Client::new(modern_config);

let legacy_auth = auth_provider_fn(move |request| {
    let client = legacy_client.clone();
    async move { adapt_v1(client, request).await }
});

let modern_auth = auth_provider_fn(move |request| {
    let client = modern_client.clone();
    async move { adapt_v2(client, request).await }
});
```

Those providers can be attached to different routers/routes or used during a staged migration without changing `ores-middleware`.

## Shared Auth dual-provider adapter

The stricter Shared Auth path follows the same static-first dependency-injection model. `shared_auth_provider_fn(...)` returns a concrete adapter implementing `StaticSharedAuthProviderVerifier` while retaining the older object-safe provider trait for compatibility.

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

The same pattern applies independently to the Neon verifier. Keep each adapter concrete for as long as the consuming surface permits it.

## Middleware order remains consumer-owned

Provider injection does not imply a canonical middleware order. A consuming Axum service chooses both middleware types and ordering. Prefer explicit route/router composition or the framework-neutral `StagePipeline` when runtime heterogeneous registration is useful.

```rust
use std::sync::Arc;
use ores_middleware::{StagePipeline, AuthStage};

let pipeline = StagePipeline::new()
    .with_stage(Arc::new(request_id_stage))
    .with_stage(Arc::new(AuthStage::from_provider("auth-v2", modern_auth)))
    .with_stage(Arc::new(tenant_context_stage))
    .with_stage(Arc::new(rate_limit_stage))
    .with_stage(Arc::new(telemetry_stage));
```

Another service may intentionally choose a different chain. `ores-middleware` exposes `MiddlewareOrderPolicy` and `MiddlewareCompositionPlan` so the consumer can validate only the invariants it owns. The legacy `DEFAULT_MIDDLEWARE_ORDER` is an opt-in reference profile, not a universal requirement.

## Route-specific providers

Different routers can use different concrete providers or provider versions:

```rust
let legacy_state = AuthLayerState::from_provider(legacy_auth);
let modern_state = AuthLayerState::from_provider(modern_auth);

let legacy = Router::new()
    .route("/legacy", legacy_route)
    .layer(middleware::from_fn_with_state(legacy_state, authenticate));

let modern = Router::new()
    .route("/account", account_route)
    .layer(middleware::from_fn_with_state(modern_state, authenticate));

let app = Router::new()
    .merge(legacy)
    .merge(modern);
```

This is useful for migrations, admin/customer separation, tenant-specific identity systems, and compatibility windows.

## Design rules

1. Concrete provider SDKs are dependencies of the consumer, not `ores-middleware` core.
2. Prefer `StaticAuthVerifier` / `StaticSharedAuthProviderVerifier`; type-erase only at a boundary that actually requires runtime heterogeneity.
3. Pin exact versions or immutable Git revisions when determinism is required.
4. Map provider-specific success values into stable ORES types at the adapter boundary.
5. Keep raw provider failure diagnostics inside the adapter boundary; public middleware failures are sanitized.
6. Do not leak provider SDK types into route/business interfaces.
7. Keep credentials and secrets outside provider descriptors and logs.
8. Consumers own middleware selection/order; provider adapters are composable primitives.
9. Prefer a small adapter closure first; use a named newtype/struct when the mapping becomes substantial or needs dedicated tests.
10. Treat `StagePipeline` and legacy `MiddlewareStack` as intentional dynamic boundaries, not as reasons to erase provider types earlier.
