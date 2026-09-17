# ores-middleware

`ores-middleware` is the cross-language request-lifecycle contract and composition toolkit for ORESoftware services. It provides shared middleware primitives with idiomatic adapters for Rust, TypeScript/JavaScript, Go, Gleam, Elixir, and Erlang while leaving middleware selection and ordering to each consuming service.

This repository is governed by [`ORESoftware/my-ai/AGENTS.md`](https://github.com/ORESoftware/my-ai/blob/main/AGENTS.md). TypeSpec and JSON Schema/OpenAPI are independent, peer contract authorities. Neither authority is generated from the other and treated as canonical. Generated artifacts are compared evidence; discrepancies fail closed and require human evaluation.

## Repository layout

```text
contracts/
  typespec/                 # top-level human-authored TypeSpec authority
  json-schema/              # top-level human-authored JSON Schema authority
  fixtures/                 # canonical positive/negative contract cases
scripts/                    # authority, descriptor, and source-layout gates
src/
  rust/                     # Tokio + Axum/MASH/Leptos/Dioxus
  ts/                       # Fetch core + Node/Deno/Bun/framework adapters
  golang/                   # context.Context + net/http/Gorilla/Gin/Echo/Fiber
  gleam/                    # Gleam implementation on Erlang/OTP
  elixir/                   # Plug/Phoenix/Bandit/Cowboy implementation
  erlang/                   # OTP core + Cowboy/Ranch/Elli boundaries
target/
  rust/
  ts/
  golang/
  gleam/
  elixir/
  erlang/
  contracts/
  descriptors/
```

`target/` is disposable. CI builds it from immutable commits; generated files are not contract authorities.

## Standard SDK surface

Every language exports the same seven semantic operations, using idiomatic symbol names recorded in its adapter descriptor:

| Semantic operation | Purpose |
| --- | --- |
| `descriptor` | Describe language, runtime, adapters, capabilities, and exported symbols. |
| `defaultConfig` | Construct secure baseline configuration for a named service. |
| `validateConfig` | Enforce contract invariants and production safety gates. |
| `createMiddleware` | Build request-lifecycle middleware/composition primitives. |
| `runWithContext` | Execute work with request-scoped context propagation. |
| `currentContext` | Read request context in the active task/process/goroutine chain. |
| `capabilities` | Return the normative capability vocabulary. |

The normative capabilities are request context, crash recovery, request and trace IDs, structured logging, RED metrics, deadlines, payload limits, rate limiting, authentication, sync observation, JSON, header policy, compression, TLS policy, security headers, idempotency, IP policy, ETag/cache control, content negotiation, test-only fault injection and auth bypass, and test schema capture.

A runtime can expose the same capability vocabulary without forcing every consumer to enable every capability or to place it at one global position.

## Request-context model

Language-native propagation is primary:

- Rust uses Tokio task-local storage and request extensions.
- TypeScript uses `AsyncLocalStorage`.
- Go uses `context.Context`; goroutines must receive the derived context explicitly.
- Gleam, Elixir, and Erlang use BEAM process-local state and copy context into spawned request tasks.

A request context contains the request ID, trace ID, optional span, tenant, user and locale identifiers, start/deadline timestamps, and bounded OpenTelemetry baggage. Do not place access tokens, passwords, raw personal data, full request bodies, or unrestricted arbitrary headers in context or logs.

A bounded, TTL-limited request-ID registry may be used for diagnostics or controlled cross-boundary lookup. It is not the primary propagation mechanism and must not become an unbounded global map.

## Consumer-owned middleware composition

`ores-middleware` provides middleware implementations, adapters, validation helpers, provider ports, and composition primitives. The consuming service owns **which middleware is enabled, its exact order, its route/server scope, and its concrete provider versions**.

For new Rust consumers, the primary consumer-owned surfaces are:

- `StagePipeline` — executes request stages in the exact insertion order selected by the consumer and unwinds response hooks in reverse entered order;
- `AuthStage` — framework-neutral authentication backed by an injected `AuthVerifier`;
- `frameworks::axum_composable::{AuthLayerState, authenticate}` — a standalone Axum auth primitive that can be placed at any router/route boundary;
- `MiddlewareOrderPolicy` + `validate_consumer_middleware_order(...)` — validation of **consumer-authored** required/forbidden/unique/first/before/after rules using arbitrary stage names.

An empty `MiddlewareOrderPolicy` imposes no stage selection, order, uniqueness, or first-stage requirement. This is intentional: a shared library cannot know the correct chain for every service.

`DEFAULT_MIDDLEWARE_ORDER` and `validate_middleware_order(...)` remain only as an opt-in legacy/reference profile for consumers that deliberately choose that reviewed 16-stage sequence. They are not the default architecture for new consumers.

A commonly reviewed profile may include recovery/deadline, correlation, trusted transport, payload controls, auth, rate limiting, authorization, idempotency, handler execution, response transforms, security headers, and telemetry. The service may split, omit, add, or reorder those stages. Technical dependencies should be written as explicit consumer rules—for example, a principal-aware limiter can require `auth -> rate-limit` when its key depends on authenticated identity.

See [`docs/consumer-composition.md`](docs/consumer-composition.md), [`docs/MIDDLEWARE_EXECUTION_MODEL.md`](docs/MIDDLEWARE_EXECUTION_MODEL.md), and [`docs/COMPLETE_MIDDLEWARE_STACK.md`](docs/COMPLETE_MIDDLEWARE_STACK.md).

Authentication is fail-closed when enabled on a protected route. `opto-sync` observation may be configured fail-open for non-critical audit delivery, but its failure is always recorded. Test auth bypass and fault injection are configuration errors in production.

## Integration ports

The core packages depend on narrow ports rather than hard-coding provider SDKs:

- **auth providers / shared-auth:** the consuming service pins the concrete auth SDK/version/revision and injects it through the stable `AuthVerifier`/`AuthDecision` boundary. `auth_provider_fn(...)` supports closure adapters; `shared_auth_provider_fn(...)` supports the stricter Shared Auth provider boundary. See [`docs/provider-injection.md`](docs/provider-injection.md).
- **opto-sync:** request-completion observer/outbox hook. Payloads contain correlation and operational metadata, not credentials or unrestricted bodies.
- **ores-otel:** trace propagation and telemetry sink. W3C `traceparent` and `baggage` are the baseline.
- **rate and idempotency stores:** in-memory implementations support local development and tests; distributed services should inject Redis or another durable/consistent implementation appropriate to the endpoint semantics.

The repository never embeds credentials. Endpoints, trust anchors, encrypted environment paths, and runtime secrets are deployment configuration.

## Rust examples

### Framework-neutral composition

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
            claims: Default::default(),
        })
    }
});

