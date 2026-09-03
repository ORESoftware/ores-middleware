# Request-scoped operation boundaries

`ores-middleware` treats context propagation and failure recovery as separate,
composable mechanisms:

1. A **context carrier** makes request, trace, tenant, and authenticated actor
   identifiers available to downstream work.
2. An **operation boundary** catches a recoverable exception, panic, deadline,
   or cancellation at the protocol callback boundary and turns it into a typed,
   sanitized outcome.

Async-local/task-local/process-local storage does **not** catch exceptions by
itself. Likewise, a `try`/`catch` or panic recovery block does not preserve
correlation unless it executes while the correct context is active. Both are
required.

## Ownership

- `ores-middleware` owns HTTP/TCP/WebSocket adapters, request lifecycle,
  deadlines, cancellation, and the decision to return an HTTP problem response,
  close a TCP connection, reject one WebSocket message, or close a socket.
- `ores-otel/ores.otel.log` owns logger/tracer context, redaction, exporters, and
  capture/re-entry primitives. It must not depend on `ores-middleware`.
- TypeSpec and JSON Schema remain independent top-level authorities. Operation
  boundaries implement the existing `request-context`, `trace-context`,
  `panic-recovery`, and `deadline-timeout` capabilities; they do not change the
  wire contract or make one schema source subordinate to the other.

## Required lifecycle

Every protocol adapter follows the same sequence:

1. Validate or create a request ID and trace ID.
2. Build an immutable/defensively copied request context.
3. Enter the native request carrier and the native ores-otel carrier.
4. Enter a tracing span or process metadata scope.
5. Execute exactly one recoverable protocol operation.
6. On failure, classify it while the scope is still active and emit only
   bounded, redacted fields.
7. Translate the typed failure to protocol policy.
8. Restore or clear every ambient scope in `finally`, `defer`, `after`, or RAII
   cleanup paths.

Failure telemetry must never contain request/response bodies, authorization or
cookie headers, tokens, arbitrary exception messages, panic payloads, email
addresses, phone numbers, or provider payloads. The default failure event is
limited to:

- operation name, transport, and scope;
- failure kind and bounded error type;
- request ID and trace ID;
- safe authenticated internal IDs already admitted by the context policy.

Logger/exporter failure is fail-open for the application outcome. The failed
operation is still failed; telemetry cannot replace it with another exception.

## Protocol policy

| Transport | Recommended boundary | Failure translation |
| --- | --- | --- |
| HTTP | One boundary per request | Sanitized `application/problem+json`; do not expose the cause |
| TCP | One boundary per accepted connection and optionally per decoded frame | Close/discard only the affected connection or frame; keep the listener alive |
| WebSocket | Connection boundary plus one boundary per message callback | Reject one message or close that socket according to policy; keep other sockets and the listener alive |

Do not continue a partially mutated transaction after recovering a panic. Roll
back/discard the affected unit of work and let the protocol adapter decide
whether the connection remains trustworthy.

## TypeScript / Node.js

The request carrier uses `AsyncLocalStorage`. The operation helper captures both
middleware and ores-otel frames at callback registration time.

```ts
import {
  bindOperationBoundary,
  type OperationOutcome
} from "@oresoftware/ores-middleware/operation";

const onMessage = bindOperationBoundary(
  {
    transport: "websocket",
    scope: "message",
    name: "chat.message"
  },
  async (message: Uint8Array) => decodeAndHandle(message)
);

socket.on("message", async (message) => {
  const outcome: OperationOutcome<void> = await onMessage(message);
  if (!outcome.ok) {
    socket.send(JSON.stringify({
      type: `urn:ores:middleware:${outcome.failure.code}`
    }));
  }
});
```

`bindOperationBoundary` captures once and re-enters on every callback. A
callback captured outside any request receives an explicit empty child frame;
it cannot inherit whichever unrelated request happens to invoke it later.

Cancellation is cooperative in JavaScript. Pass an `AbortSignal`, make the
operation observe it, and throw/propagate `AbortError` or `TimeoutError`.
JavaScript cannot safely force-stop a promise that ignores cancellation.

## Go

Go keeps business and telemetry context explicit through `context.Context` and
recovers panics at the callback boundary.

```go
outcome := oresmiddleware.RunOperationBoundary(
    connectionContext,
    requestContext,
    oresmiddleware.OperationDescriptor{
        Transport: oresmiddleware.OperationTransportTCP,
        Scope:     oresmiddleware.OperationScopeConnection,
        Name:      "smtp.accept",
    },
    oresmiddleware.OresOperationFailureReporter,
    func(ctx context.Context) (Reply, error) {
        return handleConnection(ctx, conn)
    },
)

if !outcome.OK() {
    _ = conn.Close()
}
```

The operation must honor `ctx.Done()` for timely cancellation. The boundary
checks cancellation/deadline state before and after the operation and classifies
returned `context.Canceled` and `context.DeadlineExceeded` values.

## Rust / Tokio

Rust enters three scopes around the same future:

- `ores-middleware` Tokio task-local request context;
- `ores-otel` task-local log context;
- a `tracing` span containing request and operation fields.

```rust
let outcome = run_operation_boundary(
    request_context,
    OperationDescriptor {
        transport: OperationTransport::WebSocket,
        scope: OperationScope::Message,
        name: "chat.message".into(),
    },
    async move {
        tracing::info!("processing message");
        handle_message(message).await
    },
).await;
```

Because the future is instrumented, ordinary `tracing` events inherit the span.
Timeout and cancellation variants drop the guarded future before reporting the
terminal outcome, so task-local guards are cleaned up and late work cannot keep
running through that future.

`catch_unwind` handles unwind panics only. It cannot catch `panic=abort`, process
termination, out-of-memory aborts, `SIGKILL`, or corrupted-state failures. A
process-wide panic hook is also separate from `catch_unwind`; applications must
install an audited, redacting hook once at startup rather than changing the
process-global hook per request.

## Gleam / Erlang VM

Each operation installs the typed `RequestContext` in the current BEAM process
and maps safe IDs into Erlang Logger process metadata. The FFI boundary catches
`throw`, `error`, and catchable `exit` classes without copying the raw reason or
stack into telemetry, then restores the previous process dictionary and logger
metadata in an `after` clause.

```gleam
operation.run_with_logger(
  context,
  request_logger,
  operation.WebSocket,
  operation.Message,
  "chat.message",
  fn() { handle_message(message, context) },
)
```

BEAM supervision remains the final containment layer. Untrappable kills and VM
termination are not converted into normal request outcomes. For long-lived
connections, capture the explicit context and request logger in the owning
process and pass them to each spawned message process.

## Testing requirements

Every adapter must prove all of the following:

- parallel operations cannot observe another request's ID, user, tenant, trace,
  or baggage;
- a failed WebSocket message is followed by a successful message without
  terminating the listener;
- a TCP connection panic closes/fails only that connection boundary;
- HTTP handler failure becomes a sanitized response;
- timeout and cancellation restore context;
- nested scopes restore the previous scope;
- a callback captured with no context cannot inherit an unrelated caller;
- telemetry reporter failure cannot replace the application outcome;
- public failure values do not reproduce exception messages or payloads;
- operation names and error types are bounded, low-cardinality tokens.
