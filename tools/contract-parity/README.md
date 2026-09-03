# Rust contract parity and polyglot generation

`ores-contract-parity` enforces the independent TypeSpec and JSON Schema/OpenAPI
authorities used by `ores-middleware`. Neither authority is generated from,
subordinate to, or allowed to overwrite the other.

## Docs-serving semantic checker

Run the primary checker from the repository root:

```sh
cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml --
```

Write a report to an explicit location or evaluate another checkout:

```sh
cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml -- \
  --root /path/to/ores-middleware \
  --report target/audit/docs-serving-contract-parity.json
```

## Persistence polyglot generator

The `persistence_codegen` binary independently reads the TypeSpec and JSON
Schema persistence sources. Each source generates its own SQL, TypeScript
interfaces and JavaScript validator, Rust, Go, Gleam, Elixir, Erlang, Diesel,
and SeaORM code. The TypeSpec lane additionally emits protobuf/gRPC evidence;
the JSON Schema/OpenAPI lane emits OpenAPI/HTTP write-client evidence.

```sh
cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml \
  --bin persistence_codegen -- \
  --output-root target/schema-convergence \
  --report target/schema-convergence/receipt.json

node scripts/validate-generated-polyglot.mjs
```

Every source and artifact is SHA-256 bound. Common generated products are
compared byte for byte. Outputs remain under `target/` and may never rewrite an
authored authority.

The persistence authority currently declares a data model but no service
operations. The generator therefore emits message/component projections with
explicit empty operation sets; it does not invent an API to make a transport
lane appear complete.

## Exit contract

- `0`: the compared peer authorities and generated products agree;
- `2`: one or more unexplained discrepancies require
  `STOPPED_FOR_EVALUATION`;
- `1` or `64`: the checker could not execute or its CLI input was invalid.

Rust tests include positive parity, complete artifact inventory, independent
source digests, both ORM projections per authority, no-invented-operation
checks, and negative requiredness, enum, SQL-table, regex-pattern, and authority
topology drift cases. Add a regression before extending normalization or
code-generation semantics. Unsupported syntax must fail closed rather than be
silently erased or approximated.

`scripts/check_contract_parity.py` remains only a compatibility bridge for the
wider Python audit orchestrator. Database-backed four-way ORM compilation is
owned by `scripts/orm_matrix_gate.py`.
