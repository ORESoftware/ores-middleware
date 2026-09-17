# Complete ORES middleware capability profile

This document is a reviewed **reference composition** for deployable HTTP/RPC servers that use `ORESoftware/ores-middleware`. It is not a mandatory library-owned stack. A consuming service owns which capabilities it enables, their exact order, their route/server scope, and the provider implementations/versions injected behind ORES ports.

`ores-middleware` is a lifecycle/composition toolkit. It must not silently reimplement the authoritative packages it composes:

- shared value/request contracts: `oresoftware/ores-interfaces`
- admission/rate limiting: `ores-rate-limit/ores-rl-lib-core`
- bounded local/Redis LRU cache profiles: `ores-redis-lru-cache/ores-lru-redis-lib-core`
- authentication/session verification: consumer-selected auth provider, optionally `shared-auth/shared-auth-lib-core`
- telemetry/log context: `ores-otel/ores.otel.log`

Consumers may use native package-manager projections, but `.zpkg.toml` is the cross-repository dependency declaration and provenance boundary.

For the implemented consumer-owned APIs, see [`consumer-composition.md`](consumer-composition.md): `StagePipeline`, injectable `AuthStage`, standalone Axum auth middleware, and `MiddlewareOrderPolicy`.

## Request data model

Every adapter that uses the portable ORES pipeline should project the incoming request into a data-only request envelope. The semantic contract belongs in `ores-interfaces`; language runtimes use idiomatic maps/types:

- `headers`: case-insensitive-on-input, canonical lowercase string map. Authored/generated names should be lowercase, including the reserved `x-ores-*` extension namespace.
- `query`: string or multi-string map.
- `json_payload`: typed `string -> JSON value` map when the decoded root is an object.
- `decoded_payload`: representation-tagged decoded value for JSON, XML, MessagePack, or Protobuf.
- `raw_body`: bounded bytes retained only when a downstream contract explicitly needs them.
- `attributes`: bounded middleware-owned typed metadata such as request ID, trace ID, authenticated subject, tenant, cache decision, and rate-limit decision.

The request envelope is per-request data. It is never a process-global mutable map and must not contain plaintext secrets in telemetry.

## Reviewed reference profile

The sequence below is a useful reviewed profile, **not** an instruction that every service must install all stages in this order. A service may omit stages, add custom stages, split the profile across routers, or choose another order when its semantics require it. Where one stage depends on another, the consumer should encode that relationship explicitly with its own composition/order policy.

1. **panic/error boundary** — convert uncaught failures to sanitized `5xx` problem responses.
2. **deadline/cancellation** — establish request deadline and cancellation propagation.
3. **request ID and trace context** — accept only valid inbound values; otherwise generate fresh values.
4. **trusted-proxy/TLS termination policy** — derive effective HTTPS only from a directly secure connection or forwarded metadata from an explicitly trusted proxy.
5. **request decompression** — decode supported `Content-Encoding` values before payload parsing, while enforcing compressed/expanded byte limits.
6. **payload byte limit** — bound the post-decompression payload before parser allocation.
7. **anonymous flood guard** — coarse abuse control where the service chooses to place it.
8. **content-type dispatch / deserialization** — JSON, XML, MessagePack, or Protobuf.
9. **authentication** — invoke the provider injected by the consumer; protected routes fail closed.
10. **principal rate limit** — optionally use the authenticated principal when the consumer places this stage after auth.
11. **authorization** — application/route policy, commonly after authentication when identity is required.
12. **cache lookup / conditional request** — authorization-sensitive data must be safely scoped or bypass shared caching.
13. **idempotency admission** — for configured mutation methods.
14. **redirect policy** — normalized, explicitly allowed destinations only.
15. **application handler**.
16. **catch-all / fall-through 4xx** — structured `404`/`405` where applicable.
17. **cache write / ETag finalization** — only when response/cache policy permits it.
18. **response compression** — negotiate an enabled algorithm and avoid double compression.
19. **encryption/wrapping** — explicit application/message encryption policy.
20. **security headers and correlation headers**.
21. **telemetry/finalization/cleanup** — emit bounded metadata and release resources.

