# Deterministic rate-limit core adapter

`ores-middleware-rate-limit-core` connects the public middleware contract to
the deterministic Rust transitions in the private
`ores-rate-limit/ores-rl-lib-core` package.

## Semantic salvage

The original work in PR #27 put a private Cargo Git dependency directly in the
main Rust middleware crate. The referenced commit exists, but ordinary
cross-organization GitHub Actions jobs cannot authenticate that transport.
Merging it would also place a private distribution dependency inside the
framework-neutral package.

The replacement preserves the useful adapter implementation and its history,
but moves it to:

```text
integrations/ores-rate-limit-core-rust/
```

The primary `src/rust` crate remains independently buildable. Applications that
need the deterministic core opt into the integration crate.

## Zed is the dependency authority

The repository-root `.zpkg.toml` contains the only cross-repository dependency
declaration:

```toml
[dependencies]
"ores-rate-limit/ores-rl-lib-core" = "^0.1.0"
```

The integration crate consumes the package from the configured Zed installation
tree:

```text
.vendor/.zed/ores-rate-limit/ores-rl-lib-core
```

A frozen `.zpkg.lock` must bind all of the following before compilation or
promotion:

- package identity and exact version;
- artifact SHA-256 and byte size;
- archive format and source;
- VCS tag;
- exact VCS commit.

The first admitted release is
`ores-rate-limit/ores-rl-lib-core@0.1.0`, annotated tag `v0.1.0`, at reviewed
merge commit `cfc81aef5d1de60ff6c46798745a6b3f970bc39d`. The tag was created only
after the source, live Redis, backend-free, formal, Zed pack, archive, and local
consumer checks passed. Remote publication is still fail-closed because the
protected `zed-pkg` environment did not provide `ZED_PKG_TOKEN`; no lock may be
created until that approved secret channel is repaired and the package is
actually published.

There is no direct Cargo Git fallback, authenticated Git URL, copied source
fallback, or branch-based dependency. Missing publication, lock, authorization,
artifact, provenance, or installed path is a release-blocking failure.

## Runtime role

The adapter delegates token-bucket, fixed-window, and weighted sliding-window
decisions to the shared deterministic core. Concurrency limiting is
intentionally rejected because it requires a separate cancellation- and
panic-safe permit lifecycle.

The local state store:

- accepts only 64-character lowercase hexadecimal HMAC digests;
- is scoped by policy ID and opaque principal;
- defaults to 10,000 entries with a 30-second inactivity TTL;
- preserves future-timestamp entries so monotonic-clock regressions fail;
- reports exhaustive core errors through stable middleware reason codes.

It is a process-local hot set and fallback, not a strict global quota. A
horizontally scaled service can overshoot by the aggregate local capacity during
coordination delay.

## Identity and storage flow

```text
trusted IP or verified Shared Auth subject/tenant
  -> canonical length-prefixed HMAC-SHA-256 scope
  -> 32-byte / 64-lowerhex opaque principal
  -> deterministic local state transition
  -> optional ores-redis-lru-cache denial propagation
  -> low-cardinality ores-otel decision event
```

`shared-auth` supplies only verified stable identity material. Middleware owns
canonical HMAC derivation. `ores-redis-lru-cache` stores only bounded opaque
denial markers and remains non-authoritative. `ores-otel` receives policy,
layer, algorithm, outcome, source, retry class, and request/trace correlation,
never raw IP, email, token, subject, tenant, or a full principal digest.

## Admission gate

The dedicated workflow must:

1. verify the root Zed dependency and absence of private Git fallbacks;
2. validate the exact frozen lock, `v0.1.0` tag, and commit
   `cfc81aef5d1de60ff6c46798745a6b3f970bc39d`;
3. install with `zed install --frozen --install-mode copy`;
4. prove the installed package manifest exists;
5. compile with warnings denied and run the adapter tests;
6. rerun the main middleware Rust tests;
7. retain sanitized lock and validation receipts.

The current release blocker is tracked by
`ores-rate-limit/ores-rl-lib-core#7`; the semantic salvage is tracked by
`ORESoftware/ores-middleware#37`.

## Strict quotas

Billing, entitlement, scarce-resource allocation, long-duration lockouts, and
irreversible writes require an atomic distributed or transactional data-store
limiter. Neither this local adapter nor Redis Pub/Sub denial propagation may be
represented as exact accounting.
