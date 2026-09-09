# TJSV cross-runtime contract gate

Tracking: DEN-3959. This supplements, rather than replaces, the existing Rust,
TypeScript/Node/Bun/Deno, Go, Gleam, Elixir, Erlang, generated-runtime, persistence,
function-body, and adversarial workflows.

## Independent authorities, executable evidence

`contracts/tjsv.matrix.json` enumerates the five existing contract surfaces:
middleware, docs serving, function bodies, idempotency persistence, and rate-limit
v2. `scripts/tjsv-check.mjs` invokes the actual `runCheck` API from
`ORESoftware/typespec-json-schema-validator`, pinned to the complete Git commit
recorded in the matrix. It does not copy, approximate, or replace TJSV with a
regex scanner. The checked-out validator must have that exact HEAD and clean
tracked source; its own lockfile supplies its dependencies.

For each surface, TJSV inventories the TypeSpec declarations, compiles a fresh
JSON Schema witness, compares the witness against the independently authored
JSON Schema, and executes both validators on synthesized instance probes.
Formats are assertions, object schemas are sealed, and the existing numeric
int64 wire strategy is explicit. Neither input is rewritten. There are no
ignored declarations, skipped lanes, or continue-on-error approval paths.

Every receipt must prove nonempty declaration coverage, compiler availability,
source immutability, both structural and instance-validation coverage, zero
unexplained findings, zero refusals, and consistent per-declaration probe
accounting. A `passed` string or exit zero without that evidence is rejected.
Missing tools fail startup; mismatches block the gate while the remaining lanes
still run to collect diagnostics. Fresh receipts and witnesses go only under
ignored `target/tjsv/`, tagged in the matrix summary with the source commit and
validator revision. Old output cannot be reused as current evidence.

The compiler-backed controls require a genuine matching pair to pass, a
one-sided scalar change to produce a counterexample, and a contradictory
independent corpus expectation to fail even when the two validators agree.
Dependency-free receipt-gate tests additionally reject malformed, incomplete,
empty, disabled, truncated, inconsistent, or incorrectly attributed evidence.

## Running locally

Install the root locked dependencies, then create the tool checkout at
`target/tools/tjsv` and check out the exact revision recorded in the matrix.
Do not overwrite an existing checkout with uncommitted work. Run:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm ci --prefix target/tools/tjsv --ignore-scripts --no-audit --no-fund
node --test tests/tjsv-evidence.test.mjs tests/tjsv-compiler.test.mjs
node scripts/tjsv-check.mjs
```

The scripts have fixed policy and accept no custom gate options. The TJSV
compiler CLI uses upstream's canonical flags-2-env implementation. No secondary
CLI option parser is introduced here. Run the dependency-free receipt tests
alone before the tool checkout is available:

```sh
node --test tests/tjsv-evidence.test.mjs
```

## Release and rollout boundaries

A new failing receipt is not permission to make the authored schema match the
emitter mechanically. Inspect declaration identity, semantics, runtime fixtures,
and both author histories. A naming-only mapping must be explicit and reviewed;
missing declarations, constraints, conditional invariants, or runtime support
must not be hidden by an ignore list. Keep the PR blocked until discrepancies
are genuinely reconciled. This initial integration may expose differences that
the previous local comparators did not report.

TJSV checks data-shape parity; it does **not** establish that a framework handler,
TCP or WebSocket transport, function body, authentication provider, timeout,
retry, or database adapter behaves correctly. Keep all existing runtime tests,
negative admission tests, and independent authority-by-runtime witnesses
mandatory for each supported adapter. Successful schema evidence is not a claim
that every named framework/runtime has been exercised, deployed, or certified.

The new GitHub Actions job runs for pull requests and main/dev pushes, checks out
the exact PR head, and retains evidence even on failure. Merge requires green
exact-head TJSV and existing runtime checks plus the repository's review policy.
This workflow does not change repository rulesets or make itself a required
status check administratively; that governance setting must be verified before
claiming server-side merge enforcement.
