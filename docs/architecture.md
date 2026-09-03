# RPC/API-doc middleware and peer-authority architecture

## Semantic conflict resolution

The standard middleware SDK remains one coherent polyglot product. The
routing-neutral docs selector is integrated into the primary language packages
rather than replacing them:

- Rust exports `ores_middleware::docs_serving` beside the Axum/MASH/Leptos/
  Dioxus middleware stack.
- TypeScript exports `@oresoftware/ores-middleware/docs-serving` beside the
  Fetch/Node/Deno/Bun/framework adapters.
- Go exports `docsserving` inside the existing module.
- Gleam, Elixir, and Erlang retain their complete request-lifecycle SDKs; thin
  docs-serving adapters for those runtimes remain admission-gated follow-up
  work in issue #3.

This is the semantic resolution of PR #2 against the newer all-language SDK. No
language implementation is demoted to a placeholder and no duplicate nested
package owns the same decision surface.

## RPC/API-document boundary

`ORESoftware/api-docs` remains the producer and validator of route-map RPC
contracts and their digest-bound documentation projections. `ores-middleware`
owns only a framework-neutral policy for deciding whether a host request should
pass through or receive one of those already-built artifacts.

```text
host framework request
        |
        v
thin adapter: method/path/Accept/X-Ores-Docs-Format
        |
        v
ores-middleware docs selector
        |
        +-- pass ------------------------------> host router
        +-- serve(artifact key) ---------------> injected api-docs provider
        +-- 405 / 406 / 503 -------------------> hardened host response
```

The selector owns no listener, router, request body, TLS termination, RPC frame,
or document-generation logic. It receives no credentials and must never reflect
arbitrary request headers. `shared-auth` may run before it for private documents
or after it for intentionally public documents. Compression and TLS remain at
the host or trusted-edge boundary.

| Concern | Owner |
| --- | --- |
| TypeSpec RPC/service authority | Human-authored TypeSpec lane |
| JSON Schema/OpenAPI authority | Human-authored JSON Schema/OpenAPI lane |
| Route-map inventory and RPC digests | `ORESoftware/api-docs` |
| OpenAPI/OpenRPC/Connect/Hyper-Schema/catalog/HTML bytes | `ORESoftware/api-docs` |
| Path/header docs selection | `ORESoftware/ores-middleware` |
| Actual route registration | Host framework adapter/application |
| Authentication/authorization | Surrounding `shared-auth` policy |
| Observability and propagation | ORES middleware ports plus `ores-otel` |
| Sync completion observation | ORES middleware port plus `opto-sync` |
| TLS termination | Trusted edge, ingress, or host server |
| SQL/catalog and ORM convergence | Independent authority lanes plus issue #5 |

## Independent top-level contract authorities

The following topology is prohibited:

```text
TypeSpec -> JSON Schema/OpenAPI -> downstream artifacts
```

The required peer flows are:

```text
TypeSpec
  -> SQL_T where applicable
  -> Protobuf
  -> gRPC
  -> wire clients

JSON Schema/OpenAPI
  -> interfaces/types/runtime validators
  -> SQL_J where applicable
  -> write clients
```

Both inputs are human-authored, top-level authorities. A translation from one
into the other may exist only as comparison evidence; it cannot establish
precedence, mutate the peer source, or silently repair a mismatch.

`contracts/docs-serving.tsp` and `contracts/docs-serving.schema.json` are
compared directly for enums, properties, requiredness, and normalized types.
Rust, TypeScript/JavaScript, and Go execute the same behavior corpus. Gleam,
Elixir, and Erlang docs adapters must run that identical corpus before claiming
docs-serving compatibility.

## SQL, generated types, Diesel, and SeaORM

The selector itself is stateless, but the wider middleware stack includes
idempotency, which requires persistent state. This repository therefore carries
a second independent authority pair for a representative idempotency record:

- `contracts/persistence/idempotency-record.tsp`
- `contracts/persistence/idempotency-record.schema.json`

