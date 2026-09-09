# Rust operation admission and transport audit

Tracking: [DEN-3176 / PR #68](https://github.com/ORESoftware/ores-middleware/pull/68).
This is a scoped boundary hardening change, not certification of every server,
protocol, runtime, framework, or JSON Schema keyword.

## Admission contract

Use `run_operation_boundary_with_timeout_and_cancellation` when both shutdown
and a request/message budget apply. The existing timeout-only and cancellation-only
APIs share the same implementation. Poll order is cancellation, monotonic deadline,
then the guarded handler. Already-ready cancellation, a zero budget, and a budget
that cannot be represented by the monotonic clock reject before handler polling.
Cancellation wins a simultaneous readiness tie. An ordinary result or contained
unwind panic retains the existing typed outcome and public error code.

All losing futures are owned inside a lexical scope that ends before terminal
failure reporting. Request and ores-otel context must be restored before callers
observe the outcome; nested calls restore their outer context rather than erase
it. The function-body evidence now requires the ordered control branches instead
of the previous `tokio::time::timeout` spelling. Negative source mutations exercise
that binding. Source-fragment witnesses are drift detectors, not AST proofs or
substitutes for actual Rust execution.

The budget starts on first poll. Cancellation is cooperative: a handler that
never yields cannot be preempted; effects completed before cancellation cannot be
rolled back. Keep side effects inside the future rather than its constructor.
Dropping a spawned task's JoinHandle does not abort that task. Callers own task
supervision, transactional cancellation semantics, authorization, retries and
backpressure. `panic=abort`, destructor panics, and process failures are outside
this unwind boundary's guarantee.

## Actual test surfaces

`operation_admission.rs` tests pre-cancelled and zero-budget no-dispatch behavior,
unpolled-future destruction, successful pending-cancellation work, nested logger
restoration and concurrent request isolation. HTTP/TCP/WebSocket enum values here
are descriptor coverage, not three wire protocols.

`operation_controlled.rs` tests three-way readiness priority, budget overflow,
virtual-time expiration, cancellation after handler admission, destruction of
unused cancellation resources, returned errors and contained panics.

`tcp_lifecycle.rs` uses actual loopback sockets: 24 concurrent connections, two
messages per connection, nested connection/message scopes, cancelled owned sockets,
partial-frame deadlines, truncated input, invalid length prefixes, and continued
operation after a fully read message handler fails or panics. Its four-byte length
prefix and 64-byte maximum are deliberately test-only framing, not a production
codec. Abandoning a partially consumed frame closes the owned connection; it does
not pretend that a cancelled `read_exact` can safely resume a new frame.

The dedicated workflow checks out the exact PR head, runs both `--all-features`
and `--no-default-features` under Cargo `--locked`, verifies no tracked source
mutation, and retains source identity and execution logs. A source snapshot alone
is not a successful test receipt. Failures never produce `result.txt=passed`.
The repaired Cargo lockfile comes from Cargo metadata at run `34300014073` and
adds only the already-pinned `bytes` edge to Tokio; no package version changes.

Reproduction: unchanged production code at regression-only commit
`ac80dc3e775a213d569fdb8fdca0a937f0117f0c` compiled, but five of seven new admission
tests failed in run `34299184138`. The production fix at
`b8212795944a5098ba064d314f4f421aed301f40` passed all seven under both feature modes
in run `34299544588`. That earlier run does not certify later TCP tests: use the
final candidate's retained logs for their execution result.

## Contract and runtime coverage boundaries

The merged [TJSV native gate (#67)](https://github.com/ORESoftware/ores-middleware/pull/67)
uses `ORESoftware/typespec-json-schema-validator` pinned to
`4473504c4c9d2831d825919f70c03994d8ce01d2`. TypeSpec and authored JSON Schema remain
independent peers. IdempotencyRecord admission binds a verified current-input
Contract IR to actual Node, Rust, Go, Gleam, Elixir and Erlang executions in both
authority lanes. Its 12 native cells do not certify every middleware contract.
Filesystem evidence hardening is separately tracked in
[PR #72](https://github.com/ORESoftware/ores-middleware/pull/72).

| Surface | Evidence and remaining acceptance work |
| --- | --- |
| Rust HTTP and generic futures | Existing Rust/Axum and otel tests plus this admission suite. No claim for every non-Tokio executor, every HTTP server framework, streaming response, or upgrade lifecycle. |
| Rust TCP | Actual bounded loopback tests in this PR. Production framing, TLS, per-message authenticated authorization, flow control and protocol-specific cancellation still belong to adapters/applications. |
| Rust WebSocket | Generic operation outcome/descriptor coverage only in this slice. Real upgrade, close handshake, fragmentation, ping/pong, malformed-frame and disconnect/backpressure tests remain required. |
| Go and Gleam | Existing native contract lanes remain required; descriptor/code generation alone is not server integration evidence. Replay codec parity, cancellation and concrete server-adapter acceptance need their own tests. |
| Node / Express / Nest / Next / Hono / Hapi | [PR #65](https://github.com/ORESoftware/ores-middleware/pull/65) contains separate Hapi/Fetch/body-bound/replay fixes and actual Hapi tests. Treat individual framework installers, lifecycle hooks and streaming/upgrade behavior independently. |
| Bun and Deno | PR #65 records real native loopback executions for specific tested versions. Do not infer all Node package, Next Edge, Proxy or Nest Fastify compatibility from Fetch support. |

Broader five-surface TJSV parity remains tracked in
[PR #66](https://github.com/ORESoftware/ores-middleware/pull/66). Do not waive
structural disagreements because a finite corpus agrees. Conversely, do not
classify every representation mismatch as an exploitable runtime defect: reconcile
the independently authored semantics and retain the reason and negative tests.

Reference semantics: [Tokio select](https://docs.rs/tokio/latest/tokio/macro.select.html),
[Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html), and
[Tokio JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html).
