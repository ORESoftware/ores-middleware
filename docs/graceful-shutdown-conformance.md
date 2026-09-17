# Graceful shutdown conformance

The versioned corpus in `contracts/graceful-shutdown/conformance.json` is the portable behavioral contract for graceful application-request draining. It complements the independently implemented runtime code; it does not move signal handling, TTY handling, listener shutdown, transport stream shutdown, telemetry flushing, resource closing, or process termination into `ores-middleware`.

## Canonical behavior

A coordinator begins in `running`, moves once to `draining`, and may escalate to `forced`. The default graceful deadline is 5000 ms. The default retry hint is 5000 ms. Requests admitted before the drain edge retain their lease; requests that lose admission after the edge are rejected with HTTP 429, the stable `service_draining` code, `application/problem+json`, `Cache-Control: no-store`, and a positive whole-second `Retry-After` rounded up from milliseconds.

Generic rejection metadata must never contain hop-by-hop `Connection`. A concrete HTTP/1.0 or HTTP/1.1 adapter may add `Connection: close` while draining. HTTP/2 and HTTP/3 adapters must omit it.

A graceful deadline that expires with requests still active escalates the coordinator to `forced`. An explicit force transition does the same immediately. A zero drain timeout with active work therefore times out and forces immediately. A drain with no active work completes immediately but stays in the non-accepting `draining` phase.

## Ownership boundary

`ores-middleware` owns application admission, in-flight accounting, and the canonical application-level rejection. Consumer process/server code owns signal policy, TTY policy, listener/ingress shutdown, protocol/connection drain, telemetry flush, external resource closure, and final process exit. That separation avoids a second lifecycle authority inside middleware and keeps HTTP/2+ stream teardown with the transport that actually owns those streams.

## Original fifteen hardening checks

The first conformance tranche makes the following invariants executable:

1. the corpus has a stable version identifier;
2. phase vocabulary is exactly running/draining/forced;
3. default drain timeout is 5000 ms;
4. default retry hint is 5000 ms;
5. draining admission status is HTTP 429;
6. the stable rejection code is `service_draining`;
7. `Retry-After` rounds fractional seconds upward and remains positive;
8. generic rejection headers exclude `Connection`;
9. HTTP/1.0 drain rejection requires connection close;
10. HTTP/1.1 drain rejection requires connection close;
11. HTTP/2 drain rejection forbids connection close;
12. HTTP/3 drain rejection forbids connection close;
13. successful completion before the deadline reports drained without forcing;
14. deadline, zero-timeout, and explicit-force cases end in forced state as specified;
15. CI validates the corpus and executes the native shutdown suites for framework-neutral Rust, Axum, TypeScript/Node, and Go.

## Fifteen additional negative controls

The validator is intentionally fail-closed. A dedicated `node:test` suite mutates the canonical corpus one invariant at a time and requires every mutation to be rejected:

1. unknown top-level contract fields;
2. unknown constant fields;
3. phase vocabulary drift;
4. duplicate retry-delay milliseconds;
5. incorrect `Retry-After` rounding;
6. unreviewed lifecycle cases;
7. lifecycle action drift;
8. lifecycle remaining-count drift;
9. rejection metadata attached to the running phase;
10. non-canonical generic header casing;
11. hop-by-hop `Connection` in generic metadata;
12. unknown transport protocols;
13. duplicate transport names;
14. overlap between middleware-owned and consumer-owned lifecycle responsibilities;
15. omission of the consumer-owned `resource-close` boundary.

The strict validator also requires exact key sets, exact reviewed lifecycle/admission/transport case sets, unique ordered retry vectors, unique ownership entries, and the canonical ownership lists. Expanding the versioned corpus therefore requires an explicit validator change rather than silently broadening behavior.

## Exact-source CI provenance

For pull requests, the graceful-shutdown workflow checks out `github.event.pull_request.head.sha`; for pushes it checks out `github.sha`. Checkout credentials are not persisted, fetch depth is one, and CI verifies `git rev-parse HEAD` equals the selected source SHA before running validation or native tests. This keeps the source under test unambiguous instead of relying on GitHub's synthetic pull-request merge ref.

The runtime implementations remain independent. Their native tests exercise each language's real scheduling, timer, cancellation, request-accounting, and HTTP behavior while the versioned corpus fixes the common observable semantics they must preserve.
