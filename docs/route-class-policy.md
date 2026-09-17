# Route class policy inheritance

`route_class_id` is classified once at the trusted route/operation boundary and then reused for middleware policy lookup. This layer does **not** inspect raw user, tenant, query, or path values and does not create a second route matcher.

## Ownership

- route/operation classification produces a stable `route_class_id`;
- this contract maps that class through bounded inheritance to named policy references;
- provider/config owners still define the referenced auth, authorization, rate-limit, cache, timeout, CORS, CSRF, validation, resilience, and idempotency policies;
- numeric quotas, credentials, provider internals, and secret values never move into this contract.

The wire contract is `ores.middleware.route-class-policies/v1`. TypeSpec and independently authored Draft 2020-12 JSON Schema are peer authorities; TJSV is the fail-closed parity gate.

## Bounded inheritance

A table contains at most 64 route classes and an inheritance chain may contain at most eight classes. Class ids are stable lowercase identifiers. Parents must exist and cycles fail closed.

Resolution starts from the selected class (or `default_class_id` when classification produced no explicit class), walks to the root, and overlays only the explicit policy-reference fields in root-to-leaf order.

The allowed override fields are intentionally closed:

- `auth_policy_id` and `authorization_policy_id`;
- `rate_limit_policy_id` and `cache_policy_id`;
- `timeout_policy_id` and `resilience_policy_id`;
- `cors_policy_id` and `csrf_policy_id`;
- `validation_policy_id` and `idempotency_policy_id`.

No arbitrary key/value bag is accepted.

## Security-critical overrides

For an inherited class, changing auth, authorization, CSRF, validation, or idempotency references requires an explicit `security_override_intent`:

- `preserve-or-strengthen` means the consumer asserts the reviewed replacement is not a weakening;
- `weaken` additionally requires `security_exception` metadata containing a bounded exception id, tracking ticket id, and non-empty reason.

The exception metadata is an audit reference only. It must not contain credentials, tokens, user data, or unrestricted request content.

## Composition with route classification

The existing route-binding resolver already establishes deterministic precedence for operation id, route template, and method constraints and fails closed on equal-specificity ambiguity. Route-class policy inheritance starts **after** that classification result. This avoids two middleware subsystems independently deciding which path a request belongs to.

A typical flow is:

```text
trusted router / generated operation metadata
  -> stable route_class_id
  -> bounded route-class inheritance
  -> resolved named policy references
  -> provider-specific policy resolution
  -> middleware execution
```

If no explicit class is produced, the policy table's named default class is used. An unknown explicit class is a resolution error rather than an implicit fallback.

## Conformance

`contracts/route-class-policy/fixtures/conformance.json` contains exactly 15 shared cases covering inheritance, default fallback, critical preserve/strengthen overrides, reviewed weakening, missing intent, missing exception, missing parents, cycles, excessive depth, duplicate classes, missing defaults, invalid policy ids, unknown requested classes, and table-count bounds.

Rust and TypeScript consume the same corpus. The exact-head workflow also runs AJV against the authored schema, compiles TypeSpec, runs TJSV with a deliberate drift canary, and executes Rust formatting/Clippy/tests.