The portable execution guarantee is simpler than this profile: request stages run in the order the **consumer declared**; response hooks unwind in reverse entered order. `DEFAULT_MIDDLEWARE_ORDER` / `validate_middleware_order(...)` are compatibility/reference helpers only. New consumers should use `MiddlewareOrderPolicy` / `validate_consumer_middleware_order(...)` when they want explicit validation of their own composition.

## Content representations

The portable middleware contract recognizes these canonical media types when the consumer enables corresponding codec stages:

| Representation | Canonical media type | Notes |
| --- | --- | --- |
| JSON | `application/json` | `application/problem+json` is the canonical structured error representation. |
| XML | `application/xml` | `text/xml` may be accepted as a compatibility alias but should normalize to `application/xml`. Disable DTD/external-entity resolution. |
| MessagePack | `application/msgpack` | `application/x-msgpack` may be accepted as a compatibility alias. |
| Protobuf | `application/protobuf` | `application/x-protobuf` may be accepted as a compatibility alias. Protobuf decoding requires a route/message descriptor. |

All parsers receive bounded bytes and should reject trailing garbage where the codec exposes that distinction.

## Encryption and compression

Compression and encryption are transforms around the representation codec, not alternate representations. If a consumer enables all of these transforms, their semantic relationship is:

Inbound:

`transport -> trusted TLS/proxy admission -> decrypt -> decompress -> representation decode -> contract validation`

Outbound:

`application value -> representation encode -> optional compress -> optional encrypt -> transport`

These local transform dependencies do not imply a universal 21-stage application chain; they apply only when those transforms are selected.

Authenticated encryption must be used for payload encryption. Nonce/key material is injected by an approved secret provider and is never committed to `.ores-*.toml`, `.zpkg.toml`, logs, or generated receipts.

## Rate limiting

`ores-rate-limit` owns rate/admission semantics. **The consuming service owns where each rate-limit stage is positioned and which routes use it.** `ores-middleware` provides adapters/primitives so consumers do not need to reimplement admission logic.

Common patterns include:

- anonymous flood limiting before expensive work;
- principal/tenant limiting after authentication when the key depends on authenticated identity;
- strict/fail-closed policy for security-sensitive operations;
- bounded/local degradation for explicitly configured public-read profiles.

Those are reviewed patterns, not hidden order rules. If a service requires auth before a principal-aware limiter, encode `auth -> limiter` in its own `MiddlewareOrderPolicy`.

A consumer should not instantiate a second unrelated token bucket when an ORES rate-limit adapter is configured for the same policy boundary.

## LRU / Redis cache

`ores-redis-lru-cache` owns cache profiles and Redis-backed bounded-LRU semantics. Middleware integrates it through a cache interface and should preserve:

- bounded entries and TTLs for local memory;
- namespace/versioned keys;
- tenant/user/authorization scope where data is not public;
- negative-cache policy as an explicit choice;
- stampede/single-flight behavior where provided by the cache package;
- no caching of `Set-Cookie`, secrets, or private responses unless an explicit route policy says otherwise.

A cache outage must not bypass authentication or authorization on routes where those controls are required.

## Authentication / Shared Auth

The consuming service owns the concrete authentication dependency and exact version/revision. `shared-auth` is an important ORES provider option, but `ores-middleware` must not make one auth SDK/version a transitive universal requirement.

The stable boundary is `AuthVerifier` / `AuthDecision`, with closure/provider adapters such as `auth_provider_fn(...)`. Consumers can therefore inject a selected Shared Auth build, another provider, or two incompatible provider versions during migration without changing the middleware crate.

When authentication is enabled for a protected route:

- normalized identity fields are attached to request context/extensions;
- provider-specific diagnostics are not exposed to clients;
- failures produce sanitized `401`/`403` responses as appropriate;
- the configured auth boundary fails closed;
- authorization can consume the stable identity established by auth when the consumer orders it that way;
- test bypasses are forbidden in production and remain explicit in test/staging configuration.

`AuthStage` copies stable user/tenant identity and only `otel.*` baggage by default. A service that needs additional provider claims for authorization should explicitly allow-list them with a decision enricher rather than copying arbitrary claims wholesale.

