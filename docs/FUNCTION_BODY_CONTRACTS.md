# Executable function-body contracts

Middleware signatures are insufficient when the critical behavior lives inside a function body. `contracts/function-bodies/operation-boundary.plan.json` is executable pseudocode for one protocol operation boundary. It requires this semantic sequence:

```text
capture allowlisted context
normalize low-cardinality inputs
enter request/log context
enter a language failure boundary
arm cancellation/deadline handling
invoke user code only when still admissible
classify failure without copying payloads
construct a bounded public failure
report telemetry without letting the reporter replace the outcome
restore ambient context in all paths
return one typed outcome
```

## Peer authorities

`contracts/function-bodies/typespec/main.tsp` and the Draft 2020-12 schemas in `contracts/function-bodies/json-schema/` are independently authored top-level authorities. Neither is generated from or subordinate to the other. The audit compares their enums, models, properties, and supported language set. An unexplained mismatch is `STOPPED_FOR_EVALUATION`.

TypeSpec provides compile-time shape checking. JSON Schema performs strict runtime admission for the plan and six-language binding evidence. Semantic checks beyond either structural language enforce exact step order, dependency direction, one terminal outcome, reporter fail-open behavior, context restoration, and redacted public failures.

## Native evidence

`contracts/function-bodies/language-bindings.json` binds every pseudocode step to reviewed source witnesses in Rust, TypeScript, Go, Gleam/Erlang FFI, Elixir, and Erlang. The audit reads the actual source, rejects missing or forbidden fragments, and records a SHA-256 for every source in its receipt. Witnesses are necessary evidence, not a claim that lexical search proves full program equivalence; native compile, lint, runtime, race, and E2E tests remain mandatory.

## Commands

```sh
npm run function-bodies:compile
npm run function-bodies:runtime
npm run function-bodies:e2e
npm run function-bodies:check
```

The runtime audit always attempts to write `target/function-body-contract/receipt.json`. Schema, semantic, binding, or source drift blocks promotion. The CI workflow retains the receipt even when the gate fails.
