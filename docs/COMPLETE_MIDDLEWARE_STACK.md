# Complete ORES middleware stack

This document is the reviewed composition contract for deployable HTTP/RPC servers that use `ORESoftware/ores-middleware`.

`ores-middleware` is the lifecycle/orchestration layer. It must not silently reimplement the authoritative packages it composes:

- shared value/request contracts: `oresoftware/ores-interfaces`
- admission/rate limiting: `ores-rate-limit/ores-rl-lib-core`
- bounded local/Redis LRU cache profiles: `ores-redis-lru-cache/ores-lru-redis-lib-core`
- authentication/session verification: `shared-auth/shared-auth-lib-core`
- telemetry/log context: `ores-otel/ores.otel.log`

Consumers may use native package-manager projections, but `.zpkg.toml` is the cross-repository dependency declaration and provenance boundary.

## Request data model

Every adapter must project the incoming request into a data-only request envelope. The semantic contract belongs in `ores-interfaces`; language runtimes use idiomatic maps:

- `headers`: case-insensitive-on-input, canonical lowercase string map. Authored/generated names should be lowercase, including the reserved `x-ores-*` extension namespace.
- `query`: string or multi-string map.
- `json_payload`: typed `string -> JSON value` map when the decoded root is an object.
- `decoded_payload`: representation-tagged decoded value for JSON, XML, MessagePack, or Protobuf.
- `raw_body`: bounded bytes retained only when a downstream contract explicitly needs them.
- `attributes`: middleware-owned typed metadata such as request ID, trace ID, authenticated subject, tenant, cache decision, and rate-limit decision.

The request envelope is per-request data. It is never a process-global mutable map and must not contain plaintext secrets in telemetry.

## Reviewed lifecycle order

Request stages execute in this order. Response stages execute on unwind in the documented response order.

1. **panic/error boundary** — convert uncaught failures to sanitized `5xx` problem responses.
2. **deadline/cancellation** — establish request deadline and cancellation propagation.
3. **request ID and trace context** — accept only valid inbound values; otherwise generate fresh values.
4. **trusted-proxy/TLS termination policy** — derive effective HTTPS only from a directly secure connection or forwarded metadata from an explicitly trusted proxy.
5. **request decompression** — decode supported `Content-Encoding` values before payload parsing, while enforcing both compressed and expanded byte limits to avoid decompression bombs.
6. **payload byte limit** — bound the post-decompression payload before parser allocation.
7. **anonymous flood guard** — coarse abuse control before expensive authentication.
8. **content-type dispatch / deserialization** — JSON, XML, MessagePack, or Protobuf. Unsupported media type is `415`; malformed payload is `400`; contract/schema mismatch is `422` unless an application-specific contract intentionally chooses another 4xx.
9. **authentication** — Shared Auth is authoritative. Authentication must fail closed whenever the configured route requires identity.
10. **principal rate limit** — use the authenticated principal when available. Security-sensitive operations use strict/fail-closed policy; public reads may use bounded/local degradation according to `.ores-rl.toml`.
11. **authorization** — route/application policy after authentication and principal admission.
12. **cache lookup / conditional request** — bounded local cache may front Redis according to `.ores-lru.toml`; authorization-sensitive data must include tenant/subject scope in the cache key or bypass shared caching.
13. **idempotency admission** — for configured mutation methods.
14. **redirect policy** — only normalized, explicitly allowed destinations; reject protocol-relative, credential-bearing, control-character, or untrusted-host targets. Permanent redirects are application policy, not an automatic middleware rewrite.
15. **application handler**.
16. **catch-all / fall-through 4xx** — an otherwise unhandled route becomes a structured `404` problem response; a method mismatch becomes `405` with `Allow` where the framework can determine it. Fall-through must never become a generic `500`.
17. **cache write / ETag finalization** — only cache responses allowed by status, auth scope, `Cache-Control`, and configured cache policy.
18. **response compression** — negotiate an enabled algorithm only when worthwhile and never double-compress an already encoded response.
19. **encryption/wrapping** — application/message encryption is explicit policy. Transport TLS is not replaced by payload encryption. Decryption happens before representation parsing; encryption happens after serialization and before transport framing.
20. **security headers and correlation headers**.
21. **telemetry/finalization/cleanup** — emit bounded metadata, remove request context, release leases/resources.

## Content representations

The portable middleware contract recognizes these canonical media types:

