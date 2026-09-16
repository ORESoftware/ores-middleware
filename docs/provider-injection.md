# Consumer-owned provider injection

`ores-middleware` does **not** select or pin a concrete authentication SDK for downstream services. The consuming application owns:

- which auth library/provider is used;
- the exact dependency version or immutable Git revision;
- construction and lifecycle of the concrete auth client;
- how provider-specific success/error values map into the stable ORES auth contract;
- which middleware is enabled and the exact middleware order.

`ores-middleware` owns the narrow provider ports, closure adapters, framework adapters, validation helpers, and middleware primitives.

## Why this boundary exists

If `ores-middleware` directly depended on one auth SDK/version, every consumer would inherit that version choice and upgrades would become coupled to the middleware crate. Instead, consumers inject a concrete client through a stable adapter.

```text
consumer Cargo.toml
    |
    +-- auth-sdk v1.x --------+
    |                         |
    +-- auth-sdk v2.x -----+  |
                           |  |
                           v  v
                    consumer closures
                           |
                           v
               ores_middleware::AuthVerifier
                           |
                 +---------+----------+
                 |                    |
                 v                    v
             AuthStage       standalone Axum layer
                 |
                 +-- compatibility: MiddlewareStack
```

The concrete SDK types never become part of the `ores-middleware` public API.

## Generic auth provider adapter

`auth_provider_fn(...)` converts an async closure into an `AuthVerifier`. The closure receives owned `RequestMetadata`, which lets it capture and use any auth client without borrowing from the middleware stack.

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

// The consuming service chooses this stage's exact position.
let auth_stage = AuthStage::from_provider("company-auth", auth);
```

`AuthStage` sanitizes public failures itself and establishes stable user/tenant context. Provider-specific error messages are not copied to the public response.

## Compatibility with `MiddlewareStack`

`dyn_auth_provider(...)` is the recommended compatibility boundary when a caller needs an `Arc<dyn AuthVerifier>` for the older bundled `MiddlewareStack` API:

```rust
use ores_middleware::{
    MiddlewareStack, auth_provider_fn, dyn_auth_provider,
};

let provider = dyn_auth_provider(auth_provider_fn(move |request| {
    let client = client.clone();
    async move {
        // Map this concrete SDK/version into AuthDecision / IntegrationError.
        authenticate(client, request).await
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

Each version gets its own adapter:

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

The stricter Shared Auth path has the same dependency-injection model. `shared_auth_provider_fn(...)` adapts a closure to `SharedAuthProviderVerifier`.

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

The same pattern applies independently to the Neon verifier. `SharedAuthReadyStack::new(...)` accepts the resulting concrete adapters; it never needs to know the underlying provider crate version.

## Middleware order remains consumer-owned

Provider injection does not imply a canonical middleware order. A consuming Axum service chooses both middleware types and ordering. Prefer explicit route/router composition or the framework-neutral `StagePipeline`.

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

Different routers can use different providers or provider versions:

```rust
let legacy = Router::new()
    .route("/legacy", legacy_route)
    .layer(legacy_auth_layer);

let modern = Router::new()
    .route("/account", account_route)
    .layer(modern_auth_layer);

let app = Router::new()
    .merge(legacy)
    .merge(modern);
```

This is useful for migrations, admin/customer separation, tenant-specific identity systems, and compatibility windows.

## Design rules

1. Concrete provider SDKs are dependencies of the consumer, not `ores-middleware` core.
2. Pin exact versions or immutable Git revisions when determinism is required.
3. Map provider-specific success values into stable ORES types at the adapter boundary.
4. Keep raw provider failure diagnostics inside the adapter boundary; public middleware failures are sanitized.
5. Do not leak provider SDK types into route/business interfaces.
6. Keep credentials and secrets outside provider descriptors and logs.
7. Consumers own middleware selection/order; provider adapters are composable primitives.
8. Prefer a small adapter closure first; use a named newtype/struct when the mapping becomes substantial or needs dedicated tests.
