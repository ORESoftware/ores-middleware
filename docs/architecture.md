# RPC/API-doc middleware boundary

## Decision

`ORESoftware/api-docs` remains the producer and validator of route-map RPC
contracts and their digest-bound documentation projections. `ores-middleware`
owns a small, framework-neutral policy for deciding whether a host request
should pass through or receive one of those already-built artifacts.

This separation prevents a middleware package from becoming another route-map,
RPC-frame, OpenAPI, or schema authority. It also lets Axum, Tower, Leptos,
Dioxus, Express, Hono, Bun, Deno, Nest, Next, Nuxt, Hapi, `net/http`, Gorilla,
Gin, Echo, Fiber, Gleam/OTP, Plug, and Cowboy adapters share behavior without
sharing router APIs.

```text
host framework request
        |
        v
framework adapter: extract method/path/Accept/X-Ores-Docs-Format
        |
        v
ores-middleware docs selector  -- no listener, router, body, TLS, or RPC decode
        |
        +-- pass ------------------------------> host router
        |
        +-- serve(artifact key) ---------------> injected api-docs provider
        |
        +-- 405 / 406 / 503 -------------------> hardened host response
```

## Contract ownership

| Concern | Owner |
| --- | --- |
| TypeSpec RPC/service authority | Human-authored TypeSpec lane in the contract home |
| JSON Schema/OpenAPI authority | Human-authored JSON Schema/OpenAPI lane in the contract home |
| Route-map inventory and RPC digests | `ORESoftware/api-docs` |
| OpenAPI/OpenRPC/Connect/Hyper-Schema/catalog/HTML bytes | `ORESoftware/api-docs` |
| Path/header docs selection | `ORESoftware/ores-middleware` |
| Actual route registration | Host framework adapter/application |
| Authentication/authorization | A surrounding `shared-auth` middleware/policy |
| TLS termination | Edge proxy, ingress, or host server—not this selector |
| Compression | Host response middleware after safe representation selection |
| SQL/catalog and ORM convergence | Each database-backed `*-lib-core` plus declarative migrations |

The selector never receives credential values and must not reflect arbitrary
request headers. A host can place shared-auth before the selector when docs are
private, or after it when the selected documents are intentionally public.

## Peer authorities and discrepancy gates

The following hierarchy is prohibited:

```text
TypeSpec -> JSON Schema/OpenAPI -> downstream artifacts
```

The required independent flows are:

```text
TypeSpec
  -> normalized contract/persistence IR_T
  -> SQL_T where applicable
  -> Protobuf/proto3
  -> gRPC
  -> wire clients

JSON Schema/OpenAPI
  -> normalized contract/persistence IR_J
  -> interfaces/types/runtime validators
  -> SQL_J where applicable
  -> HTTP/write clients
```

For this repository, `contracts/docs-serving.tsp` and
`contracts/docs-serving.schema.json` are independently authored and directly
compared for enum values, property names, requiredness, and normalized types.
The same behavior fixture is then run by Rust, TypeScript/JavaScript, and Go.
Future Gleam, Elixir, and Erlang cores must pass the identical corpus before
being called compatible.

The docs selector has no persistent entities, migrations, tables, columns, or
ORM models. Therefore SQL/catalog and Diesel/SeaORM execution is genuinely not
applicable here. The audit receipt says so explicitly; it does not report those
checks as executed. Database-backed RPC contracts remain blocked upstream until:

1. SQL_T and SQL_J are materialized independently in disposable PostgreSQL
   databases and normalize to one catalog;
2. generated type/client surfaces pass common positive, negative, boundary, and
   compatibility fixtures;
3. Diesel and SeaORM are generated or introspected independently, agree with
   each other, and agree with the admitted catalog; and
4. every unexplained difference is resolved by a human-reviewed change or an
   exact, owned, tested, expiring exception.

Those gates are tracked in Linear `DEN-3959`, `DEN-3982`, and `DEN-3321` and are
not weakened by this middleware extraction.

## Header and path behavior

The selector matches exact paths after removing the query component. It does
not percent-decode, normalize trailing slashes, or reinterpret framework route
parameters.

The generic HTML aliases can negotiate another representation. Selection order
is:

1. a valid `X-Ores-Docs-Format` value;
2. the highest-quality recognized `Accept` media range;
3. HTML when the headers are absent or accept `*/*`.

Artifact-specific paths are fixed. `X-Ores-Docs-Format` and `Accept`, when
present, must permit that fixed representation. Conflicts return `406`.

Supported media types:

| Representation | Preferred media type | Compatible broad type |
| --- | --- | --- |
| HTML | `text/html` | `*/*` |
| Catalog | `application/vnd.ores.api-docs+json` | `application/json`, `application/*`, `*/*` |
| OpenAPI | `application/vnd.oai.openapi+json` | `application/openapi+json`, `application/json`, `application/*`, `*/*` |
| OpenRPC | `application/openrpc+json` | `application/json`, `application/*`, `*/*` |
| Connect | `application/vnd.ores.connect+json` | `application/json`, `application/*`, `*/*` |
| Hyper-Schema | `application/schema+json` | `application/json`, `application/*`, `*/*` |

Quality `q=0` excludes a range. Other valid quality values are sorted descending
with source order as the tie-breaker. Invalid quality parameters do not become
silent acceptance.

## Digest admission

`api-docs` artifacts carry one normalized RPC contract SHA-256. A server may
also compile or configure the digest of the RPC mechanism it actually runs.

When `runtimeContractDigest` is provided, `docsContractDigest` must be present,
must be 64 lowercase hexadecimal characters, and must match exactly. Invalid,
missing, or different evidence returns:

```text
action = stopped-for-evaluation
status = 503
```

No document is selected. This is a release/runtime admission failure, not a
content-negotiation fallback. When a valid docs digest is available, the
selector adds `X-Ores-Contract-SHA256` to the response decision.

## Provider interface

Language/framework adapters should expose an injected provider equivalent to:

```text
load(representation, verified_contract_digest) -> body bytes or stream
```

The provider may wrap `ores-api-docs::Catalog`, a generated static bundle, or a
remote immutable artifact store. It must not regenerate or mutate authored
TypeSpec/JSON Schema inputs during a request. Provider failures are host errors;
they do not cause the selector to silently choose another representation.
