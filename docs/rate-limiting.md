# Layered rate limiting

This document is the integration contract for the `ores-rate-limit` program. The
program is intended for every ORESoftware application, while each application
selects only the enforcement layers it needs.

The dedicated `ores-rate-limit` organization owns reusable policy, edge,
ingress, distributed-backend, and formal-model packages. This repository owns
the application/framework adapter surface. `ores-otel` owns observability,
`shared-auth` owns authenticated identity, `ores-redis-lru-cache` owns the
bounded local/Redis hybrid, and `ORESoftware/k8s-cluster` owns cluster ingress
and service-mesh deployment.

## Non-negotiable invariants

1. A raw email address, user ID, tenant ID, session ID, API key, or full client
   IP address MUST NOT be used as a Redis key, cache key, metric attribute, log
   field, or Pub/Sub payload.
2. Origin-side principals MUST be derived with HMAC-SHA-256 over a canonical,
   length-prefixed signal sequence. The namespace and key version are included
   in the MAC domain.
3. Production MUST use a stable secret-store-backed HMAC key. An unavailable
   key is an enforcement failure, never a reason to fall back to a raw key.
4. Forwarded client-address headers are trusted only when the immediate peer is
   inside an explicitly configured proxy CIDR.
5. Authorization-layer policies MUST NOT fail open.
6. A local cache MUST be bounded. The default and hard contract maximum are
   10,000 principals per process, with a TTL at least as long as the policy
   window.
7. Redis Pub/Sub is an invalidation accelerator, not a durable source of truth.
   Subscribers recover from reconnects and revision gaps by reading a bounded
   snapshot from the authoritative store.
8. A backend denial is never converted to an allow by a fallback path.
9. Every denial has a stable policy ID, layer, reason, `Retry-After`, and
   rate-limit metadata. Principal digests are deliberately excluded from normal
   logs and metrics.

## Enforcement layers

| Layer | Best signals | State | Intended role |
| --- | --- | --- | --- |
| Cloudflare edge | IP prefix, route, method | Cloudflare native rate-limit binding plus short-lived Cache API block hints | Drop anonymous floods before origin compute. Edge decisions are coarse and location-aware, not a global account ledger. |
| Kubernetes ingress | trusted client IP, route, method, selected signed entitlement headers | NGINX shared memory, Envoy Gateway local/global service, or HAProxy stick tables | Protect clusters and services before application runtime. |
| Service mesh | service identity, caller workload, route, method | Envoy/Cilium local policy or a central rate-limit service | Bound service-to-service fan-out and expensive internal RPCs. |
| Application middleware | HMAC principal from auth and network signals | bounded local token bucket and optional distributed limiter | Apply product-specific route, tenant, user, API-key, and cost rules. |
| Authorization | subject, organization, session, credential class, risk event | strict distributed limiter plus durable lockout record for high-risk flows | Protect login, magic-link, OTP, password-reset, token exchange, and introspection paths. |
| Durable abuse controls | account/organization/risk case | Postgres or another reviewed durable contract | Multi-hour/day lockouts, investigations, entitlements, and auditable operator overrides. |

Cloudflare Cache API entries may carry a short-lived opaque block marker, policy
revision, and expiry. They MUST NOT be used as an atomic global counter. Where a
strict edge-global decision is required, shard a Durable Object or delegate to
the origin/distributed service. The native Workers Rate Limiting binding is the
preferred no-Redis counter at the edge; its per-location behavior must remain
visible in policy documentation.

The Kubernetes target set is deliberately plural:

- **ingress-nginx** for the existing cluster edge, using native shared-memory
  request zones for node-local protection and an external service only when a
  policy requires global coordination;
- **Envoy Gateway** using `BackendTrafficPolicy` for local limits and its
  external rate-limit service integration for global policies;
- **HAProxy Ingress** using stick tables for node-local limits and a reviewed
  external backend for strict global policies.