`scripts/schema_convergence.py` projects both lanes independently and compares:

1. field names, logical types, requiredness, enum values, table names, primary
   keys, and unique constraints;
2. SQL_T and SQL_J;
3. independently generated TypeScript client types;
4. a TypeSpec/Diesel-shaped Rust witness and a JSON-Schema/SeaORM-shaped Rust
   witness; and
5. the normalized persistence semantics of both ORM witnesses.

Both Rust witnesses are compiled when `rustc` is available. Any difference
writes a deterministic discrepancy fingerprint and exits with code 2 and
`STOPPED_FOR_EVALUATION`. The checker never selects a winning authority or ORM.

These lightweight witnesses prove the fail-closed projection mechanism; they do
not pretend to be complete Diesel or SeaORM integrations. Before generated
persistence artifacts are released, issue #5 requires:

1. actual Diesel and SeaORM model/migration compilation;
2. independent application of SQL_T and SQL_J to disposable PostgreSQL
   databases;
3. `pg_catalog`/`information_schema` read-back into one normalized catalog;
4. four-way comparison among both database catalogs and both ORMs; and
5. an immutable receipt with source digests and exact tool versions.

Generation, publication, dependency bumps, and server adoption stop on every
unexplained mismatch until a human-reviewed source change or a narrow, owned,
tested, expiring exception resolves it.

## Header, path, and representation behavior

The selector matches exact paths after removing the query component. It does
not percent-decode, normalize trailing slashes, or reinterpret framework route
parameters.

Generic HTML aliases negotiate in this order:

1. valid `X-Ores-Docs-Format`;
2. highest-quality recognized `Accept` media range;
3. HTML when headers are absent or accept `*/*`.

Artifact-specific paths remain fixed. Header conflicts return `406`; unsupported
methods return `405` with `Allow: GET, HEAD`.

| Representation | Preferred media type | Compatible broad type |
| --- | --- | --- |
| HTML | `text/html` | `*/*` |
| Catalog | `application/vnd.ores.api-docs+json` | `application/json`, `application/*`, `*/*` |
| OpenAPI | `application/vnd.oai.openapi+json` | `application/openapi+json`, `application/json`, `application/*`, `*/*` |
| OpenRPC | `application/openrpc+json` | `application/json`, `application/*`, `*/*` |
| Connect | `application/vnd.ores.connect+json` | `application/json`, `application/*`, `*/*` |
| Hyper-Schema | `application/schema+json` | `application/json`, `application/*`, `*/*` |

Quality `q=0` excludes a range. Other valid values are sorted descending with
source order as the tie-breaker. Invalid quality parameters do not become silent
acceptance.

## Digest admission and provider interface

`api-docs` artifacts carry one normalized RPC-contract SHA-256. When
`runtimeContractDigest` is supplied, `docsContractDigest` must also be a
64-character lowercase SHA-256 and must match exactly. Missing, malformed, or
mismatched evidence returns:

```text
action = stopped-for-evaluation
status = 503
```

There is no representation fallback. A valid digest is returned as
`X-Ores-Contract-SHA256`.

Adapters inject a provider equivalent to:

```text
load(representation, verified_contract_digest) -> body bytes or stream
```

The provider may wrap an `ores-api-docs` catalog, a generated static bundle, or
an immutable artifact store. It must not regenerate or mutate either authored
authority during a request. Provider failure is a host error, never permission
to choose another representation.

## Polyglot packaging and receipts

`.zpkg.toml` intentionally omits a package-level language and publishes one
whole-repository artifact plus six named language slices. The build orchestrator
writes only disposable evidence beneath:

```text
target/rust
target/ts
target/golang
target/gleam
target/elixir
target/erlang
```

Generated output is never an authority. CI emits
`ores.schema-audit-receipt/v1` with the exact commit, source digests, tool
versions, executed/failed checks, applicability decisions, and discrepancy
fingerprints.
