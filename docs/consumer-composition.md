# Consumer-owned middleware composition

`ores-middleware` provides middleware implementations, provider ports, framework adapters, validation helpers, and composition primitives. It does **not** own the middleware selection or execution order of a consuming service.

The application or organization consuming the crate owns:

- which middleware exists on a server, router, route, or route group;
- the exact request/response ordering;
- whether a middleware is global, route-specific, or absent;
- concrete provider dependencies and versions;
- which ordering relationships are hard errors, advisories, or intentionally unconstrained.

This is important because two services can use the same ORES primitives with different semantics. An authenticated mutation API may need authentication before a principal-aware rate limiter, while a public endpoint may use only an anonymous flood guard. The library should not manufacture one global answer for both.

## Framework-neutral stage composition

`StagePipeline` is the framework-neutral composition root. Stages execute in exactly the order the consumer adds them; response hooks unwind in reverse order.

`AuthStage` adapts any injected `AuthVerifier` into that pipeline without selecting a position for it:

```rust
use std::sync::Arc;
use ores_middleware::{
    AuthDecision, AuthStage, IntegrationError, RequestMetadata,
    StagePipeline, auth_provider_fn,
};

let sdk = selected_auth_sdk::Client::new(auth_config);
let provider = auth_provider_fn(move |request: RequestMetadata| {
    let sdk = sdk.clone();
    async move {
        let verified = sdk
            .verify(request.headers.get("authorization"))
            .await
            .map_err(|error| IntegrationError {
                code: "auth_rejected",
                message: error.to_string(),
            })?;
        Ok(AuthDecision {
            user_id: Some(verified.user_id),
            tenant_id: verified.tenant_id,
            claims: verified.stable_claims,
        })
    }
});

let pipeline = StagePipeline::new()
    .with_stage(Arc::new(request_id_stage))
    .with_stage(Arc::new(AuthStage::from_provider("company-auth-v2", provider)))
    .with_stage(Arc::new(tenant_rate_limit_stage))
    .with_stage(Arc::new(authorization_stage));
```

`AuthStage` establishes the stable user/tenant request context and copies only `otel.*` claims into request baggage. It does **not** automatically copy arbitrary provider claims into generic attributes. If a later consumer-owned authorization stage needs a reviewed subset, use `with_decision_enricher(...)` to explicitly allow-list those values.

The pipeline itself does not consult `DEFAULT_MIDDLEWARE_ORDER`.

## Standalone Axum authentication primitive

The consuming crate can also place an auth adapter directly at an Axum router boundary:

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

`authenticate` inserts the stable `AuthDecision` into request extensions. Provider-specific error text is not copied into the public `401` response.

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

For a `StagePipeline`, the actual plan is available directly:

```rust
let actual = pipeline.stage_names();
let issues = validate_consumer_middleware_order(&actual, &policy);
```

An ordering rule is conditional by default: if one of its two stages is absent, the rule does not make that stage mandatory. Use `.require(...)` for presence or `.require_both()` on a particular relationship when absence itself is a violation.

Duplicate middleware is also allowed by default. A consumer opts into uniqueness only for stage names where duplication is semantically invalid. If duplicates are allowed, a `before -> after` rule requires the last `before` occurrence to precede the first `after` occurrence, so an interleaved duplicate cannot accidentally satisfy the policy.

## Serializable composition plans and `.ores-mw.toml`

`MiddlewareCompositionPlan` is a serializable declaration containing the exact consumer-selected `stages` sequence plus its `policy`. It is intentionally independent of the cross-language root `MiddlewareConfig` for now: a project/CLI can embed it in `.ores-mw.toml` without silently changing every language's root config contract before TypeSpec, JSON Schema, and all runtime adapters are updated together.

A TOML projection can look like this:

```toml
[composition]
stages = [
  "request-id",
  "company-auth-v2",
  "tenant-rate-limit",
  "authorization",
  "handler",
  "telemetry",
]

[composition.policy]
required = ["company-auth-v2", "authorization"]
forbidden = ["test-auth-bypass"]
unique = ["tenant-rate-limit", "authorization"]
first = "request-id"

[[composition.policy.rules]]
before = "company-auth-v2"
after = "tenant-rate-limit"
code = "auth-before-tenant-limit"
message = "this service derives its limiter principal from authenticated identity"
severity = "error"
requireBoth = true
```

The crate exposes two admission layers:

```rust
use ores_middleware::{
    MiddlewareCompositionPlan,
    validate_declared_middleware_plan,
    validate_runtime_middleware_plan,
};

let declared: MiddlewareCompositionPlan = load_from_ores_mw_toml()?;

// Config/CI validation before the server is built.
let declaration_issues = validate_declared_middleware_plan(&declared);

// Startup/test validation proves live registration did not drift from config.
let runtime_issues = validate_runtime_middleware_plan(&pipeline.stage_names(), &declared);
```

`validate_runtime_middleware_plan(...)` compares the runtime stage sequence to the declared sequence exactly and then applies only the consumer-authored policy. It never consults the legacy default order.

This two-step design keeps provider/stage construction in application code—where concrete Rust types and pinned SDK versions belong—while allowing config/CLI tooling to review the intended sequence independently.

## Route-specific policies

A large service does not need one policy for every route. Keep policies/plans at the same ownership boundary as the composition they validate:

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

A consumer may store multiple named composition plans in its own `.ores-mw.toml` projection—for example `public`, `mutation`, and `admin`—and validate each corresponding runtime router independently. The shared crate does not impose the naming or routing topology.

## The reviewed legacy profile

`DEFAULT_MIDDLEWARE_ORDER` and `validate_middleware_order(...)` predate the consumer-owned policy API. They remain available for compatibility and as an opt-in reviewed reference profile. They intentionally validate that complete legacy 16-stage profile and therefore should **not** be used when a service wants a different selection or order.

New consumers that need order validation should prefer `MiddlewareOrderPolicy` / `MiddlewareCompositionPlan` and declare only their own invariants.

Likewise, the existing bundled `frameworks::axum::install(...)` + `MiddlewareStack` path remains useful for services that intentionally want that bundled lifecycle. It is not a requirement for consuming the standalone framework primitives or `StagePipeline`.

## Rules for provider/version injection

1. The consumer owns the concrete provider crate and exact version/revision.
2. `ores-middleware` owns only stable provider ports and adapters.
3. Provider-specific types stop at the adapter boundary.
4. Public middleware responses must not expose raw provider diagnostics.
5. Consumers choose middleware selection, ordering, and scope.
6. Validation checks consumer-authored policy; it must not silently add library policy.
7. Runtime admission can prove live stage registration matches the consumer's serialized plan exactly.
8. Legacy bundled stacks and reviewed default order helpers are opt-in compatibility surfaces, not universal architecture.