All three are disabled-by-default overlays. An application opts in by policy ID
and route selector; no blanket annotation is added to every workload.

## Principal derivation

The canonical principal is:

```text
HMAC-SHA-256(
  secret[keyVersion],
  length(namespace) || namespace ||
  length(keyVersion) || keyVersion ||
  for each configured signal in order:
    length(signalTag) || signalTag ||
    length(normalizedValue | "<missing>") || normalizedValue | "<missing>"
)
```

Normalization rules:

- email: trim and lowercase;
- method: uppercase;
- IPv4 prefix: `/24` by default;
- IPv6 prefix: `/56` by default;
- route: the registered route template when available, otherwise the path;
- subject/user/tenant/organization/session/device/API-key ID: the canonical ID
  established by `shared-auth`, never a display name or bearer secret.

At least one principal signal is required. Route and method alone cannot create
a principal. Cloudflare-edge policy accepts only `ip`, `ip-prefix`, `route`, and
`method`; authenticated signals are derived at a trusted origin boundary.

HMAC rotation is versioned. During a controlled rotation a distributed backend
may accept both the current and immediately previous version for the maximum
policy TTL, but new writes use only the current version. Rotation never emits a
mapping between old and new digests to logs or metrics.

## Algorithms

- **Token bucket**: default for short bursts and the only local fallback in the
  initial Rust adapter.
- **Sliding-window counter**: preferred distributed algorithm for user/account
  quotas where boundary bursts matter.
- **Fixed window**: acceptable for coarse edge protection and inexpensive
  operational limits; never presented as a strict rolling quota.
- **Concurrency**: bounds simultaneous expensive operations. Its permit release
  path must be cancellation- and panic-safe.

The policy contract includes `capacity`, `refillPerSecond`, `windowMs`, and a
unit `cost`. Future adapters may expose route cost tables, but unknown costs must
fail validation rather than silently becoming free.

## Bounded local cache plus Redis

The local cache exists for latency and availability, not global accuracy.

1. A request derives an opaque principal digest.
2. A local bounded token bucket makes the fast-path decision.
3. A node batches deltas to Redis with an idempotent batch ID and policy
   revision. Redis applies each batch atomically.
4. When the global policy is exceeded, Redis publishes an opaque block event
   containing principal digest, policy ID, monotonically increasing revision,
   expiry, and reason code.
5. Subscribers install the block into their local bounded cache.
6. A subscriber that observes a revision gap, reconnects, or starts cold reads
   a bounded snapshot before trusting subsequent Pub/Sub events.

Pub/Sub messages never contain raw identity. The authoritative Redis operation
must be atomic (Lua/function/transaction) and return a complete decision; a
read-then-increment sequence is forbidden. Batch retry is idempotent so a
network retry cannot double-count.

A 10,000-entry process cache is suitable for short hot sets and approximately
one-second windows. It is not a one-million-user ledger. Long windows, strict
cross-node quotas, and durable account lockouts remain centralized. The eviction
policy can reduce protection for a cold principal, so eviction is observable as
an aggregate metric and must not disable the ingress/edge safety net.

## Failure modes

| Mode | Primary backend unavailable | Allowed use |
| --- | --- | --- |
| `fail-open` | return `degraded-allowed` | low-risk read traffic only; never authorization |
| `fail-closed` | return `degraded-denied` and HTTP 503 | auth, write, billing, privileged, and strict entitlement paths |
| `local-only` | use bounded local token bucket; deny if the fallback is unavailable | ordinary APIs that tolerate a documented bounded overshoot |

A normal exhausted policy returns HTTP 429. An enforcement dependency failure
under fail-closed returns HTTP 503 so clients and operators can distinguish
quota exhaustion from unavailable enforcement.

## Response and telemetry contract

Denied responses include:

