# Response trace-context policy

`traceparent` identifies a concrete trace and span. A middleware layer must not
invent an all-zero span ID, and it must not relabel the inbound parent span as
the server span.

## Ownership

- Inbound `traceparent` is parsed only to continue or replace the trace ID.
- A real runtime tracer owns server-span creation, sampling flags, status,
  exception recording, and span completion.
- The portable middleware always emits its configured request-ID response
  header.
- The portable middleware does not synthesize a response `traceparent`.
- A downstream tracer may set a response `traceparent`; middleware preserves it
  only when version, trace ID, span ID, and flags are structurally valid and both
  IDs are non-zero.
- Runtimes whose response API is constructed independently of the handler
  (currently the Rust pipeline metadata API and Cowboy pre-handler adapter) omit
  `traceparent` until their tracer integration can supply the active server
  span.

## Validation

For W3C version `00`, this repository accepts exactly four lowercase-normalized
fields:

```text
00-<32 hex non-zero trace id>-<16 hex non-zero span id>-<2 hex flags>
```

Malformed values and all-zero identifiers fail closed and are removed rather
than propagated.

## Runtime behavior

TypeScript/Fetch, Go, Gleam, Elixir, and Erlang preserve a valid response header
created by the downstream tracer and remove an invalid one. Express and NestJS
set only the request ID before dispatch; they never echo the inbound parent
span. Rust and Cowboy omit response trace context until a tracer-owned server
span is available.

The static Rust audit in `scripts/check_traceparent_policy.rs` runs in CI and
prevents reintroducing the known all-zero response-span pattern.
