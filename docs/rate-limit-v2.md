# Rate-limit V2 contract and middleware order

This additive contract hardens rate limiting without silently changing the existing `1.0.0` middleware wire surface. TypeSpec and JSON Schema Draft 2020-12 are independently authored peer authorities. The JavaScript gate compares normalized enum and property sets, validates positive and negative fixtures, and writes a sanitized receipt. Rust independently deserializes and validates the same semantic invariants at runtime.

## Visible Rust execution order

`src/rust/src/middleware_order.rs` exposes `DEFAULT_MIDDLEWARE_ORDER` as an execution trace:

1. panic boundary;
2. deadline;
3. request ID;
4. trace context;
5. trusted-proxy validation;
6. transport security;
7. payload limit;
8. anonymous flood guard;
9. authentication;
10. authenticated-principal rate limit;
11. authorization;
12. idempotency;
13. handler;
14. response compression;
15. security headers;
16. telemetry finalization.

Framework adapters may have different lexical nesting, but their observable execution trace must satisfy `validate_middleware_order`. In particular, forwarded client identity is unusable until the immediate peer passes trusted-proxy validation, and principal limiting cannot run until authentication establishes a canonical subject.

## Consistency and failure behavior

- `strict`: one atomic coordinator owns every decision, maximum overshoot is zero, and backend failure is fail closed.
- `bounded`: local decisions are allowed only with an explicit positive overshoot bound and finite denial cache.
- `advisory`: telemetry or coarse protection only; it cannot claim global authority or turn coordinator failure into a global denial.

Authentication recovery, mutations, payment or ledger writes, and job admission are always strict, coordinated, and fail closed. Authentication attempts are also strict, but a coarse edge guard may deny obvious floods before credential work. Edge enforcement cannot own denial for recovery, mutation, payment/ledger, or job-admission operations; those decisions remain application/coordinator responsibilities.

## Algorithms

V2 declares token bucket, sliding-window counter, fixed window, GCRA, and concurrency. Token bucket requires refill quantity and interval and forbids a window. Sliding window, fixed window, and GCRA require a window and forbid refill fields. Concurrency permits are cancellation- and panic-sensitive and therefore forbid refill/window fields.

The deterministic `ores-rate-limit/ores-rl-lib-core` package remains the intended shared algorithm implementation. Its private Zed publication is a separate provenance gate; this public contract does not copy or expose private core source.

## Identity and telemetry

Policies contain only opaque identifiers and key-version metadata. Raw email addresses, IP addresses, authentication subjects, tenant IDs, bearer tokens, cookies, and API keys are prohibited in rate-limit state, Redis keys, Pub/Sub payloads, logs, traces, metrics, and retained receipts.

## Verification

```sh
npm ci --ignore-scripts
npx tsp compile contracts/rate-limit-v2/typespec --no-emit
node scripts/validate-rate-limit-v2.mjs
cargo fmt --manifest-path src/rust/Cargo.toml -- --check
cargo clippy --manifest-path src/rust/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src/rust/Cargo.toml --all-features
java -cp /tmp/tla2tools.jar tlc2.TLC \
  -config formal/rate-limit-pipeline/RateLimitPipeline.cfg \
  formal/rate-limit-pipeline/RateLimitPipeline.tla
```

The initial organization rollouts remain audit-only. Production enforcement requires a separate reviewed change after trusted-proxy behavior, coordinator capacity, reconnect repair, telemetry cardinality, operation classification, and rollback behavior are verified from real traffic.

## Full peer-authority TJSV reconciliation

The V2 policy now exposes a named `RateLimitPolicyV2` declaration in the authored Draft 2020-12 schema and the TypeSpec authority carries the same numeric bounds and conditional policy invariants already enforced by JSON Schema and Rust runtime validation. The authored root still validates through `$ref`, so the wire shape is unchanged; naming the root only makes declaration identity explicit for cross-authority evidence.

`contracts/rate-limit-v2/tjsv/mapping.json` maps all seven TypeSpec/generated/authored declarations one-to-one. All three ignore lists are intentionally empty. The dedicated `rate-limit-v2-peer-authority` workflow uses TJSV with `--int64-strategy=number`, because these bounded `uint64` fields are JSON integer wire values rather than decimal strings. It requires seven compared declarations, synthesized differential probes with zero divergence/refusal, zero unexplained findings, verified Contract IR, and an intentional numeric-bound drift that must stop evaluation.