let pipeline = StagePipeline::new()
    .with_stage(Arc::new(request_id_stage))
    .with_stage(Arc::new(AuthStage::from_provider("company-auth-v2", provider)))
    .with_stage(Arc::new(tenant_rate_limit_stage));
```

The concrete `selected_auth_sdk` dependency belongs to the consuming crate and can be pinned to an exact release or Git revision. Provider-specific types stop at the adapter boundary.

### Consumer-owned order validation

```rust
use ores_middleware::{
    MiddlewareOrderPolicy, MiddlewareOrderingRule,
    validate_consumer_middleware_order,
};

let policy = MiddlewareOrderPolicy::new()
    .require("company-auth-v2")
    .rule(MiddlewareOrderingRule::before(
        "company-auth-v2",
        "tenant-rate-limit",
        "auth-before-tenant-limit",
        "this service derives its limiter key from authenticated identity",
    ));

let issues = validate_consumer_middleware_order(&pipeline.stage_names(), &policy);
assert!(issues.is_empty());
```

### Standalone Axum primitive

```rust
use axum::{Router, middleware, routing::get};
use ores_middleware::frameworks::axum_composable::{AuthLayerState, authenticate};

let protected = Router::new()
    .route("/account", get(account))
    .layer(middleware::from_fn_with_state(
        AuthLayerState::from_provider(provider),
        authenticate,
    ));
```

The existing bundled `MiddlewareStack` + `frameworks::axum::install(...)` surface remains available for compatibility and for services that deliberately want that bundled lifecycle. It is not required for consumer-owned composition.

## Framework adapters

### TypeScript / JavaScript

The TypeScript package implements a Fetch `Request`/`Response` core, allowing shared middleware primitives to serve Node.js, Deno, Bun, Next.js, Nuxt, Hapi, Hono, Express, and NestJS boundaries. Consumer registration order remains explicit at the application/config boundary.

### Go

The Go implementation wraps `net/http`; Gorilla Mux, Gin, Echo, and Fiber are adapted at their server boundary. The request-scoped `context.Context` remains available to handlers and downstream clients. The application owns which wrappers are installed and their nesting order.

### Gleam, Elixir, and Erlang

Gleam is a first-class implementation compiled to Erlang/OTP; it is not an Elixir wrapper. Elixir exposes Plug/Phoenix boundaries and Erlang exposes a framework-neutral around-handler plus Cowboy middleware. Each runtime uses supervised or monitored request execution for deadline/crash isolation while preserving consumer-selected composition.

## TLS termination

TLS can terminate in-process or at an explicitly trusted edge proxy. In trusted-proxy mode:

- configure exact proxy CIDRs;
- reject forwarded headers received from untrusted peers;
- use the socket peer address when trust cannot be established;
- never accept arbitrary `X-Forwarded-Proto`, `X-Forwarded-For`, or equivalent headers from the public internet.

Production defaults should require HTTPS. Local test configurations may explicitly disable that requirement; silent environment-based weakening is forbidden.

## Test and release gates

Run all gates with:

```bash
just verify
```

The gate compiles TypeSpec, compares the TypeSpec and JSON Schema vocabularies, validates canonical fixtures, compiles/tests every language package, prints a runtime descriptor from every implementation, validates each descriptor against JSON Schema, and compares the all-language operation/capability surfaces.

A release must stop when:

- the two contract authorities disagree;
- a language descriptor omits or renames a semantic operation without an explicit mapping;
- a runtime lacks a required capability;
- production accepts test bypass/fault injection;
- generated SQL/types/interfaces derived independently from TypeSpec and JSON Schema disagree;
- compile-time or runtime conformance fails.

## Server adoption checklist

A downstream server PR is complete only when it:

1. pins `ores-middleware` to a reviewed release or immutable commit;
2. installs the needed framework-neutral stages and/or framework adapters at the actual router/server boundary;
3. supplies a service name and explicit TLS/trusted-proxy policy where applicable;
4. wires auth, opto-sync, ores-otel, rate-limit, cache, and other ports as applicable;
5. owns and documents the concrete provider versions/revisions it injects;
6. chooses and documents the middleware selection/order appropriate to that service;
7. declares consumer-owned order invariants where static validation is useful;
8. adds tests for the selected middleware's correlation, context, payload, auth, deadline, ordering, and production-safety behavior;
9. documents any temporarily disabled capability and links a tracked follow-up;
10. keeps the PR draft until its own build and tests pass.
