# Rust contract parity checker

`ores-contract-parity` compares the independently authored TypeSpec and JSON Schema/OpenAPI contract authorities used by `ores-middleware`. Neither authority is generated from, or subordinate to, the other.

Run the checker from the repository root:

```sh
cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml --
```

Write a report to an explicit location or evaluate another checkout:

```sh
cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml -- \
  --root /path/to/ores-middleware \
  --report target/audit/docs-serving-contract-parity.json
```

Exit codes are part of the gate contract:

- `0`: the compared peer authorities agree;
- `2`: one or more unexplained discrepancies require `STOPPED_FOR_EVALUATION`;
- `1` or `64`: the checker could not execute or its CLI input was invalid.

The Rust tests include positive parity plus negative requiredness, regex-pattern, and authority-topology drift cases. Add a regression before extending normalization semantics. Unsupported syntax must fail closed rather than being silently erased during comparison.

`scripts/check_contract_parity.py` is only a temporary compatibility bridge for the wider Python audit orchestrator. It delegates validation and reporting to this Rust executable and must not acquire independent parity logic.
