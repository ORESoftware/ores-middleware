# Edge middleware placement and route-specific rate limits

## Ownership

`ORESoftware/ores-middleware` owns portable middleware interfaces, execution-placement metadata, framework/edge adapters, and the runtime route-policy resolver.

Consumer repositories own middleware selection and ordering. For product organizations that means the `*-infra` repository and its `.ores-mw.toml` orchestration. The orchestration file identifies the execution target and stack configuration; it is not a third policy authority.

Rate-limit policy remains owned by the `ores-rate-limit` program and `.ores-rl.toml`. Applications and infrastructure bind policy IDs to routes. `ORESoftware/k8s-cluster`, Cloudflare Workers, ingress controllers, service mesh components, and Rust load balancers are execution targets rather than policy authorities.

## Execution placements

The shared Rust model exposes three placements:

- `edge`: Cloudflare Worker, Kubernetes ingress, or an ORES load balancer;
- `transport`: service mesh and transport-specific admission/backpressure;
- `application`: web/API server middleware where authenticated identity, application state, and database-backed checks are available.

A middleware implementation declares the placements it supports. Consumers decide where to run it. Database-dependent middleware is rejected outside the application boundary, and middleware requiring trusted authenticated identity must not assume origin identity at the Cloudflare edge.

The same conceptual stage may run at multiple boundaries when that is intentional. For example request IDs and trace propagation can run at edge and application boundaries. Implementations should detect already-established canonical ORES metadata rather than generate unrelated duplicate context.

## Route-specific rate limiting

A single global request rate is not sufficient. Different operations have different cost, abuse risk, and consistency requirements. The runtime resolver therefore selects a rate-limit policy before any bucket is consumed.

Resolution input is:

- HTTP method;
- concrete request path;
- registered route template when the framework or generated router knows it;
- stable RPC/API operation ID when available.

A selector may constrain any combination of method, path template, and operation ID. Every specified constraint must match. The most specific matching selector wins. Operation ID is intentionally stronger than a path-only match. Static route segments are more specific than parameterized segments. An exact registered route-template match receives a small preference over a fallback concrete-path template match.

If equally specific policies match, resolution returns an ambiguity error instead of silently selecting one. This is a deployment/configuration failure and must be surfaced before or during admission; an enforcement path must not pick an arbitrary limit.

If no route-specific selector matches, the configured default policy is used. If no default exists, the result is `None` and the caller can apply its explicit no-policy posture. Consumers must not silently manufacture an unlimited default.

## Example policy binding

The authored `.ores-rl.toml` should bind routes to canonical policy IDs rather than duplicating rate-limit implementation logic in application code. The exact TOML schema remains governed by the `ores-rate-limit` TypeSpec and JSON Schema authorities, but the intended shape is:

```toml
schema_version = 1
default_policy_id = "public-read-default"

[[route_bindings]]
policy_id = "search-read"
methods = ["GET"]
path_template = "/search"

[[route_bindings]]
policy_id = "login-attempt"
methods = ["POST"]
path_template = "/auth/login"
operation_id = "auth.login"

[[route_bindings]]
policy_id = "ledger-entry-read"
methods = ["GET"]
path_template = "/ledger/{ledger_id}/entries/{entry_id}"
```

Those policy IDs can have materially different limits, algorithms, and failure postures. For example a public search route may use a relatively generous bounded token bucket, while `auth.login` should use a strict coordinated policy with a much smaller capacity and fail-closed semantics.

## Runtime example

```rust
use ores_middleware::{
    RouteRateLimitRequest, RouteRateLimitTable,
};

let selected = table.resolve(&RouteRateLimitRequest {
    method: "POST",
    path: "/auth/login",
    route_template: Some("/auth/login"),
    operation_id: Some("auth.login"),
})?;

if let Some(selected) = selected {
    // Derive the opaque principal, then consume the bucket using
    // selected.policy. The route resolver never stores a raw identity key.
}
```

The canonical order is:

```text
normalize trusted request metadata
    -> identify route / operation
    -> resolve route-specific rate-limit policy
    -> derive opaque principal key
    -> consume policy bucket / concurrency permit
    -> emit standardized rate-limit decision metadata
```

Route selection must happen before bucket consumption. The selected policy ID is part of the bucket namespace so two routes with different limits never accidentally share state unless the authored policy explicitly intends them to.

## Path-template rules

The Rust adapter accepts absolute route templates such as `/users/{id}` or `/users/:id`. A terminal `*` may be used as a catch-all. Query strings and fragments are not part of route matching. Prefer the framework's registered route template over matching a raw concrete path, because templates avoid high-cardinality policy state and produce stable telemetry.

Percent-decoding and path canonicalization belong to the trusted HTTP/router boundary. The rate-limit resolver must receive the same normalized path representation used for routing; it must not independently reinterpret encoded separators or dot segments.

## Ingress and load-balancer behavior

Edge targets should normally use only edge-safe signals: IP/prefix, route, and method. Authenticated user/tenant/account signals are resolved at a trusted application or authorization boundary unless a reviewed signed identity assertion is explicitly part of the ingress contract.

For Kubernetes ingress and an ORES Rust LB, compile route bindings into the target's native representation where possible. Do not copy numeric limits into Kubernetes YAML by hand. Generated ingress configuration should retain the canonical policy ID so telemetry and application decisions can be correlated across layers.

Cloudflare, ingress, and application rate limits do not need identical numeric thresholds. They may intentionally use separate policy IDs for coarse flood protection versus strict account-level quotas. What must remain shared is the contract, route identity, policy identity, decision metadata, and validation rules.

## Consumer repository convention

A product `*-infra` repository should use this layout when it owns edge execution:

```text
*-infra/
  .ores-mw.toml
  .ores-rl.toml
  edge/
    middleware.rs        # Rust LB/ingress composition root when applicable
    cloudflare.ts        # Worker adapter when applicable
    generated/           # read-only generated target policy/config
  modules/
    cloudflare_routing/
    kubernetes_ingress/
    load_balancer/
  environments/
    dev/
    stage/
    prod/
```

`edge/middleware.*` is the ORES analogue of a Next.js `middleware.ts` / `proxy.ts`: it is the consumer composition root. Reusable implementations stay in `ores-middleware`; detailed rate-limit policy stays in `ores-rate-limit`; target deployment logic stays in `*-infra` and `k8s-cluster`.
