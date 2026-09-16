# Consumer-owned middleware composition

`ores-middleware` provides middleware implementations, provider ports, framework adapters, validation helpers, and composition primitives. It does **not** own the middleware selection or execution order of a consuming service.

The application or organization consuming the crate owns:

- which middleware exists on a server, router, route, or route group;
- the exact request/response ordering;
- whether a middleware is global, route-specific, or absent;
- concrete provider dependencies and versions;
- which ordering relationships are hard errors, advisories, or intentionally unconstrained.

This is important because two services can use the same ORES primitives with different semantics. An authenticated mutation API may need authentication before a principal-aware rate limiter, while a public endpoint may use only an anonymous flood guard. The library should not manufacture one global answer for both.

## Standalone Axum authentication primitive

The consuming crate pins and constructs its chosen auth SDK, adapts it through `auth_provider_fn(...)`, and places the ORES Axum middleware exactly where it wants:

```rust
use axum::{Router, middleware, routing::get};
use ores_middleware::{
    AuthDecision, IntegrationError, RequestMetadata, auth_provider_fn,
    frameworks::axum_composable::{AuthLayerState, authenticate},
};

let client = selected_auth_sdk::Client::new(auth_config);

let provider = auth_provider_fn(move |request: RequestMetadata| {
    let client = client.clone();
    async move {
        let verified = client
            .verify(request.headers.get("authorization"))
            .await
            .map_err(|error| IntegrationError {
                code: "auth_rejected",
                message: error.to_string(),
            })?;

        Ok(AuthDecision {
            user_id: Some(verified.user_id),
            tenant_id: verified.tenant_id,
            claims: Default::default(),
        })
    }
});

let auth_state = AuthLayerState::from_provider(provider);

let protected = Router::new()
    .route("/account", get(account))
    .layer(middleware::from_fn_with_state(auth_state, authenticate));

let app = Router::new()
    .route("/healthz", get(healthz))
    .merge(protected);
```

`authenticate` inserts the stable `AuthDecision` into request extensions. Provider-specific error text is logged only as permitted by the adapter and is not copied into the public `401` response.

The same provider can be placed on an entire router, one route group, or multiple independently composed routers. Different provider versions can coexist because the concrete SDK types remain in the consumer.

## Consumer-supplied order policy

`validate_consumer_middleware_order(...)` has no built-in ordering rules. Stage names are open strings rather than a closed enum, so application-specific middleware participates without changes to `ores-middleware`.

```rust
use ores_middleware::{
    MiddlewareOrderPolicy, MiddlewareOrderingRule,
    validate_consumer_middleware_order,
};

let policy = MiddlewareOrderPolicy::new()
    .require("company-auth-v2")
    .unique("tenant-rate-limit")
    .forbid("test-auth-bypass")
    .rule(MiddlewareOrderingRule::before(
        "company-auth-v2",
        "tenant-rate-limit",
        "auth-before-tenant-limit",
        "this service derives its limiter principal from authenticated identity",
    ));

let actual = [
    "request-id",
    "company-auth-v2",
    "tenant-rate-limit",
    "handler",
    "telemetry",
];

let issues = validate_consumer_middleware_order(&actual, &policy);
assert!(issues.is_empty());
```

An ordering rule is conditional by default: if one of its two stages is absent, the rule does not make that stage mandatory. Use `.require(...)` for presence or `.require_both()` on a particular relationship when absence itself is a violation.

Duplicate middleware is also allowed by default. A consumer opts into uniqueness only for stage names where duplication is semantically invalid.

## Route-specific policies

A large service does not need one policy for every route. Keep policies at the same ownership boundary as the composition they validate:

```rust
let public_policy = MiddlewareOrderPolicy::new()
    .forbid("session-auth")
    .require("anonymous-flood-guard");

let mutation_policy = MiddlewareOrderPolicy::new()
    .require("session-auth")
    .require("authorization")
    .rule(MiddlewareOrderingRule::before(
        "session-auth",
        "authorization",
        "identity-before-authorization",
        "authorization requires an authenticated principal",
    ));
```

This keeps policy explicit and reviewable in the consumer instead of hiding it inside the shared library.

## The reviewed legacy profile

`DEFAULT_MIDDLEWARE_ORDER` and `validate_middleware_order(...)` predate the consumer-owned policy API. They remain available for compatibility and as an opt-in reviewed reference profile. They intentionally validate that complete legacy 16-stage profile and therefore should **not** be used when a service wants a different selection or order.

New consumers that need order validation should prefer `MiddlewareOrderPolicy` plus `validate_consumer_middleware_order(...)` and declare only their own invariants.

Likewise, the existing bundled `frameworks::axum::install(...)` + `MiddlewareStack` path remains useful for services that intentionally want that bundled lifecycle. It is not a requirement for consuming the standalone framework primitives.

## Rules for provider/version injection

1. The consumer owns the concrete provider crate and exact version/revision.
2. `ores-middleware` owns only stable provider ports and adapters.
3. Provider-specific types stop at the adapter boundary.
4. Public middleware responses must not expose raw provider diagnostics.
5. Consumers choose middleware selection, ordering, and scope.
6. Validation checks consumer-authored policy; it must not silently add library policy.
7. Legacy bundled stacks and reviewed default order helpers are opt-in compatibility surfaces, not universal architecture.
