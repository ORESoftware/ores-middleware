# Persistence constraint parity

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
authorities. The existing polyglot generator compares fields, requiredness,
logical and SQL types, enum values, table identity, primary keys, uniqueness,
and generated artifacts. It must not silently erase validation semantics before
that comparison.

`tools/contract-parity/src/bin/persistence_constraint_gate.rs` is a separate Rust
admission gate for semantics that were previously normalized away:

- `additionalProperties` and `unevaluatedProperties` object closure;
- string `pattern`;
- string `minLength`;
- string `maxLength`;
- unsupported JSON Schema validation keywords or TypeSpec field decorators.

The gate does not translate one authority into the other. It parses both sources
independently, produces typed snapshots, compares them, writes a deterministic
receipt, and returns exit code 2 with `STOPPED_FOR_EVALUATION` when the sources
differ or use semantics the checker does not yet model.

## Pre-hardening evidence

Before this gate, both the Python `schema_convergence.py` compatibility model and
the Rust polyglot generator reduced string fields to the same generic `string`
shape. Adding a JSON Schema pattern and length bounds changed validation behavior
but left the canonical comparison model unchanged. Reopening the JSON object with
`additionalProperties: true` was likewise outside that canonical model.

That behavior was a false-negative parity path. The new regression suite proves:

1. current closed peer authorities pass;
2. a one-sided JSON pattern stops evaluation;
3. a one-sided TypeSpec length bound stops evaluation;
4. aligned pattern and length constraints pass;
5. reopened object policy stops evaluation;
6. missing closure declarations fail hard;
7. unsupported semantic keywords and decorators fail closed;
8. impossible length ranges fail hard.

## Adding a new constraint

Add the constraint independently to both authored sources. For TypeSpec string
fields use the standard `@pattern`, `@minLength`, and `@maxLength` decorators.
For JSON Schema use their Draft 2020-12 keyword peers. Extend this Rust gate and
its negative tests before introducing another validation keyword; do not rely on
the generator to ignore it.

Run:

```bash
npm run contracts:constraint-parity
cargo test --manifest-path tools/contract-parity/Cargo.toml --all-targets
```

The complete `npm run contracts:check` and audit paths invoke the gate as a
required check. Python remains only where current ORM orchestration still imports
its data structures; parity admission for this constraint boundary is Rust-owned.
