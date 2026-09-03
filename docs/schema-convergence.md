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

## SQL and ORM admission algorithm

1. Parse each authored source independently.
2. Emit lane-specific artifacts into separate target directories.
3. Compare normalized types and requiredness.
4. Compare normalized SQL definitions.
5. Compare Diesel and SeaORM persistence semantics.
6. Compile the generated Rust witnesses.
7. Emit receipts with source digests, artifact digests, and tool versions.
8. Admit only when there are zero unexplained findings.

A mismatch produces an immutable fingerprint and the state
`STOPPED_FOR_EVALUATION`. Generation, publication, downstream bumps, and server
adoption must not proceed until a human-reviewed resolution changes an authored
source or records a narrowly scoped, owned, tested, expiring exception. A
translation witness never decides which authored authority is correct.

## Current scope and next gate

The checked-in projection witness proves the mechanism with the middleware
idempotency record. It catches property, type, nullability, enum, table,
primary-key, unique-constraint, generated SQL, generated client type, and
ORM-shape drift. The bidirectional shadow gate additionally catches lossy
TypeSpec/JSON Schema conversion and both round-trip directions.

Issue #5 remains open for the database-backed admission stage: compile actual
Diesel and SeaORM models, apply SQL_T and SQL_J independently to disposable
PostgreSQL databases, normalize `pg_catalog`/`information_schema` read-back, and
compare the TypeSpec, JSON Schema/OpenAPI, Diesel, and SeaORM witnesses. Linear
DEN-3321, DEN-3959, DEN-3982, and DEN-4078 are the durable control-plane records.
