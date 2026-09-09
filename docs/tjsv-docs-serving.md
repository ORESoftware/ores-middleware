# TJSV docs-serving contract gate — initial integration slice

This additive gate executes `ORESoftware/typespec-json-schema-validator` at
`6bb5b7c1ee41c8b43741e50a264c33a1165549c4` against the two existing,
independently human-authored docs-serving authorities. JSON Schema is unchanged;
TypeSpec gains explicit object-closure annotations without changing its fields. Existing Rust, native-language, schema, transport, and formal checks
remain necessary and unchanged.

## Scope and limits

The covered declarations are `DocsRequest`, `DocsDecision`, `DocsAction`, and
`DocsRepresentation`. The corpus contains both accepted and rejected examples
for every declaration. Tests include unexpected properties, missing fields,
nulls, wrong scalar types, digests, enums, and uint16 bounds. Empty method/path
strings are intentionally data-valid under the current authorities; HTTP
semantics still require application/runtime checks.

This does not establish parity for the main middleware configuration,
adapter descriptors, persistence, rate limiting, or function-body contracts.
It does not certify Go, Rust, BEAM/Gleam, Node, Bun, Deno, TCP, HTTP, WebSocket,
or framework-specific runtime behavior. TJSV data-shape parity is necessary
but not sufficient for that evidence.

## Explicit object closure

The authored JSON Schema closes `DocsRequest` and `DocsDecision` with
`additionalProperties: false`. TypeSpec declares the same policy using
`@TypeSpec.JsonSchema.extension("additionalProperties", false)` on each model.
Generic emitter sealing is disabled because it emits `unevaluatedProperties`,
a different structural keyword. Unknown-property rejection remains mandatory
in both models' negative corpus. This is an explicit flat-model policy, not
a waiver or a rule for composed/inherited models. Both authored sources remain
independent; no generated comparison file replaces either one.

## Invocation

Use Node >=22.9. The CI job uses the existing repository Node pin, 22.23.1.
Provision an independent, clean checkout of the pinned TJSV revision at
`tmp/tjsv`, then run:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm ci --prefix tmp/tjsv --ignore-scripts --no-audit --no-fund
npm rebuild --prefix tmp/tjsv @oresoftware/f2e --ignore-scripts=false --no-audit --no-fund
npm run docs-serving:tjsv:test
npm run docs-serving:tjsv
```

The explicit allowlisted rebuild is necessary: the pinned `@oresoftware/f2e`
package has an install script (`cd clients/nodejs && node-gyp rebuild`). Skipping
all install scripts leaves `flags2env.node` absent. Keep the general install
script-disabled, then build this reviewed native package only. A C/C++ build
chain and Python for node-gyp must be available. The workflow verifies that the
native module exists before invoking TJSV. The native compilation itself still
needs a hosted run; the local unit tests do not build it.

The dependency checkout and its lockfile are from the same upstream revision.
No dependency is resolved from floating `main` and there is no `npx` download,
local comparator, copied validator, or pass-on-missing fallback. The upstream
CLI retains its own `flags-2-env` parsing; this repository task accepts no flags.
The compiler executable is explicitly selected from the upstream installation.

The new workflow is additive. `npm run verify` retains its existing behavior;
run this TJSV task as well. Before release, require the new workflow alongside
existing checks in the repository's approval/ruleset process. This patch does
not change GitHub branch protection.

## Failure and evidence handling

Each invocation owns a fresh `target/tjsv/docs-serving-*` directory. It starts
with a failed `gate.json` marker and only marks that invocation passed after
exit-code, receipt, corpus coverage, declaration identity, input immutability,
and upstream revision/clean-tree checks succeed. Admission also requires untruncated
zero finding counts, all declared check coverage, exact namespaced TypeSpec
identities and consistent per-declaration/aggregate probe totals. A prior passing receipt is
never consulted. Both authorities and the full fixture inventory are hashed
before and after execution.

Missing tools, failed compiler processes, signals, timeouts, malformed or
missing reports, discrepancies, refused validators, absent probes, and
incomplete coverage fail the task. Child-process environments omit credentials,
Node injection settings, and TJSV environment overrides. Subprocess output is
not echoed; inspect the bounded, fresh `parity.json` when available. Receipts
may contain synthetic witness values; do not replace this corpus with private
production payloads.

This is a trusted-source CI runner, not an operating-system sandbox for hostile
TypeSpec programs or concurrently hostile filesystem writers. Dependency
installation needs approved network access; missing cross-repository access
must be repaired through normal scoped credentials, never hardcoded tokens.

## Validation owed before merge

The unit suite uses an explicit subprocess test double to exercise admission
and failure handling. It must not be reported as a real TJSV compiler run.
Run the pinned upstream CLI in CI, inspect any structural or behavioral
findings, and reconcile the authorities semantically. Never silence findings
by disabling probes, ignoring declarations, or auto-generating an authority
from its peer. Run all existing repository checks before merging.

## Further rollout

Create explicit declaration mappings and independent instance corpora for the
remaining contract families. Main configuration schemas need special attention
to int64 wire representation, root models versus `$defs`, duplicate declaration
names, and additional-property policy. Validate runtime adapters separately
using TJSV runtime-conformance evidence bound to current inputs and exact
consumer revisions, with genuine sibling `*-test` execution. No such runtime
execution is claimed by this initial docs-serving slice.
