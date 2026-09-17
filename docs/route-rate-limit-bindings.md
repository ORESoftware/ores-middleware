# Route rate-limit policy bindings

Route selection and rate-limit policy definition are separate authority boundaries.

`ores-middleware` owns the portable route selector, deterministic precedence rules, stable route-class identity, and the binding from a route class to a canonical rate-limit **policy ID**. `ores-rate-limit` remains the owner of the policy referenced by that ID: capacity, refill/window semantics, consistency, enforcement mode, storage/coordinator behavior, keying, and failure posture do not move into this contract.

The peer authorities are:

- `contracts/route-rate-limit/typespec/main.tsp`;
- `contracts/route-rate-limit/json-schema/authored.schema.json`.

Neither is generated from the other. `contracts/route-rate-limit/tjsv/mapping.json` admits their reviewed declarations through TJSV, and `contracts/route-rate-limit/fixtures/conformance.json` is the shared 15-case runtime/schema corpus.

## Wire shape

```json
{
  "schema": "ores.middleware.route-rate-limit-bindings/v1",
  "default_policy_id": "public-read-default",
  "routes": [
    {
      "route_class_id": "auth-login",
      "policy_id": "auth:login-strict",
      "selector": {
        "methods": ["POST"],
        "path_template": "/auth/login",
        "operation_id": "auth.login"
      }
    }
  ]
}
```

A route binding never embeds a `RateLimitPolicyV2`. Consumers resolve the returned `policy_id` against the canonical policy catalog supplied by `ores-rate-limit` / `.ores-rl.toml`. Missing policy IDs therefore remain a consumer deployment-admission error rather than causing `ores-middleware` to manufacture policy values.

## Stable route classes

`route_class_id` is a bounded lowercase identifier used for configuration inheritance, telemetry, generated ingress/LB output, and cross-runtime fixtures. It is not derived from a raw request path. A table may contain at most 128 route classes and each selector may list at most eight HTTP methods.

Route-class IDs are unique within one table. Exact duplicate selectors are rejected. Those two checks are semantic runtime admission because JSON Schema cannot express uniqueness by an object field without turning route identity into a map key.

## Selection rules

Every selector constraint that is present must match. Selection then uses the existing ORES specificity rules:

1. operation ID dominates path-only matches;
2. path selectors dominate method-only selectors;
3. more static path segments are more specific;
4. an exact framework-registered route-template match receives a small preference over fallback concrete-path matching;
5. method constraints add a final small specificity signal.

If two different route classes receive the same best score, resolution fails closed. The Rust and TypeScript errors emit sorted, de-duplicated `route_class_ids` and `policy_ids` so ambiguity evidence is deterministic across runtimes.

If no route-specific binding matches, `default_policy_id` is returned when explicitly configured. Without it, resolution returns no binding. The library never silently creates an unlimited/default policy.

## Path and identifier hardening

Selectors accept normalized absolute templates such as `/users/{id}` and `/users/:id`; a terminal `*` is the only catch-all. Query strings and fragments are forbidden in configured templates. Empty segments, dot segments, malformed parameter names, and nonterminal catch-alls fail runtime admission. Percent decoding and other HTTP canonicalization remain the trusted router/transport boundary's responsibility.

Policy IDs and route-class IDs are bounded stable identifiers. HTTP methods use bounded uppercase ASCII tokens. Operation IDs use the existing bounded ORES ASCII identifier vocabulary.

## Request admission hardening

Resolution is fail-closed even when a caller skips an explicit preflight validation call. Both public runtimes validate the binding table and the request metadata before selecting a policy. Invalid tables produce a table-scoped resolution error; invalid request metadata produces a request-scoped resolution error.

Request methods accept bounded ASCII HTTP tokens case-insensitively at match time. Request paths are bounded to 4096 Unicode scalar values, must be absolute, and reject fragments, CR/LF/NUL, repeated slash separators, and literal `.` / `..` segments. Query strings remain outside route matching and are ignored after the path boundary.

A consumer-supplied `route_template` is treated as trusted router metadata only after it passes the same canonical template grammar as configured selectors **and** describes the concrete request path. Equality between a supplied route template and a configured selector can add specificity, but it can no longer turn an unrelated concrete path into a match. Parameter placeholders also require a non-empty concrete segment.

The request metadata shape is independently authored in both peer authorities as `RouteRateLimitBindingRequest`; TJSV maps the request method/path scalars and model alongside the binding-table declarations. The additional `request-hardening.json` corpus fixes 15 cross-runtime cases covering method/path bounds, canonicalization hazards, route-template spoofing, and empty-parameter behavior.

## Fifteen-case conformance tranche

The original binding-table corpus fixes these reviewed behaviors:

1. exact route binding;
2. parameterized route matching with query removal;
3. operation-ID precedence;
4. explicit default fallback;
5. explicit no-default/unbound behavior;
6. fail-closed equal-specificity ambiguity;
7. versioned schema rejection;
8. route-class identifier rejection;
9. policy-ID rejection;
10. empty-selector rejection;
11. malformed HTTP-method rejection;
12. method-count bound;
13. duplicate route-class rejection;
14. duplicate-selector rejection;
15. dot-segment rejection.

The first twelve have schema/runtime verdicts where the peer authorities can express the rule. Duplicate semantic identities and normalized path semantics are additionally enforced by the runtime validators. A second exact 15-case request-admission corpus covers the request-side boundary described above; both corpora are consumed by the exact-source CI lane.

## Public runtimes

Rust exports `RouteRateLimitBindingTable` and related types from the crate root. TypeScript exposes `@oresoftware/ores-middleware/rate-limit-bindings`. Both consume the same fixture corpus in CI.

The older `rate_limit_routes` / `rate-limit-routes` API that embeds complete policy objects remains available for compatibility. New orchestration and generated infrastructure should prefer the ID-only binding surface so policy ownership stays with `ores-rate-limit`.