- `Retry-After` in whole seconds;
- `RateLimit-Limit`;
- `RateLimit-Remaining`;
- `RateLimit-Reset` when known;
- `X-Ores-Rate-Limit-Policy`;
- `X-Ores-Rate-Limit-Layer`;
- `X-Ores-Rate-Limit-Decision`.

`ores-otel` events use low-cardinality fields: policy ID, layer, algorithm,
outcome, source, remaining bucket, retry class, and reason code. They carry the
normal request/trace correlation context. Raw signals and principal digests are
not metric attributes and are not logged by default.

Recommended metrics:

```text
ores.rate_limit.decisions_total{policy,layer,algorithm,outcome,source}
ores.rate_limit.backend_errors_total{policy,backend,reason}
ores.rate_limit.local_cache_entries{policy}
ores.rate_limit.local_cache_evictions_total{policy}
ores.rate_limit.pubsub_revision_gaps_total{policy}
ores.rate_limit.batch_replays_total{policy,result}
```

## Application profiles

| Profile | Example use | Layers | Failure posture |
| --- | --- | --- | --- |
| `public-anonymous` | public docs/search/read routes | Cloudflare + ingress + application | edge/ingress coarse; application local-only |
| `authenticated-api` | normal signed-in reads/writes | edge anonymous shield + ingress + application | reads local-only; writes fail-closed when quota is contractual |
| `shared-auth-high-risk` | login, OTP, password reset, token exchange | edge + ingress + authorization + durable lockout | fail-closed |
| `expensive-compute` | media processing, graph builds, fabrication jobs | edge + ingress + application + concurrency | fail-closed admission; cancellation-safe permit release |
| `service-to-service` | internal RPC fan-out | service mesh + application | workload identity; fail-closed for writes |

`fiducia-cloud`, `sonus-auris`, `networking-components`, `quaestor-ledger`,
`daedalus-fab`, and other organizations select profiles in their own deployment
repositories. Limits are not copied into application code; applications import
the shared SDK and bind policy IDs to routes.

## Formal and test obligations

The rate-limit state machine is finite at the contract boundary:

```text
primary = allow | deny | unavailable
fallback = allow | deny | unavailable
failureMode = fail-open | fail-closed | local-only
layer = edge | ingress | mesh | application | authorization
result = allowed | denied | degraded-allowed | degraded-denied
```

CI exhaustively enumerates this bounded state space and checks:

- primary deny is never weakened;
- authorization never allows on unavailable enforcement;
- fail-closed never allows on unavailable enforcement;
- local-only allows only when the local fallback allows;
- every enum variant participates in an explicit match arm.

The real cache has concurrent-capacity, eviction-bound, TTL, retry-metadata, and
opaque-key tests. The TypeSpec and JSON Schema authorities are independently
human-authored; a Rust parity gate compares all rate-limit enum value sets and
model property/requiredness signatures. Any unexplained discrepancy stops the
pipeline for evaluation.

Distributed backends additionally require property/state-machine tests for:

- duplicate and reordered batch delivery;
- Pub/Sub loss, reconnect, and revision gaps;
- clock skew around window boundaries;
- simultaneous requests on multiple nodes;
- Redis failover during an atomic decision;
- HMAC key rotation;
- stale policy revision rejection;
- bounded memory under adversarial principal churn.

## Rollout order

1. Merge the typed middleware contract and Rust adapter behind explicit config.
2. Implement the Cloudflare worker and policy compiler without Redis at edge.
3. Add disabled-by-default ingress overlays for NGINX, Envoy Gateway, and
   HAProxy Ingress in `ORESoftware/k8s-cluster`.
4. Implement the revisioned Redis/LRU backend and its adversarial test suite.
5. Integrate strict authenticated policies in `shared-auth`.
6. Publish `ores-otel` semantic conventions and dashboards.
7. Roll out one application profile at a time, beginning in audit-only mode,
   then shadow decisions, then enforcement.

Audit-only and shadow modes emit decisions but do not block. They are forbidden
for routes whose existing security contract already requires strict lockout.
