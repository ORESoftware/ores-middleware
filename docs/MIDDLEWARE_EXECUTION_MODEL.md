# Middleware execution model

This document defines the semantic middleware algebra that `ores-middleware` exposes to consumers. Framework-specific callback/continuation APIs are adapters around this model; they are not the cross-language contract.

**Composition ownership is deliberately outside the library.** A consuming service chooses which stages exist, their exact request order, their response-unwind behavior, and their route/server scope. `ores-middleware` provides primitives, provider ports, adapters, validation helpers, and optional reviewed reference profiles; it does not silently install one universal chain.

See [`consumer-composition.md`](consumer-composition.md) for the concrete Rust `StagePipeline`, injectable `AuthStage`, standalone Axum auth layer, and consumer-authored `MiddlewareOrderPolicy` APIs.

## Goals

The core model must support:

- consumer-defined middleware stages built against abstract ORES interfaces;
- fluent/chained composition without coupling domain code to Axum, Express, Gin, Plug, Cowboy, or another framework;
- consumer-owned selection and ordering, including custom stages unknown to this repository;
- request short-circuiting by returning a response or structured rejection;
- response-phase processing without requiring every language to express middleware as nested callbacks;
- typed request context and payload access;
- deterministic execution for the order the consumer actually declared;
- explicit dependency/failure policy without importing hidden library ordering rules;
- equivalent semantics across Rust, TypeScript, Go, Gleam, Elixir, and Erlang.

## Request-scoped state, not a singleton

`ores-interfaces` should own the semantic request envelope. Each runtime projects that contract into an idiomatic per-request value.

The envelope should contain stable typed fields such as:

- canonical lowercase headers;
- query/path parameters;
- request ID, trace ID, span ID, deadline, locale;
- authenticated subject, tenant and authorization metadata;
- representation tag and decoded payload;
- bounded raw body when explicitly retained;
- cache, rate-limit, idempotency and circuit-breaker decisions;
- a bounded extension/attribute area for middleware-specific metadata.

The request envelope is **not** a process-global singleton and is never a shared mutable map across concurrent requests. Runtime-local propagation such as Tokio task-local state, `AsyncLocalStorage`, Go `context.Context`, or BEAM process-local state may expose the current request ergonomically, but the underlying state remains request-scoped.

Prefer fixed typed fields for shared ORES semantics. Use an extension bag only for data that cannot reasonably be standardized. In Rust, a native typemap/extension mechanism may hold type-safe local values while the portable ORES envelope remains serializable/data-oriented.

## Return-oriented stage algebra

The portable semantic core models a request stage as a transformation that returns a decision rather than invoking `next` directly.

Conceptually:

```text
before(request_context, request_envelope) -> StageDecision

after(request_context, request_envelope, response_envelope) -> ResponseDecision
```

`StageDecision` has three normative outcomes:

```text
Continue(updated_request_context, updated_request_envelope)
Respond(response_envelope)
Reject(problem_response)
```

`ResponseDecision` has two normative outcomes:

```text
ContinueResponse(updated_response_envelope)
ReplaceResponse(response_envelope)
```

A middleware stage may implement only `before`, only `after`, or both. A pipeline runner executes `before` stages in the **consumer-declared** order and `after` stages in reverse order for stages that were entered successfully. This provides conventional "around middleware" behavior without making nested continuations the contract.

The application handler is the terminal request function:

```text
handle(request_context, request_envelope) -> response_envelope | problem_response
```

This design makes middleware independently testable: each stage is a function from typed input to typed output/decision.

## Language projections

The semantic contract is shared; syntax remains idiomatic.

### Rust

Prefer the return-oriented async `StagePipeline` when framework-neutral composition is useful. The application selects stage objects and insertion order:

```text
StagePipeline::new()
  .with_stage(request_id)
  .with_stage(AuthStage::from_provider("company-auth-v2", auth_provider))
  .with_stage(tenant_rate_limit)
  .with_stage(authorization)
```

The auth provider is injected by the consuming crate, which therefore owns the concrete SDK and exact dependency version/revision. `ores-middleware` owns only the stable `AuthVerifier` boundary and adapters.

A framework adapter can translate Axum/Tower request/response types to ORES envelopes, run a pipeline, then translate the final response back. Consumers may also compose standalone framework primitives directly, such as `frameworks::axum_composable::authenticate`. Avoid requiring consumer middleware to implement Tower `Service` unless it specifically wants a Tower-native adapter.

For object-safe dynamic stage registries, use boxed futures or another object-safe async abstraction; concrete/static pipelines may use generic async functions for zero-cost composition.

### TypeScript / JavaScript

Expose async functions/classes returning the same decision union. Express/Hono/Nest/Next adapters may translate `next()` middleware into or out of the ORES stage form, but shared middleware should prefer returning a decision/response. Registration order belongs to the consuming application or its explicit middleware config.

