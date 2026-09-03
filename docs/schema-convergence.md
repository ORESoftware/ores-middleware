# Independent TypeSpec and JSON Schema polyglot convergence

## Binding invariant

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
contract authorities. Neither source is generated from the other, neither wins
by precedence or fallback, and neither may rewrite the other.

```text
TypeSpec
  -> normalized IR_T
  -> SQL_T
  -> Rust / TypeScript / Go / Gleam / Elixir / Erlang code
  -> Diesel_T and SeaORM_T
  -> Protobuf / gRPC projection
  -> wire clients

JSON Schema / OpenAPI
  -> normalized IR_J
  -> SQL_J
  -> Rust / TypeScript / Go / Gleam / Elixir / Erlang code
  -> Diesel_J and SeaORM_J
  -> OpenAPI / HTTP write-client projection
```

Cross-translations remain non-authoritative witnesses. Passing a translated or
round-tripped document never changes source ownership.

## Rust polyglot generator

`tools/contract-parity/src/bin/persistence_codegen.rs` performs two independent
source reads and two independent generation passes. It writes only below
`target/schema-convergence/`:

```text
target/schema-convergence/
  receipt.json
  typespec/
    model.json
    sql/schema.sql
    typescript/idempotency_record.d.ts
    typescript/idempotency_record.mjs
    rust/idempotency_record.rs
    golang/idempotency_record.go
    gleam/idempotency_record.gleam
    elixir/idempotency_record.ex
    erlang/idempotency_record.erl
    diesel/idempotency_record.rs
    seaorm/idempotency_record.rs
    protobuf/idempotency_record.proto
    grpc/projection.json
  json-schema-openapi/
    model.json
    sql/schema.sql
    typescript/idempotency_record.d.ts
    typescript/idempotency_record.mjs
    rust/idempotency_record.rs
    golang/idempotency_record.go
    gleam/idempotency_record.gleam
    elixir/idempotency_record.ex
    erlang/idempotency_record.erl
    diesel/idempotency_record.rs
    seaorm/idempotency_record.rs
    openapi/idempotency_record.openapi.json
    http/write-client.json
```

The common SQL, type/interface, executable-code, Diesel, and SeaORM artifacts
must match byte for byte after each source is parsed independently. The report
also binds every source and generated artifact to SHA-256.

The persistence sources define data but no RPC/HTTP operations. The generator
therefore emits a protobuf message plus explicit message-only gRPC evidence and
an OpenAPI components document with an empty `paths` object. It never invents a
service operation merely to make a transport projection look complete.

Run the generator and behavioral checks with:

```bash
npm run contracts:polyglot-generate
npm run contracts:generated-check
```

`validate-generated-polyglot.mjs` verifies the report, source digests, complete
artifact inventory, cross-authority byte parity, positive and negative runtime
fixtures, and the no-invented-operation boundary.

## Native compilation matrix

The `generated-polyglot` GitHub Actions job regenerates both lanes from the
checked-out exact head and validates the emitted products with their native
toolchains:

- TypeScript declarations compile under strict NodeNext settings.
- Both JavaScript validators execute the same positive and negative fixtures.
- Both Rust libraries compile with `rustc` and unsafe code forbidden.
- Both Go packages pass `gofmt` and `go test`.
- The TypeSpec protobuf output compiles to a descriptor set with `protoc`.
- Both Gleam modules compile inside the repository's pinned Gleam project.
- Both Elixir modules compile independently.
- Both Erlang modules compile independently.

The compiled artifacts and the complete generator receipt are retained for 90
days. A green source-language SDK job cannot substitute for this generated-code
job.

## Four-way ORM and PostgreSQL gate

`scripts/orm_matrix_gate.py` is the database-backed admission path. It preserves
the existing exact Diesel and SeaORM pins, but replaces the old one-ORM-per-source
pairing with the complete cross-product:

| Source authority | Diesel | SeaORM | SQL/catalog |
| --- | --- | --- | --- |
| TypeSpec | `Diesel_T` | `SeaORM_T` | `SQL_T` / `typespec_lane` |
| JSON Schema/OpenAPI | `Diesel_J` | `SeaORM_J` | `SQL_J` / `json_schema_lane` |

The gate:

1. Requires the Rust polyglot generator receipt to be `passed` with zero
   unexplained findings.
2. Generates one isolated Rust witness crate containing all four real ORM
   implementations.
3. Creates and retains `Cargo.lock`, then compiles the exact Diesel and SeaORM
   versions with warnings denied.
4. Executes each compiled lane and compares it with its own source-derived
   expected manifest.
5. Compares Diesel across authorities and SeaORM across authorities.
6. Compares Diesel and SeaORM contract semantics within each authority.
7. Applies `SQL_T` and `SQL_J` to separate schemas in disposable PostgreSQL.
8. Reads back columns, nullability, constraints, checks, and indexes through
   PostgreSQL catalogs and compares both lanes.
9. Emits `ores.orm-catalog-convergence-report/v2` with exact source, compiler,
   ORM, SQL, catalog, and artifact evidence.

Run it with disposable PostgreSQL:

```bash
python3 -m unittest \
  scripts/test_orm_catalog_gate.py \
  scripts/test_orm_matrix_gate.py \
  scripts/test_subprocess_capture.py \
  -v

DATABASE_URL=postgresql://... python3 scripts/orm_matrix_gate.py
# or
DATABASE_URL=postgresql://... npm run persistence:check
```

The legacy `orm_catalog_gate.py` remains as a compatibility/helper module. It no
longer owns the package or CI admission command.

## Cross-translation evidence

Direct generation and cross-translation are separate gates:

1. Compare independently authored TypeSpec and JSON Schema/OpenAPI semantics.
2. Generate a non-authoritative JSON Schema shadow from TypeSpec.
3. Generate a non-authoritative TypeSpec shadow from JSON Schema/OpenAPI.
4. Run both round trips.
5. Compile or validate the generated shadows where the corresponding toolchain
   is available.
6. Retain digest-addressed evidence under `target/cross-translation/`.

Unsupported constructs are discrepancies. Translators may not silently drop,
approximate, or reinterpret them.

## Admission semantics

Any unexplained mismatch produces a stable fingerprint and enters
`STOPPED_FOR_EVALUATION`. That state blocks:

- generated artifact publication;
- zed-pkg publication;
- automatic merge;
- migration planning or application;
- downstream dependency bumps;
- server/client promotion; and
- deployment.

A human resolution must change an authored source or add a narrowly scoped,
owned, tested, approved, and expiring exception. No generator, ORM, translated
shadow, fastest CI job, or previously green commit may select the winner.

Passing proves the checked-in idempotency contract under the exact retained
evidence. Every newly supported scalar, union, discriminator, relation,
constraint, operation, streaming mode, or database dialect requires positive
and negative fixtures before admission.

## Fleet follow-through

Applicable `*-interfaces`, `*-lib-core`, and private `*-orm-core` repositories
must adopt this topology without copying generated artifacts back into an
authority source. Fleet enforcement remains tracked by Linear `DEN-3959`,
`DEN-3982`, `DEN-3321`, and `DEN-3828`.
