# RPC/API-doc middleware and peer-authority architecture

## Semantic merge decision

PR #1 remains the implementation baseline for the standard middleware SDK in
Rust, TypeScript/JavaScript, Go, Gleam, Elixir, and Erlang. PR #2 adds a
routing-neutral API-document selector without replacing those packages:

- Rust: `src/rust/docs-serving` is a nested crate beside the main SDK crate.
- TypeScript: `src/ts/docs-serving` is a nested package beside the main SDK.
- Go: `src/golang/docsserving` is a normal subpackage in the existing module.
- Gleam, Elixir, and Erlang keep their complete PR #1 SDK implementations;
  docs-serving adapters for those runtimes remain tracked rollout work.

This is a semantic merge, not a textual choice between branches.

## API-doc boundary

`ORESoftware/api-docs` remains the producer and validator of route-map RPC
contracts and digest-bound documentation projections. `ores-middleware` owns a
small policy function that decides whether a host request should pass through
or receive one of those already-built artifacts.

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
        +-- 405 / 406 / 503 -------------------> hardened response
```

The selector owns no listener, router, request body, TLS termination, RPC frame,
or document-generation logic. It receives no credentials and must never reflect
arbitrary headers. `shared-auth` may run before it for private documents or
after it for intentionally public documents. Compression and TLS stay at the
host/edge boundary.

## Independent top-level authorities

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

Both sources are human-authored, top-level authorities. A translation from one
into the other may be retained as comparison evidence, but it cannot establish
precedence or silently repair the peer source.

`contracts/docs-serving.tsp` and `contracts/docs-serving.schema.json` are
directly compared for enum values, properties, requiredness, and normalized
types. Rust, TypeScript, and Go then execute the same TSV behavior corpus. Thin
Gleam/OTP, Elixir/Plug, and Erlang/Cowboy docs adapters must run that identical
corpus before claiming docs-serving compatibility.

## Persistence projection and ORM witnesses

The docs selector itself is stateless. The wider middleware SDK is not: its
idempotency capability needs a durable persistence contract. Therefore this
repository also carries two independent authorities for one representative
idempotency record:

- `contracts/persistence/idempotency-record.tsp`
- `contracts/persistence/idempotency-record.schema.json`

`scripts/schema_convergence.py` projects both lanes independently and compares:

1. normalized field names, logical types, requiredness, enum values, primary
   keys, and unique constraints;
2. SQL_T and SQL_J;
3. generated TypeScript client types;
4. a compile-checked Diesel-shaped Rust witness from TypeSpec and a
   compile-checked SeaORM-shaped Rust witness from JSON Schema/OpenAPI.

Any difference writes a discrepancy report and exits with code 2,
`STOPPED_FOR_EVALUATION`. The checker never selects a winner.

These lightweight witnesses do not replace the live database admission gate.
Before generated persistence artifacts are released, issue #5 requires real
Diesel and SeaORM compilation, independent application of SQL_T and SQL_J to
disposable PostgreSQL databases, `pg_catalog`/`information_schema` read-back,
and four-way reconciliation among both catalogs and both ORMs.

## Header, path, and digest behavior

The selector matches exact paths after removing the query component. It does
not percent-decode, normalize trailing slashes, or reinterpret framework route
parameters. Generic HTML aliases negotiate in this order:

1. valid `X-Ores-Docs-Format`;
2. highest-quality recognized `Accept` media range;
3. HTML when headers are absent or accept `*/*`.

Artifact-specific paths remain fixed. Header conflicts return `406`; unsupported
methods return `405` with `Allow: GET, HEAD`.

When `runtimeContractDigest` is supplied, `docsContractDigest` must also be a
64-character lowercase SHA-256 and must match exactly. Missing, malformed, or
mismatched evidence returns a `503` decision with
`action = stopped-for-evaluation`. There is no representation fallback.

## Generated outputs and packaging

`.zpkg.toml` publishes one canonical whole-repository package plus six language
slices. The package-level `language` field is intentionally absent. Builds write
only disposable artifacts beneath:

```text
target/rust
target/ts
target/golang
target/gleam
target/elixir
target/erlang
```

Generated outputs are evidence, never authorities. CI records exact source
digests, tool versions, checks, applicability decisions, and discrepancy
fingerprints in `ores.schema-audit-receipt/v1`.