| Representation | Canonical media type | Notes |
| --- | --- | --- |
| JSON | `application/json` | `application/problem+json` is the canonical structured error representation. |
| XML | `application/xml` | `text/xml` may be accepted as a compatibility alias but should normalize to `application/xml`. Disable DTD/external-entity resolution. |
| MessagePack | `application/msgpack` | `application/x-msgpack` may be accepted as a compatibility alias. |
| Protobuf | `application/protobuf` | `application/x-protobuf` may be accepted as a compatibility alias. Protobuf decoding requires a route/message descriptor; there is no safe schema-free object decoder. |

All parsers receive bounded bytes and must reject trailing garbage where the codec exposes that distinction.

## Encryption and compression

Compression and encryption are transforms around the representation codec, not alternate representations.

Inbound order is:

`transport -> trusted TLS/proxy admission -> decrypt (when explicitly configured) -> decompress -> representation decode -> contract validation`

Outbound order is:

`application value -> representation encode -> optional compress -> optional encrypt -> transport`

Authenticated encryption must be used for payload encryption. Nonce/key material is injected by an approved secret provider and is never committed to `.ores-*.toml`, `.zpkg.toml`, logs, or generated receipts.

## Rate limiting

`ores-middleware` owns *where* rate limiting runs; `ores-rate-limit` owns admission semantics. A consumer should not instantiate a second unrelated token bucket when an ORES rate-limit adapter is configured.

- anonymous flood limiting occurs before authentication;
- principal/tenant limiting occurs after authentication;
- authorization, account recovery, payments/ledger writes, and mutation admission fail closed if the authoritative coordinator is required and unavailable;
- local fallback is allowed only when `.ores-rl.toml` declares a bounded/local policy for that operation class.

## LRU / Redis cache

`ores-redis-lru-cache` owns cache profiles and Redis-backed bounded-LRU semantics. Middleware integrates it through a cache interface and must preserve:

- bounded entries and TTLs for local memory;
- namespace/versioned keys;
- tenant/user/authorization scope where data is not public;
- negative-cache policy as an explicit choice;
- stampede/single-flight behavior where provided by the cache package;
- no caching of `Set-Cookie`, secrets, or private responses unless an explicit route policy says otherwise.

A cache outage must not bypass authentication or authorization.

## Shared Auth

`shared-auth` owns session/token verification and identity semantics. `ores-middleware` may host an embedded verifier or call an HTTP verifier, but it must not invent a parallel user/session model.

- normalized identity fields are attached to the request context/envelope;
- auth failures are structured `401`/`403` responses;
- configured Shared Auth integration is fail-closed;
- authorization decisions happen after identity establishment;
- test bypasses are forbidden in production and must remain explicit in test/staging configuration.

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

Every server adapter must install a terminal 4xx fallback. The fallback emits `application/problem+json` and includes the request correlation header.

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

The fallback is *after* application routing but still inside correlation/security/telemetry finalization.

## Configuration ownership

Environment-variable names and non-secret metadata must be declared, never discovered ad hoc in application code.

| Concern | Canonical repo-local config |
| --- | --- |
| middleware target/orchestration + middleware-owned env metadata | `.ores-mw.toml` |
| rate-limit policy | `.ores-rl.toml` |
| Redis/LRU policy | `.ores-lru.toml` |
| Shared Auth integration | `.auth-shared.toml` |
| telemetry/logging | `.ores-otel.toml` |
| CLI/env argument mapping | `.cli-flags.toml` via `flags-2-env` |
| cross-repository package provenance | `.zpkg.toml` / lock |

Secrets are references/keys only in those files. Secret values belong in the approved secret-delivery boundary (for example sops+age decrypted at runtime, platform secret stores, or workload identity).

A deployable consumer must not introduce undocumented `process.env.*`, `std::env::var`, `os.Getenv`, or equivalent middleware configuration reads. Fleet audits should fail when a middleware-affecting environment key has no declaration in the appropriate `.ores-*.toml` / `.cli-flags.toml` contract.

## Consumer acceptance gate

A repository counts as a hardened live adoption only when all of the following are true on its default branch:

1. it imports/installs the shared middleware adapter in live server composition code;
2. its native package manifest uses an immutable reviewed middleware revision or released artifact;
3. `.zpkg.toml` declares the ORES package relationship used by the repo;
4. `.ores-mw.toml` selects the live server target and stack config;
5. applicable `.ores-rl.toml`, `.ores-lru.toml`, `.auth-shared.toml`, and `.ores-otel.toml` are present and validated;
6. no plaintext secrets are committed;
7. 4xx fall-through, malformed-body, unsupported-media, auth, rate-limit, cache-outage, TLS-forwarding, compression/decompression, redirect, and codec tests pass;
8. temporary rollout workflows/flags are removed;
9. the default branch CI passes after adoption.

A template or planning document alone does not count as adoption.