### Go

Keep `net/http` compatibility at the adapter boundary. Native Go consumers may still use conventional:

```text
func(next http.Handler) http.Handler
```

when required by framework composition, but the reusable ORES semantic layer should support explicit stage methods returning a decision. The adapter owns calling the next handler; the application owns how handlers/middleware are chained. This keeps the cross-language contract deterministic while remaining idiomatic for `net/http`, Gin, Echo, Gorilla and Fiber.

Go request context must flow through `context.Context`; no hidden goroutine-local singleton is allowed.

### Gleam / Elixir / Erlang

Use data-returning stage functions and supervised process-local request context. Plug/Cowboy/Ranch callback contracts remain adapters around the portable decision model. The consumer still owns which stages are registered and in what order.

## Reference lifecycle phases, not a mandatory chain

The following is a **reviewed reference profile** showing commonly useful semantic phases. It is not a universal order imposed by `ores-middleware`, and a consumer is not required to install every phase:

1. panic/crash boundary and cancellation/deadline budget;
2. request/trace correlation establishment and W3C context extraction;
3. trusted-peer/TLS policy verification;
4. decrypt explicitly encrypted application payloads;
5. decompress encoded request bodies with compressed and expanded size limits;
6. content-type dispatch and JSON/XML/MessagePack/Protobuf decode;
7. contract/schema validation;
8. CORS preflight/origin policy where applicable;
9. CSRF validation for cookie/session-authenticated unsafe methods;
10. anonymous flood admission;
11. authentication using the consumer-injected provider;
12. principal/tenant/route rate limiting;
13. authorization;
14. dependency circuit-breaker admission and downstream timeout budgets;
15. cache lookup/conditional request;
16. idempotency admission;
17. application handler;
18. catch-all 4xx normalization;
19. response validation where configured;
20. cache write / ETag finalization;
21. response compression;
22. application/message encryption where explicitly configured;
23. security/correlation headers;
24. metrics, tracing, audit observation and cleanup.

A consumer may split these phases, combine them, omit them, add custom stages, or choose another order. What matters is that the consumer explicitly declares the invariants its own composition requires. For example, an authorization stage that consumes authenticated identity should declare/authenticate-before-authorize; a public route with no identity requirement need not install auth at all.

Use `MiddlewareOrderPolicy` plus `validate_consumer_middleware_order(...)` for such consumer-authored invariants. The empty policy imposes no selection/order/uniqueness constraints. `DEFAULT_MIDDLEWARE_ORDER` and `validate_middleware_order(...)` are retained only as an opt-in legacy/reference profile.

Some transform/security relationships are semantic facts rather than a global stack prescription. For payload transforms, outbound `serialize -> compress -> encrypt` implies inbound `decrypt -> decompress -> deserialize` when those transforms are actually enabled. TLS termination may occur in-process or at an external trusted proxy; middleware verifies the configured effective transport policy and must not pretend to terminate TLS when deployment does it elsewhere.

## Circuit breakers, timeouts and bulkheads

A downstream integration port should carry an explicit resilience policy. At minimum:

- connect timeout;
- overall request/deadline budget;
- maximum in-flight requests or concurrency semaphore where appropriate;
- circuit state (`closed`, `open`, `half_open`);
- failure threshold/window;
- half-open probe limit;
- open interval/backoff;
- retry policy only for operations proven safe/idempotent;
- fallback/failure mode.

Circuit breaking is per dependency/operation class, not one global switch. A Redis cache failure may degrade to a miss when policy allows; an auth outage for a protected route must fail closed. A rate-limit authorization boundary must not silently become fail-open because the coordinator is unavailable.

Circuit state and retry counters must not be stored in the portable per-request envelope except as the bounded decision/result for that request. Shared breaker state belongs to the injected integration implementation.

## CORS

CORS is centralized policy when a consumer enables that stage, not ad hoc route headers. Configuration should support:

- exact/suffix origin allowlists and explicit development-only wildcard policy;
- allowed methods and headers;
- exposed response headers;
- credential mode;
- preflight max age;
- `Vary: Origin` behavior;
- rejection of invalid/null origins when not explicitly permitted.

A consumer may choose to answer preflight before expensive auth/business processing while still retaining correlation/telemetry and abuse controls. That relationship should be explicit in the service policy rather than assumed globally.

## CSRF

CSRF protection applies primarily to ambient-credential requests such as cookie/session-authenticated unsafe methods. Bearer-token-only APIs may use an explicit exemption policy when they do not rely on browser ambient credentials.

Support pluggable strategies such as:

- synchronizer token;
- double-submit cookie;
- strict Origin/Referer checks as an additional signal;
- framework-native token validators behind an ORES port.

