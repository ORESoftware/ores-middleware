# Request context propagation across ORES runtimes

## Decision

`ores-middleware` owns inbound framework integration and creates the canonical, data-only `RequestContext`. `ores-otel/ores.otel.log` owns logging, tracing, and telemetry primitives and consumes that context. The dependency direction is intentionally one-way:

```text
ores-middleware -> ores-otel/ores.otel.log
```

Do not make `ores-otel` depend on HTTP frameworks or on `ores-middleware`; that would create a dependency cycle and make non-HTTP logging unnecessarily heavy.

The canonical request context contains only allow-listed correlation data:

```text
request_id
trace_id
span_id?
user_id?
tenant_id?
locale?
started_at_unix_ms
deadline_unix_ms?
baggage: authenticated keys beginning with "otel." only
```

`user_id` means the stable authenticated subject identifier. Do not put email, name, authorization headers, cookies, raw tokens, credentials, request bodies, or arbitrary claims in this ambient context.

## Why there is no universal map

The logical API is shared, but storage must match each runtime's concurrency model. A process-wide `Map<request-id, context>` must not be the normal lookup path: it requires lifecycle cleanup, can leak identity data, is vulnerable to request-ID collision, and adds synchronization. Bounded registries may exist only for explicit callback/interop cases where the native context cannot be passed.

| Runtime | Native carrier | Lookup behavior | Required boundary rule |
| --- | --- | --- | --- |
| Node.js, Next.js server, Express, NestJS | `AsyncLocalStorage<RequestContext>` | O(1) field lookup from one frozen object | Call downstream code inside `run`; keep the scope alive through response completion |
| Bun / Deno Node-compatible runtime | Node-compatible ALS | O(1) | Use the server build; verify runtime ALS compatibility |
| Next.js Edge / workerd | runtime ALS only when explicitly supported; otherwise explicit argument | runtime-dependent | Never use the browser single-frame fallback for overlapping requests |
| Browser | explicit context argument | O(1) field access | There is no safe ambient request scope across unrelated async work |
| Go | one aggregate value behind one unexported `context.Context` key | one context-chain lookup, then O(1) struct access | Pass the derived `context.Context` to every goroutine and outbound call |
| Rust / Tokio | Tokio task-local context plus request extensions and ores-otel task context | task-local lookup or extension extraction | Instrument/spawn futures explicitly; never use thread-local request state |
| Gleam / Erlang / Elixir on BEAM | immutable map/record in the current BEAM process plus logger metadata | process-local lookup | Pass context in messages when spawning another process; restore nested scopes |

## TypeScript, Next.js server, Express, and NestJS

The TypeScript store contains one frozen request snapshot rather than a mutable shared `Map`. This still gives O(1) lookup while preventing sibling promises from overwriting request identity.

```ts
import {
  currentLoggedInUserId,
  currentRequestId,
  currentTenantId,
  currentTraceId,
} from "@oresoftware/ores-middleware/context";

const requestId = currentRequestId();
const userId = currentLoggedInUserId();
```

For Fetch-style Next.js server handlers, run the complete handler promise inside the middleware callback. Do not create a second ALS instance in a route.

Express and NestJS require a native lifecycle bridge because `next()` does not return the downstream promise. Install the adapter globally:

```ts
import { expressMiddleware } from "@oresoftware/ores-middleware/adapters";
import { createOresOtelMiddleware } from "@oresoftware/ores-middleware/otel";

const requestMiddleware = createOresOtelMiddleware(config, {
  logger: rootLogger,
  authVerifier,
});

app.use(expressMiddleware(requestMiddleware));
```

The adapter calls `next()` with no argument and holds both ALS frames until Node emits `finish` or `close`. It exposes:

