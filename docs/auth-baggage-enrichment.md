# Auth baggage enrichment

`ores-middleware` treats the authentication provider as a consumer-owned boundary. Authentication may return identity metadata, claims, or provider-specific baggage, but that data is **not** copied wholesale into generic request context, middleware attributes, logs, or tracing fields.

The default is intentionally empty propagation. Consumers opt in to the exact fields they need.

## Why

Authentication payloads routinely contain values that must not cross generic middleware or observability boundaries: bearer tokens, cookies, session identifiers, refresh tokens, provider payloads, and other credentials. A generic middleware library cannot know which provider-specific claims are safe for every consumer.

The safe rule is therefore:

1. authenticate using the consumer-owned provider/version;
2. establish the normalized user and tenant identity;
3. run an explicit enrichment callback;
4. copy only an allow-listed subset into request baggage or observability context.

## Gleam

`Hooks.auth_baggage_enricher` receives the request, current request context, and `AuthDecision` and returns the baggage that may cross the boundary. `default_hooks()` returns an empty dictionary, so provider baggage is not propagated unless a consumer explicitly opts in.

```gleam
let hooks0 = ores_middleware.default_hooks()
let hooks = ores_middleware.Hooks(
  ..hooks0,
  authenticate: my_authenticate,
  auth_baggage_enricher: fn(_, _, auth: ores_middleware.AuthDecision) {
    case dict.get(auth.baggage, "otel.account_tier") {
      Ok(value) -> dict.from_list([#("otel.account_tier", value)])
      Error(_) -> dict.new()
    }
  },
)
```

Do not copy the complete `auth.baggage` dictionary. Build a fresh dictionary from explicit keys.

The same enrichment callback is used for normal authentication and test-auth-bypass identity resolution, which keeps the security boundary consistent across both paths.

## TypeScript

Use `authBaggageEnricher` for the same purpose. The consumer decides which provider-specific fields are safe to copy. Omitting the enricher means no auth-provider baggage is implicitly exposed through the middleware context.

## Cross-language contract

Language adapters may represent the callback differently, but they should preserve these semantics:

- identity (`user_id` / `tenant_id`) is normalized separately from arbitrary claims;
- arbitrary auth-provider baggage is not propagated by default;
- enrichment is consumer-owned and allow-list based;
- secrets such as authorization headers, cookies, bearer tokens, refresh tokens, and session credentials must not be implicitly propagated;
- test/bypass providers follow the same enrichment boundary as production providers;
- middleware ordering remains consumer-owned and independent of the auth SDK version.

Rust's concrete HTTP pipeline may expose its own deliberately allow-listed normalized claim projection, but generic stage attributes still must not receive provider claims automatically.
