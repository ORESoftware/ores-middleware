# Consumer-owned provider injection

`ores-middleware` does **not** select or pin a concrete authentication SDK for downstream services. The consuming application owns the provider library/version, client construction, SDK-to-ORES mapping, enabled middleware, and exact route/server composition.

`ores-middleware` owns narrow provider ports, closure adapters, framework adapters, validation helpers, sanitization, and reusable middleware primitives.

## One composition surface, two dispatch choices

Rust consumers can choose either dispatch strategy without changing middleware APIs:

- **concrete/static dispatch** through `StaticAuthVerifier` and `StaticSharedAuthProviderVerifier`;
- **runtime/dynamic dispatch** through `Arc<dyn AuthVerifier>` and `Arc<dyn SharedAuthProviderVerifier>`.

The dynamic handles implement the corresponding static-composition adapter traits, so `AuthStage`, the standalone Axum primitive, and other generic helpers accept both choices.

This keeps dispatch policy consumer-owned. A service that knows its provider type at compile time can retain the concrete provider and concrete future type. A service that selects providers from configuration, mixes provider implementations in a runtime registry, or simply prefers the smaller type surface can use `dyn` directly.

```text
consumer-owned auth SDK/version
          |
          v
  auth_provider_fn(...)
          |
          +-----------------------------+
          |                             |
          v                             v
  StaticAuthVerifier            dyn_auth_provider(...)
  concrete provider                  |
          |                           v
          |                    Arc<dyn AuthVerifier>
          |                           |
          +-------------+-------------+
                        |
                        v
             same composition APIs
              /         |          \
             v          v           v
         AuthStage     Axum    MiddlewareStack

Shared Auth SDKs
      |
      v
shared_auth_provider_fn(...)
      |
      +-------------------------------+
      |                               |
      v                               v
StaticSharedAuthProviderVerifier  Arc<dyn SharedAuthProviderVerifier>
      |                               |
      +---------------+---------------+
                      |
                      v
          shared composition/policy
```

`StagePipeline` remains intentionally object-safe because it is a heterogeneous runtime stage registry. There is no requirement to make every internal boundary generic merely to avoid `dyn`.

## Concrete provider adapter

`auth_provider_fn(...)` converts an async closure into a concrete `FnAuthProvider<F>`. That value implements both the concrete `StaticAuthVerifier` surface and the object-safe `AuthVerifier` compatibility port.

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
            .map_err(|_| IntegrationError {
                code: "invalid_auth",
                message: "authentication failed".into(),
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

`AuthStage<P, E>` stores `P` by value. If `P` is concrete, the provider remains concrete. If `P` is `Arc<dyn AuthVerifier>`, the same stage uses dynamic dispatch. Type erasure is therefore an application choice rather than an architectural requirement.

## Explicit dynamic provider adapter

`dyn_auth_provider(...)` converts any `AuthVerifier` implementation into a sanitized `Arc<dyn AuthVerifier>`.

```rust
use ores_middleware::{
    AuthStage, RequestMetadata, auth_provider_fn, dyn_auth_provider,
};

let selected_provider = dyn_auth_provider(auth_provider_fn(
    move |request: RequestMetadata| {
        let client = runtime_selected_client.clone();
        async move { authenticate_provider(client, request).await }
    },
));

let auth_stage = AuthStage::from_provider(
    "runtime-selected-auth",
    selected_provider,
);
```

The returned dynamic provider also implements `StaticAuthVerifier`, using an owned boxed future internally. That means consumers do not switch to another middleware API just because provider selection becomes dynamic.

Raw provider error messages are sanitized by `dyn_auth_provider(...)` because they may contain credentials, key IDs, tenant data, or other sensitive/high-cardinality diagnostics. The wrapper logs bounded metadata and returns the stable `authentication_failed` error to downstream compatibility surfaces.

## Standalone Axum composition

The standalone Axum primitive is generic over the provider value. Consumers control exactly where it appears in the Tower/Axum chain.

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

The exact same surface accepts an explicit dynamic provider:

```rust
let selected_provider = dyn_auth_provider(runtime_provider);
let auth_state = AuthLayerState::from_provider(selected_provider);
```

`AuthLayerState` internally uses `Arc` for cheap framework-state cloning. This does not force the provider port itself to be dynamically dispatched. The resulting layer can also sit inside a consumer-owned `tower::ServiceBuilder`; the service still controls exact middleware order and route scope.

## Bundled `MiddlewareStack`

The older bundled `MiddlewareStack` consumes the object-safe provider port, so a dynamic handle is natural there:

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

This is not a reason to force every newer consumer onto dynamic dispatch. It is simply one runtime boundary whose shape is already heterogeneous.

## Shared Auth provider composition

`shared_auth_provider_fn(...)` adapts a consumer-owned Supabase, Neon, or other reviewed provider implementation into both the concrete `StaticSharedAuthProviderVerifier` path and the existing object-safe `SharedAuthProviderVerifier` port.

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
                .map_err(|_| SharedAuthProviderFailure::rejected(
                    "supabase_auth_rejected",
                    "authentication failed",
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

For runtime-selected Shared Auth implementations, store the provider as `Arc<dyn SharedAuthProviderVerifier>`. That handle implements `StaticSharedAuthProviderVerifier`, so generic shared composition can still call `verify_owned(...)` without introducing a second composition API.

The centralized `shared_auth.rs` policy remains authoritative for provider/org/issuer/audience/data-plane evidence checks, strict-paired versus availability-first behavior, canonical identity agreement, and admin fail-closed behavior. Dispatch strategy must not duplicate or weaken that policy.

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

Each version can receive its own `auth_provider_fn(...)` adapter and attach to a different route/router. A consumer can keep those adapters concrete or erase either one behind `dyn` if runtime selection is preferable.

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

## Dispatch guidance

Prefer the representation that matches the consumer's actual runtime shape:

- use a concrete provider when the application already knows the provider type and wants monomorphized dispatch;
- use `Arc<dyn AuthVerifier>` when providers are selected at runtime, stored heterogeneously, shared behind stable handles, or when that type is simply easier to compose;
- do not introduce extra generic layers solely to eliminate a tiny dynamic-dispatch cost around network/crypto-heavy authentication;
- do not erase provider types merely because a library helper happens to support `dyn`.

Both are supported paths. Correctness, provider isolation, explicit ordering, stable contracts, and failure sanitization matter more than mechanically maximizing or minimizing `dyn` usage.

## Design rules

1. Concrete provider SDKs belong to consuming applications, not `ores-middleware` core.
2. Keep one middleware composition surface that accepts either concrete or explicitly type-erased providers.
3. Use generics where the provider is naturally concrete; use `dyn` where runtime heterogeneity or simpler ownership makes it useful.
4. Keep paired Shared Auth security policy centralized regardless of dispatch mode.
5. Pin exact versions or immutable Git revisions when determinism is required.
6. Map provider-specific values into stable ORES types at the adapter boundary.
7. Sanitize raw provider errors before public responses or low-cardinality telemetry.
8. Do not leak provider SDK types into route/business interfaces.
9. Keep credentials and secrets outside descriptors and logs.
10. Consumers own middleware selection, ordering, and route/server scope.
