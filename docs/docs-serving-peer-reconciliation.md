# Docs-serving peer reconciliation — DEN-3959

This change addresses the eight docs-serving structural findings recorded by
`ores-middleware#66` and `#65`. It does not certify the other contract surfaces.
Both sources remain independently maintained; no generated authority overwrite,
comparison ignore list, waiver, or relaxed TJSV option is introduced.

## Semantic decisions

`DocsRequest` and `DocsDecision` stay closed. Their authored JSON Schema keeps
`additionalProperties: false` and additionally records
`unevaluatedProperties: false`. The independent TypeSpec lane explicitly records
`additionalProperties: false` and retains the pinned emitter's closed-object
policy. No existing additional-property enforcement is removed. These models
are flat; adding the unevaluated guard does not permit any formerly invalid
instance. Future composition/inheritance requires renewed semantic review.

Headers remain an open map of string values, including the empty map. Both
sources now name this wire type `DocsHeaders`; the TypeSpec lane uses
`Record<string>`, not a manually copied generated schema. The schema lane
references its independently authored definition. Both additional- and
unevaluated-property constraints require string values. The map is not closed,
nullable, coerced, or restricted to a hard-coded header-name allowlist.

The root request/decision union, optional properties, enum values, digest
patterns and uint16 status bounds are unchanged. Existing runtime generators,
SDK implementations, and framework/native/ORM workflows are untouched.

## Executable evidence

`.github/workflows/docs-serving-peer-contract.yml` checks the exact PR head and
pins `ORESoftware/typespec-json-schema-validator` to
`4473504c4c9d2831d825919f70c03994d8ce01d2`. Both dependency installs use their
existing lockfiles and disable lifecycle scripts. Workflow permissions are
read-only; checkout credentials are not persisted.

The independent corpus in `tests/docs-serving-peer-cases.mjs` must produce the
same specified verdicts against the historical schema at
`6183cc877d6c058349adc733d325297c07d1c063` and the reconciled schema. The old
header-map check uses its historical inline schema. Actual pinned TJSV then
compiles the peer TypeSpec source and compares all five declarations, including
the named header map, with the complete corpus and synthesized probes. Missing
models, truncated findings, unavailable compilers, refusals, mismatches or an
unclean source closure fail the gate. A one-sided string-to-integer drift must
be rejected structurally and behaviorally. A passing receipt is written last.

Run after installing the pinned tool at `target/tools/tjsv`:

```sh
node --test tests/docs-serving-peers.test.mjs
```

Receipts and witnesses are disposable evidence under `target/docs-serving-peers`.
Finite schema tests are not universal-equivalence proofs or native transport
certification. All existing exact-head language, runtime, ORM, and review gates
remain necessary. Broader TJSV PRs must merge this correction semantically and
rerun their own complete matrices; an earlier receipt cannot certify a new head.
