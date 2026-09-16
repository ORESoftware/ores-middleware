# ores-middleware

`ores-middleware` is the cross-language request-lifecycle contract for ORESoftware services. It provides one semantic middleware surface with idiomatic adapters for Rust, TypeScript/JavaScript, Go, Gleam, Elixir, and Erlang.

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
| `createMiddleware` | Build the request-lifecycle middleware or stack. |
| `runWithContext` | Execute work with request-scoped context propagation. |
| `currentContext` | Read request context in the active task/process/goroutine chain. |
| `capabilities` | Return the normative capability vocabulary. |

The normative capabilities are request context, crash recovery, request and trace IDs, structured logging, RED metrics, deadlines, payload limits, rate limiting, authentication, sync observation, JSON, header policy, compression, TLS policy, security headers, idempotency, IP policy, ETag/cache control, content negotiation, test-only fault injection and auth bypass, and test schema capture.

## Request-context model

Language-native propagation is primary:

- Rust uses Tokio task-local storage and request extensions.
- TypeScript uses `AsyncLocalStorage`.
- Go uses `context.Context`; goroutines must receive the derived context explicitly.
- Gleam, Elixir, and Erlang use BEAM process-local state and copy context into spawned request tasks.

A request context contains the request ID, trace ID, optional span, tenant, user and locale identifiers, start/deadline timestamps, and bounded OpenTelemetry baggage. Do not place access tokens, passwords, raw personal data, full request bodies, or unrestricted arbitrary headers in context or logs.

A bounded, TTL-limited request-ID registry may be used for diagnostics or controlled cross-boundary lookup. It is not the primary propagation mechanism and must not become an unbounded global map.

## Consumer-owned middleware composition

`ores-middleware` provides middleware primitives, adapters, validation helpers, provider ports, and reviewed ordering guidance. The consuming service owns **which middleware is enabled, its exact order, and its route/server scope**.

`DEFAULT_MIDDLEWARE_ORDER` and `validate_middleware_order(...)` are reviewed guidance/helpers; they are not an instruction that every consumer must install the same chain. A service may intentionally choose a different order or omit middleware that is not applicable. Framework-specific composition should remain explicit at the application boundary so the service can reason about request and response ordering.

A commonly reviewed profile is:

1. panic/recovery boundary;
2. deadline and request/trace context;
3. trusted-proxy / transport-security checks;
4. payload/flood controls;
5. authentication and authorization;
6. principal-aware rate limiting;
7. idempotency;
8. handler execution;
9. response compression/security headers;
10. telemetry finalization.

This profile is guidance, not a universal hard-coded stack. Consumers may use a different composition when semantics require it. Ordering validators should distinguish technical incompatibilities from optional/recommended policy.

Authentication is fail-closed when enabled. `opto-sync` observation may be configured fail-open for non-critical audit delivery, but its failure is always recorded. Test auth bypass and fault injection are configuration errors in production.

## Integration ports

The core packages depend on narrow ports rather than hard-coding provider SDKs:

- **shared-auth / auth providers:** token/JWT verification or a configured HTTP introspection hook. The consuming service pins the concrete auth SDK/version and injects it through the stable ORES provider interface. See [`docs/provider-injection.md`](docs/provider-injection.md).
- **opto-sync:** request-completion observer/outbox hook. Payloads contain correlation and operational metadata, not credentials or unrestricted bodies.
- **ores-otel:** trace propagation and telemetry sink. The W3C `traceparent` and `baggage` propagators are the baseline.
- **rate and idempotency stores:** in-memory implementations support local development and tests; distributed services should inject Redis or another durable/consistent implementation appropriate to the endpoint semantics.

The repository never embeds credentials. Endpoints, trust anchors, encrypted environment paths, and runtime secrets are deployment configuration.

## Framework adapters

### Rust

The Rust package exposes an Axum core plus named installers for MASH, Leptos, and Dioxus full-stack servers. MASH means Maud + Axum + server-rendered HTML/HTMX conventions; the middleware does not couple domain handlers to Maud or HTMX.

```rust
use std::sync::Arc;
use ores_middleware::{default_config, MiddlewareStack};

let mut config = default_config(env!("CARGO_PKG_NAME"));
// Development may explicitly disable HTTPS enforcement. Production should use
// in-process TLS or a trusted-proxy allowlist.
config.settings.tls.require_https = false;
config.settings.tls.mode = "disabled".into();
let stack = Arc::new(MiddlewareStack::new(config)?);
let app = ores_middleware::frameworks::axum::install(app, stack);
```

Concrete auth libraries are injected by the consuming crate rather than linked into `ores-middleware` core. For closure adapters, dual-version migrations, and Shared Auth examples, see [`docs/provider-injection.md`](docs/provider-injection.md).

### TypeScript / JavaScript

The TypeScript package implements a Fetch `Request`/`Response` core, allowing one policy engine to serve Node.js, Deno, Bun, Next.js, Nuxt, Hapi, Hono, Express, and NestJS boundaries.

```ts
import { createMiddleware, defaultConfig } from "@oresoftware/ores-middleware";
import { honoMiddleware } from "@oresoftware/ores-middleware/adapters";

const config = defaultConfig("example-api-server");
config.settings.tls.requireHttps = false;
config.settings.tls.mode = "disabled";
const middleware = createMiddleware(config, { authVerifier, telemetry });
app.use("*", honoMiddleware(middleware));
```

### Go

The Go implementation wraps `net/http`; Gorilla Mux, Gin, Echo, and Fiber are adapted at their server boundary. The request-scoped `context.Context` remains available to handlers and downstream clients.

```go
config := oresmiddleware.DefaultConfig("example-api-server")
config.Settings.TLS.RequireHTTPS = false
config.Settings.TLS.Mode = "disabled"
stack, err := oresmiddleware.New(config, oresmiddleware.Dependencies{})
if err != nil { return err }
http.ListenAndServe(":8080", adapters.Gin(stack, engine))
```

### Gleam, Elixir, and Erlang

Gleam is a first-class implementation compiled to Erlang/OTP; it is not an Elixir wrapper. Elixir exposes Plug/Phoenix boundaries and Erlang exposes a framework-neutral around-handler plus Cowboy middleware. Each runtime uses supervised or monitored request execution for deadline/crash isolation.

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
2. installs the framework adapter at the actual router/server boundary;
3. supplies a service name and explicit TLS/trusted-proxy policy;
4. wires shared-auth, opto-sync, and ores-otel ports as applicable;
5. chooses and documents the middleware selection/order appropriate to that service;
6. adds tests for correlation headers, context propagation, payload limits, auth failure, deadlines, and production safety;
7. documents any temporarily disabled capability and links a tracked follow-up;
8. keeps the PR draft until its own build and tests pass.
