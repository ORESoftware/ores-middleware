# Deterministic rate-limit core adapter

`CoreInMemoryRateLimiter` connects the existing polyglot middleware contract to the deterministic Rust transitions in `ores-rate-limit/ores-rl-lib-core`.

## Dependency and release contract

The repository-root `.zpkg.toml` is the canonical cross-repository dependency declaration. The Rust `Cargo.toml` additionally pins the currently validated core commit so pull-request CI can compile before the first reviewed `v0.1.0` package release. After the zed-pkg artifact is published, update the zed lockfile and replace the temporary Git revision with the generated local zed-pkg path/source required by the Rust adapter workflow.

Do not copy the core source into this repository.

## Runtime role

The adapter delegates token-bucket, fixed-window, and weighted sliding-window decisions to the shared core. Concurrency limiting is intentionally rejected because it requires a separate cancellation- and panic-safe permit lifecycle.

The local state store:

- accepts only 64-character lowercase hexadecimal HMAC digests;
- is scoped by policy ID and opaque principal;
- defaults to 10,000 entries with a 30-second inactivity TTL;
- preserves future-timestamp entries so monotonic-clock regressions fail explicitly;
- reports exhaustive core errors through stable middleware reason codes.

It is a process-local hot set and fallback, not a strict global quota. A horizontally scaled service can overshoot by the aggregate local capacity during coordination delay.

## Identity and storage flow

```text
trusted IP or verified Shared Auth subject/tenant
  -> canonical length-prefixed HMAC-SHA-256 scope
  -> 32-byte / 64-lowerhex opaque principal
  -> deterministic local state transition
  -> optional ores-redis-lru-cache deny propagation
  -> low-cardinality ores-otel decision event
```

`shared-auth` supplies only verified stable identity material. Middleware owns canonical HMAC derivation. `ores-redis-lru-cache` stores only bounded opaque deny markers and remains non-authoritative. `ores-otel` receives policy, layer, algorithm, outcome, source, retry class, and request/trace correlation—never raw IP, email, token, subject, tenant, or full principal digest.

## Strict quotas

Billing, entitlement, scarce-resource allocation, long-duration lockouts, and irreversible writes require an atomic distributed or transactional data-store limiter. Do not represent the local adapter or Redis Pub/Sub deny cache as exact accounting.
