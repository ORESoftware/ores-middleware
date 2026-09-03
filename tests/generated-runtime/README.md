# Generated native runtime convergence

This directory contains executable consumers for the two independently authored persistence-contract authorities:

- TypeSpec; and
- JSON Schema/OpenAPI.

Neither authority is generated from, subordinate to, or used as a fallback for the other. The Rust generator emits separate artifacts for both lanes, and `scripts/generated_runtime_matrix.mjs` executes each lane through TypeScript, Rust, Go, Gleam, Elixir, and Erlang. Promotion requires twelve executed witnesses, zero unexplained findings, and equal normalized behavior.

## Runtime invariants

The witness matrix deliberately checks language-specific boundaries that a text-only parity comparison can miss:

- Rust optional decoders use one propagation operator; nullable values remain `Option<T>` rather than being accidentally unwrapped twice.
- Generated Gleam source is accepted by `gleam format --check`; formatter drift is treated as generated-source drift.
- Elixir normalizes the argument separator inserted by `elixir -- ...` before reading the fixture, generated source, and authority identifiers.
- Erlang models the wire enum as `binary()` because Erlang types cannot express singleton binary literals. Exact allowed values remain enforced by exhaustive `valid_idempotency_status/1` clauses.

These constraints belong to the generated-code and executable-witness contract. A failure enters `STOPPED_FOR_EVALUATION`; no authority, runtime, ORM, or previously green revision wins automatically.

## Evidence boundary

A successful repair workflow is not final pull-request evidence when its source commit was authored with `GITHUB_TOKEN`, because GitHub may report subsequent workflows as `action_required` without executing jobs. Final promotion therefore requires a maintainer-authored exact head followed by successful contract conformance, generated runtime convergence, real PostgreSQL/ORM convergence, adversarial middleware, semantic audit, and Zed package/install workflows.
