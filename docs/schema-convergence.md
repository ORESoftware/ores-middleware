# Schema, SQL, and ORM convergence gate

## Invariant

TypeSpec and JSON Schema/OpenAPI are peers. Diesel and SeaORM are independent
ORM witnesses. No lane is a fallback for another.

| Lane | Authored source | Generated evidence |
| --- | --- | --- |
| T | TypeSpec | normalized IR_T, SQL_T, Protobuf/gRPC/wire-client work products, Diesel witness |
| J | JSON Schema/OpenAPI | normalized IR_J, interfaces/types/write-client work products, SQL_J, SeaORM witness |

## Admission algorithm

1. Parse each authored source independently.
2. Emit lane-specific artifacts into separate target directories.
3. Compare normalized types and requiredness.
4. Compare normalized SQL definitions.
5. Compare Diesel and SeaORM persistence semantics.
6. Compile the generated Rust witnesses.
7. Emit a receipt with source digests and tool versions.
8. Admit only when there are zero unexplained findings.

A mismatch produces an immutable fingerprint and the state
`STOPPED_FOR_EVALUATION`. Generation, publication, downstream bumps, and server
adoption must not proceed until a human-reviewed resolution changes the source
or records a narrowly scoped, owned, tested, expiring exception.

## Current scope and next gate

The checked-in witness proves the mechanism with the middleware idempotency
record. It catches property, type, nullability, enum, table, primary-key,
unique-constraint, generated SQL, generated client type, and ORM-shape drift.

Issue #5 owns the next admission stage: compile actual Diesel and SeaORM models,
apply both SQL lanes to disposable PostgreSQL instances, read back catalogs, and
compare all four witnesses. Linear DEN-3321, DEN-3959, and DEN-3982 remain the
control-plane records.