```ts
import {
  nodeRequestContext,
  nodeRequestLogger,
} from "@oresoftware/ores-middleware/adapters";

app.get("/profile", async (req, res) => {
  const context = nodeRequestContext(req); // also req.oresContext when extensible
  const log = nodeRequestLogger(req);       // also req.log / req.oresLog when available

  await log?.info("profile requested").send();
  res.json({ requestId: context?.requestId });
});
```

If an application already owns `req.log`, the adapter does not overwrite it; use `req.oresLog` or `nodeRequestLogger(req)`. Sealed request objects use WeakMap fallbacks.

## Go

Go stores one `RequestContext` struct behind one unexported key. The context chain itself is immutable; the baggage map is defensively copied on insertion and read so it cannot become a cross-goroutine data race.

```go
requestID, ok := oresmiddleware.RequestIDFromContext(r.Context())
userID, authenticated := oresmiddleware.LoggedInUserIDFromContext(r.Context())
traceID, traced := oresmiddleware.TraceIDFromContext(r.Context())
tenantID, tenanted := oresmiddleware.TenantIDFromContext(r.Context())
```

Pass `r.Context()` or a derived child to every goroutine, database call, RPC, queue publisher, and file-level logger. Do not emulate goroutine-local storage and do not look up ordinary request state by request ID in a global registry.

## Rust / Tokio

Tokio tasks may migrate between worker threads, so thread-local state is incorrect. Axum request extensions remain the typed business-logic carrier; Tokio task-local and ores-otel context provide ambient telemetry correlation.

```rust
use ores_middleware::{
    current_logged_in_user_id,
    current_request_id,
    current_tenant_id,
    current_trace_id,
};

let request_id = current_request_id();
let user_id = current_logged_in_user_id();
```

A spawned future must be instrumented or explicitly run with a captured context. A detached background job should usually start a new operation scope while retaining only trace/link metadata needed for correlation.

## Gleam

Use the typed context module for direct or current-process lookup:

```gleam
import ores_middleware/context

let request_id = context.current_request_id()
let user_id = context.current_logged_in_user_id()
```

The result is `Error(Nil)` outside a request scope. A newly spawned BEAM process does not inherit the process dictionary; send the immutable context in the process message and install it only around that process's work.

## Elixir / Plug / Phoenix

```elixir
request_id = OresMiddleware.current_request_id()
user_id = OresMiddleware.current_logged_in_user_id()
tenant_id = OresMiddleware.current_tenant_id()
```

The request context and `Logger.metadata/1` are scoped to the Plug/Phoenix request process. Nested scopes restore the exact previous context and logger metadata in `after` blocks.

## Erlang / Cowboy / OTP

```erlang
RequestId = ores_middleware_context_access:current_request_id(),
UserId = ores_middleware_context_access:current_logged_in_user_id(),
TenantId = ores_middleware_context_access:current_tenant_id().
```

For explicit maps, use `request_id/1`, `user_id/1`, and the related accessors. Child processes must receive the immutable map explicitly.

## Middleware/logger sequence

1. Validate or generate the request ID and parse W3C trace context.
2. Create the data-only context and establish the runtime-native scope.
3. Authenticate; create a new/final request snapshot containing user and tenant IDs.
4. Derive the ores-otel request child after authentication.
5. Invoke downstream work inside both scopes.
6. Inject trace context explicitly into outbound HTTP, RPC, queue, WebSocket, and job envelopes.
7. Restore/discard the scope on success, error, panic/crash, cancellation, timeout, and client disconnect.

## Required conformance tests

Every framework adapter and adopting server must prove:

1. request, trace, user, and tenant IDs are available through both the typed context API and ordinary file/module loggers;
2. many overlapping requests cannot observe each other's context;
3. nested scopes restore the exact parent context;
4. context disappears after success, error, panic/crash, cancellation, timeout, and disconnect;
5. credential-bearing inputs and non-`otel.*` baggage never enter logs;
6. logger/exporter failure does not alter the HTTP response;
7. background tasks either receive an explicit immutable snapshot or intentionally start a new scope.
