# Graceful shutdown conformance

The versioned corpus in `contracts/graceful-shutdown/conformance.json` is the portable behavioral contract for graceful application-request draining. It complements the independently implemented runtime code; it does not move signal handling, TTY handling, listener shutdown, transport stream shutdown, telemetry flushing, resource closing, or process termination into `ores-middleware`.

## Canonical behavior

A coordinator begins in `running`, moves once to `draining`, and may escalate to `forced`. The default graceful deadline is 5000 ms. The default retry hint is 5000 ms. Requests admitted before the drain edge retain their lease; requests that lose admission after the edge are rejected with HTTP 429, the stable `service_draining` code, `application/problem+json`, `Cache-Control: no-store`, and a positive whole-second `Retry-After` rounded up from milliseconds.

Generic rejection metadata must never contain hop-by-hop `Connection`. A concrete HTTP/1.0 or HTTP/1.1 adapter may add `Connection: close` while draining. HTTP/2 and HTTP/3 adapters must omit it.

A graceful deadline that expires with requests still active escalates the coordinator to `forced`. An explicit force transition does the same immediately. A zero drain timeout with active work therefore times out and forces immediately. A drain with no active work completes immediately but stays in the non-accepting `draining` phase.

## Ownership boundary

`ores-middleware` owns application admission, in-flight accounting, and the canonical application-level rejection. Consumer process/server code owns signal policy, TTY policy, listener/ingress shutdown, protocol/connection drain, telemetry flush, external resource closure, and final process exit. That separation avoids a second lifecycle authority inside middleware and keeps HTTP/2+ stream teardown with the transport that actually owns those streams.

## Fifteen hardening checks

This conformance tranche makes the following invariants executable:

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
15. CI validates the corpus and executes the native shutdown suites for framework-neutral Rust, Axum, TypeScript/Node, and Go from the pull-request merge revision.

The runtime implementations remain independent. Their native tests exercise each language's real scheduling, timer, cancellation, request-accounting, and HTTP behavior while the versioned corpus fixes the common observable semantics they must preserve.