For a state-changing route that enables CSRF and idempotency storage, the consumer should explicitly require CSRF validation before successful mutation/idempotency commitment.

## Contract and payload validation

Parsing and validation are separate stages when both are enabled.

1. Representation decoding answers: "can these bounded bytes be decoded as the selected media type?"
2. Contract validation answers: "does the decoded value satisfy the route's declared contract?"

Route metadata should select a request contract from TypeSpec/JSON Schema/OpenAPI-generated evidence or an equivalent compiled validator. `typespec-json-schema-validator` remains the parity/evidence gate between independently authored TypeSpec and JSON Schema authorities; middleware consumes a selected runtime validator rather than regenerating one authority from the other.

Validation failures should normally produce structured `422` responses; syntax/encoding failures remain `400`/`415` as appropriate.

Protobuf validation requires the route/message descriptor before decode. XML parsing must disable DTD/external entity expansion. MessagePack decoders must enforce configured depth/container/byte limits where the implementation exposes them.

## Metrics and tracing

Every enabled stage should emit bounded, low-cardinality telemetry through the `ores-otel` port where the service policy requires it. Useful dimensions include stable operation/service identifiers and outcome classes, not raw user IDs, paths with arbitrary IDs, tokens, or unbounded header values.

Standard measurements may include:

- request count by operation, method/status class and outcome;
- request duration;
- compressed and expanded request bytes;
- response bytes;
- auth/rate-limit/cache/idempotency decisions;
- parser/validator rejection counts;
- circuit-breaker state transitions and rejected calls;
- downstream dependency duration/failure class;
- CORS/CSRF rejection counts;
- active/in-flight requests where meaningful.

W3C `traceparent`/`baggage` are the baseline propagation format. The canonical authored header namespace remains lowercase, including `x-ores-*`; inbound HTTP header matching is case-insensitive.

## Configuration

Middleware-owned configuration belongs in `.ores-mw.toml`; package-specific policies remain with their owning repositories/configs such as `.ores-rl.toml`, `.ores-lru.toml`, `.auth-shared.toml`, and `.ores-otel.toml`.

First-class `.ores-mw.toml` composition sections should be **consumer-authored**, scoped to a target/router/route-group where necessary, and capable of declaring:

- stage selection and exact order;
- stage-specific enablement/config references;
- consumer-authored required/forbidden/unique/first constraints;
- consumer-authored `before`/`after` relationships and their severity;
- CORS and CSRF policy references;
- payload codec/validation limits;
- downstream timeout/circuit-breaker policy references;
- response validation policy;
- redirect/security-header policy.

The runtime must not populate omitted composition fields from `DEFAULT_MIDDLEWARE_ORDER` unless the consumer explicitly selects the legacy/reference profile. Config validation should validate what the consumer authored, not turn a recommendation into a hidden mandatory chain.

Secret values never belong in these files. Only secret references/provider keys may be declared.

## Consumer extension contract

`ores-middleware` exposes abstract ports/stage interfaces that consumers can implement without forking the library. A consumer extension should declare what downstream tooling actually needs, for example:

- stable stage name;
- whether it participates in request, response, or both phases;
- whether it may short-circuit;
- which typed envelope fields it reads/writes;
- failure policy;
- telemetry name/cardinality contract;
- configuration keys it owns.

Relative ordering is **optional and consumer-owned**. A service can add `before`/`after` rules only where its own semantics require them. Custom stage names do not need to be added to a closed ORES enum before they can participate in a consumer policy.

Ordering cycles are configuration errors only when the consumer's declared constraints create a cycle. Duplicate stages are allowed unless the consumer marks a stage unique. Conflicting writes to exclusive typed fields should still fail validation when they are statically knowable.

## Required conformance tests

Cross-language parity should include fixtures proving:

- `Continue`, `Respond`, and `Reject` have equivalent behavior;
- request stages execute in the exact order declared by the consumer;
- response finalizers execute in reverse entered order;
- short-circuited requests do not run later request stages or the handler;
- finalizers for already-entered stages still run when a later stage short-circuits, when policy requires cleanup;
- arbitrary custom stage names can participate in consumer-authored order validation;
- an empty order policy does not impose a hidden default sequence;
- request context never leaks across concurrent requests;
- dependency timeouts respect the parent request deadline;
- enabled schema validation occurs after decode when the consumer declares that dependency;
- metrics/traces are emitted for configured success, rejection, timeout and short-circuit paths;
- all authored/generated ORES header names are lowercase while inbound matching remains case-insensitive.

The portable contract should be represented in TypeSpec and JSON Schema as independent peers where data-model/schema expression is appropriate. Behavioral invariants for consumer-declared stage ordering, short-circuiting, deadline monotonicity and breaker state transitions are candidates for the existing function-body/formal-method track (including Dafny or equivalent proofs/model checks) rather than being encoded as comments only.
