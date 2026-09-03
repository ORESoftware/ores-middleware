# Schema, SQL, and ORM convergence gate

## Invariant

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
contract authorities. Diesel and SeaORM are independent ORM witnesses. No lane
is a fallback for another, and no generated translation may overwrite an
authored source.

| Lane | Authored source | Generated evidence |
| --- | --- | --- |
| T | TypeSpec | normalized IR_T, SQL_T, Protobuf/gRPC/wire-client work products, Diesel witness |
| J | JSON Schema/OpenAPI | normalized IR_J, interfaces/types/write-client work products, SQL_J, SeaORM witness |

## Direct and translated comparisons

Direct peer comparison and cross-translation are separate required gates:

1. Compare the independently authored TypeSpec and JSON Schema/OpenAPI models.
2. Generate a **non-authoritative** JSON Schema shadow from TypeSpec and compare
   it with both the TypeSpec source semantics and the independently authored
   JSON Schema/OpenAPI authority.
3. Generate a **non-authoritative** TypeSpec shadow from JSON Schema/OpenAPI and
   compare it with both the JSON Schema source semantics and the independently
   authored TypeSpec authority.
4. Run `TypeSpec -> JSON Schema -> TypeSpec` and
   `JSON Schema -> TypeSpec -> JSON Schema` round trips.
5. Compile generated TypeSpec shadows when the TypeSpec compiler is available,
   validate generated JSON Schema shadows against Draft 2020-12, and retain
   digest-addressed receipts and artifacts.

The translators deliberately support a bounded, explicit contract subset.
Encountering an unsupported construct is a discrepancy, not permission to drop
or approximate it. Generated shadows carry `authoritative: false` metadata and
exist only under `target/cross-translation/`.

Run the gate directly with:

```bash
npm run contracts:cross-translate
python3 -m unittest scripts/test_cross_translation.py -v
```

It is also mandatory in `npm run contracts:check` and in the whole-repository
zed-pkg publication smoke test. The polyglot package remains one canonical
repository artifact plus independently consumable Rust, TypeScript, Go, Gleam,
Elixir, and Erlang slices.

## Real SQL and ORM admission gate

`scripts/orm_catalog_gate.py` contains the four-witness implementation, while
`scripts/orm_catalog_gate_entrypoint.py` is the supported executable entrypoint.
The entrypoint separates machine-readable witness JSON on stdout from Cargo and
compiler diagnostics on stderr, while preserving both channels when a command
fails.

The admission stage:

1. Parses the TypeSpec and JSON Schema/OpenAPI authorities independently.
2. Re-runs normalized model, SQL, client-type, and ORM-shape projection parity.
3. Generates an isolated Rust crate containing a real Diesel table/model from
   the TypeSpec lane and a real SeaORM entity from the JSON Schema/OpenAPI lane.
4. Generates and retains the crate lockfile, then compiles both ORM implementations
   against exact top-level Diesel and SeaORM versions.
5. Executes the compiled witness to emit authority-tagged normalized manifests.
6. Applies SQL_T and SQL_J to separate `typespec_lane` and `json_schema_lane`
   schemas in a disposable PostgreSQL service.
7. Reads columns from `information_schema`, constraints from `pg_constraint`,
   and indexes from `pg_indexes`.
8. Compares each catalog with its own authority, compares SQL_T with SQL_J by
   normalized catalog read-back, and compares the compiled Diesel/SeaORM contract
   manifests.
9. Retains source, SQL, ORM source, Cargo lockfile, catalog, tool-version, and
   SHA-256 evidence in `ores.orm-catalog-convergence-report/v1`.

Run it with a disposable PostgreSQL database:

```bash
python3 -m unittest scripts/test_orm_catalog_gate.py scripts/test_subprocess_capture.py -v
DATABASE_URL=postgresql://... python3 scripts/orm_catalog_gate_entrypoint.py
# or: DATABASE_URL=postgresql://... npm run persistence:check
```

The dedicated `persistence-convergence` workflow runs on pull requests, pushes,
manual dispatch, and a daily schedule. It uploads receipts even when the gate
fails. The command is exposed as `scripts.orm-catalog` in `.zpkg.toml`, but is
not part of the package-install smoke test because it intentionally requires a
real PostgreSQL control plane.

## Admission semantics

A mismatch produces an immutable fingerprint and the state
`STOPPED_FOR_EVALUATION`. Generation, publication, downstream bumps, migration,
and server adoption must not proceed until a human-reviewed resolution changes
an authored source or records a narrowly scoped, owned, tested, expiring
exception. A translation or ORM witness never decides which authored authority
is correct.

Passing proves convergence for the checked-in middleware idempotency contract
and the exact retained compiler, ORM, SQL, and PostgreSQL evidence. It does not
establish every future TypeSpec/JSON Schema construct, every database dialect,
or every product repository. Each newly admitted construct needs fixtures and
negative tests before the supported subset expands.

## Fleet follow-through

Issue #5 owns this repository-local database-backed gate. Fleet adoption remains
tracked separately: applicable `*-lib-core` repositories must copy or depend on
the versioned mechanism, retain independent source provenance, use disposable
databases, and stop publication/promotion on discrepancies. Linear DEN-3321,
DEN-3959, DEN-3982, and DEN-4078 remain the durable control-plane records.