## TLS termination

TLS policy supports three deployment shapes:

- `in-process`: the service itself terminates TLS;
- `trusted-proxy`: Cloudflare/LB/ingress terminates TLS and forwarded transport metadata is accepted only from configured proxy CIDRs/identities;
- `disabled`: allowed only when HTTPS is not required by policy (normally local/test or a separately verified secure transport).

Never trust `x-forwarded-proto`, `forwarded`, client-IP headers, or similar identity-bearing metadata from an untrusted direct peer.

## Redirects

Redirect middleware is policy, not string concatenation. The default posture is same-origin relative redirects. External redirects require an allowlist. Reject:

- `javascript:`, `data:`, `file:` and unknown schemes;
- protocol-relative `//host/path` destinations unless explicitly normalized and allowlisted;
- embedded credentials;
- CR/LF/control characters;
- invalid Unicode/host normalization;
- redirects from an authenticated administrative route to an untrusted origin.

## 4xx catch-all / fall-through

A server that chooses the ORES fall-through stage should emit `application/problem+json` and preserve correlation data.

Canonical status mapping:

- `400` malformed syntax/encoding;
- `401` unauthenticated when identity is required;
- `403` authenticated but forbidden / IP policy denied;
- `404` no route/operation matched;
- `405` route exists but method is unsupported;
- `406` response representation cannot satisfy `Accept`;
- `408` request timeout when distinguishable from an internal deadline;
- `409` idempotency/conflict semantics where applicable;
- `413` bounded-body limit exceeded;
- `415` unsupported `Content-Type` or `Content-Encoding`;
- `422` decoded payload failed the selected request contract;
- `429` admission/rate limit denied.

If a service wants telemetry/security finalization to observe fall-through responses, it should place those response-capable stages around the router/fall-through boundary explicitly.

## Configuration ownership

Environment-variable names and non-secret metadata must be declared, never discovered ad hoc in application code.

| Concern | Canonical repo-local config |
| --- | --- |
| middleware target/composition + middleware-owned env metadata | `.ores-mw.toml` |
| rate-limit policy | `.ores-rl.toml` |
| Redis/LRU policy | `.ores-lru.toml` |
| Shared Auth integration | `.auth-shared.toml` |
| telemetry/logging | `.ores-otel.toml` |
| CLI/env argument mapping | `.cli-flags.toml` via `flags-2-env` |
| cross-repository package provenance | `.zpkg.toml` / lock |

Composition fields in `.ores-mw.toml` should describe the consumer's actual target/router composition: selected stage names, exact order, provider references, and optional required/forbidden/unique/before/after validation rules. Omitted composition must not silently expand to the legacy reference profile unless the consumer explicitly opts into that profile.

Secrets are references/keys only in those files. Secret values belong in the approved secret-delivery boundary (for example sops+age decrypted at runtime, platform secret stores, or workload identity).

A deployable consumer should not introduce undocumented `process.env.*`, `std::env::var`, `os.Getenv`, or equivalent middleware configuration reads. Fleet audits can fail when a middleware-affecting environment key has no declaration in the appropriate `.ores-*.toml` / `.cli-flags.toml` contract.

## Consumer acceptance gate

A repository counts as a hardened live adoption only when all applicable items are true on its default branch:

1. it imports/installs ORES middleware primitives/adapters in live server composition code;
2. its native package manifest uses an immutable reviewed middleware revision or released artifact;
3. `.zpkg.toml` declares the ORES package relationship used by the repo;
4. `.ores-mw.toml`, when used for composition, describes the live server target and the consumer-selected stage plan/policy;
5. applicable `.ores-rl.toml`, `.ores-lru.toml`, `.auth-shared.toml`, and `.ores-otel.toml` are present and validated;
6. concrete provider libraries/versions are owned by the consumer and injected behind stable ports;
7. no plaintext secrets are committed;
8. tests cover the middleware actually selected by the service, including its declared ordering invariants and relevant failure paths;
9. temporary rollout workflows/flags are removed;
10. the default branch CI passes after adoption.

A template, planning document, or use of the legacy default profile alone does not prove that a service's intended consumer-owned composition is correct.
