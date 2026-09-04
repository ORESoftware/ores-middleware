# Four-way ORM data-plane convergence

## Authority topology

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
authorities. Neither is generated from the other and neither is permitted to
repair, overwrite, or outrank the other.

Each authority independently produces the complete backend persistence surface:

```text
TypeSpec
  -> normalized IR_T
  -> SQL_T
  -> Diesel_T
  -> SeaORM_T

JSON Schema / OpenAPI
  -> normalized IR_J
  -> SQL_J
  -> Diesel_J
  -> SeaORM_J
```

The comparison matrix is therefore the full cross-product, not the historical
asymmetric pairing:

| Source authority | Diesel | SeaORM | PostgreSQL schema |
| --- | --- | --- | --- |
| TypeSpec | `typespec-diesel` | `typespec-seaorm` | `typespec_data_plane` |
| JSON Schema/OpenAPI | `json-schema-openapi-diesel` | `json-schema-openapi-seaorm` | `json_schema_data_plane` |

## What the row-level gate executes

`scripts/orm_data_plane_gate.py` parses both authored sources independently,
generates `SQL_T` and `SQL_J`, and applies them to isolated disposable
PostgreSQL schemas. It then generates and compiles a Rust witness crate with
real, database-enabled Diesel and SeaORM dependencies.

Every one of the four lanes executes the same bounded corpus:

- insert and read back a row with all optional values present;
- insert and read back a row with optional values absent;
- reject a duplicate primary key;
- reject a duplicate `(tenant_id, idempotency_key)` key;
- reject a status outside the independently authored enum/check vocabulary;
- reject a missing required value;
- reject an `int32` overflow; and
- reject an invalid timestamp.

Rows are normalized to the shared wire representation before comparison. The
gate requires identical positive rows and identical negative-case outcomes
across all four lanes. A compile failure, connection failure, accepted invalid
row, missing lane, or cross-lane difference produces a deterministic finding
and `STOPPED_FOR_EVALUATION`.

## Commands

A disposable PostgreSQL instance is required:

```bash
DATABASE_URL=postgresql://... npm run persistence:catalog
DATABASE_URL=postgresql://... npm run persistence:data-plane
DATABASE_URL=postgresql://... npm run persistence:check
```

`persistence:catalog` compiles both ORMs from both sources and compares SQL plus
PostgreSQL catalog read-back. `persistence:data-plane` executes the real row
paths. `persistence:check` requires both gates in that order.

## Evidence

The row-level gate emits:

```text
target/orm-data-plane-gate/
  receipt.json
  sql/typespec.sql
  sql/json-schema-openapi.sql
  witnesses/typespec-diesel.json
  witnesses/typespec-seaorm.json
  witnesses/json-schema-openapi-diesel.json
  witnesses/json-schema-openapi-seaorm.json
  rust-data-plane/Cargo.toml
  rust-data-plane/Cargo.lock
  rust-data-plane/src/main.rs
```

The receipt schema is `ores.orm-data-plane-convergence-report/v1`. It records
both source digests, the exact Diesel and SeaORM versions, compiler versions,
all four normalized witnesses, generated artifact hashes, discrepancy
fingerprints, and the final admission state.

GitHub Actions retains both catalog and row-level evidence for 90 days. These
gates use only disposable CI schemas; they do not apply migrations to a
production or shared database.

## Promotion rule

Generated persistence artifacts, migrations, packages, downstream dependency
bumps, and deployments remain blocked unless the exact same commit has:

1. direct TypeSpec/JSON Schema semantic parity;
2. independently generated SQL, interfaces, runtime code, Diesel, and SeaORM
   parity;
3. four-way ORM compilation and PostgreSQL catalog convergence; and
4. four-way row-level data-plane convergence.

No lane wins because it ran first, compiled faster, matched an older receipt,
or used a preferred ORM. Every unexplained difference requires explicit human
evaluation.
