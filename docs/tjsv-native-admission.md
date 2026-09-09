# TJSV native contract admission

The `generated-runtime-convergence` workflow uses
`ORESoftware/typespec-json-schema-validator` at immutable commit
`4473504c4c9d2831d825919f70c03994d8ce01d2`, with its own npm lockfile.
This checkout is a build-time tool, not an assertion of frozen Zed package publication.

The independently authored sources remain
`contracts/persistence/idempotency-record.tsp` and
`contracts/persistence/idempotency-record.schema.json`. Neither source is replaced.
TJSV emits a comparison-only witness, executes both schemas against the independent
positive/negative corpus with date-time format assertion enabled, and emits a
source-bound Contract IR. Unexplained disagreement blocks the workflow.

The existing generator and native harness still execute both authority lanes in
Node.js, Rust, Go, Gleam, Elixir, and Erlang (12 cells). The new bridge reads their
actual results, checks complete unique case/cell inventories and normalized
roundtrips, and rechecks source, harness, corpus, artifact, and result digests.
TJSV then verifies the IR against the current source files and admits every
required adapter/case. Missing, skipped, duplicate, stale, or discrepant evidence
cannot pass. Digests detect drift; these receipts are not signatures or protection
against a malicious actor who can replace the trusted workflow and all its inputs.

Artifacts under `target/tjsv-native/` include parity, IR, runtime evidence,
provenance, runtime admission, and seven real-TJSV rejection regressions.
A failure leaves a non-passing runtime report rather than a stale success.
Filesystem regressions cover corpus preservation, reused output, symlinked inputs,
bounded reads, malformed input, and missing-tool failure tombstones.
The pinned flags-2-env native addon is rebuilt explicitly after dependency install;
disabling every install script without that rebuild leaves TJSV unable to parse flags.
The separate dependency-free bridge tests run with:

```sh
node --test tests/tjsv-native-*.test.mjs
```

This gate covers the IdempotencyRecord data boundary, not every middleware model
or transport. Node.js execution does not certify Bun, Deno, Next.js, NestJS,
Express, Hono, Hapi, arbitrary Rust servers, TCP framing, or WebSocket lifecycle.
Those need their own actual adapter executions and required-evidence inventories.
The existing HTTP request-validation ports, auth rules, native suites, and other
semantic gates remain required; a schema pass does not establish authorization,
transaction semantics, or complete production deployment readiness.

Linear: DEN-3828.

## Explicit compiler-visible persistence policy

The first real compiler run found three structural differences despite 84 agreeing
instance probes: the emitter's generic sealing option used `unevaluatedProperties`,
the authored wire contract used `additionalProperties`, and SQL policy existed only
as TypeSpec comments rather than emitted metadata. No finding is ignored or waived.

`IdempotencyRecord` now explicitly declares `additionalProperties: false` and the
existing table/primary-key/unique policy using the standard TypeSpec JSON Schema
`@extension` decorator. The original SQL comments remain for the existing native
generator. The authored JSON Schema and model fields are unchanged. Generic emitter
sealing is disabled for this explicit contract because it adds a *different* keyword;
the independently authored unknown-property rejection fixture remains mandatory.
A future composed/inherited model requires a new semantic review of closure policy.

The bridge uses the fully qualified TypeSpec declaration ID, as required by TJSV's
Contract IR, while existing native witness model names remain unchanged. Failed
TJSV admission findings are retained separately from the final passing report.

References:
- https://typespec.io/docs/emitters/json-schema/reference/decorators/#extension
- https://typespec.io/docs/emitters/json-schema/reference/emitter/#seal-object-schemas
